//! Local display checkpoints. The daemon's usage records and billing are never modified.
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::ops::AddAssign;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use anyhow::{Context, Result};
use bh_core::settings::{atomic_write_with, AtomicWriteOptions};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agents_view_forest::{display_cost, snapshot_costs};
use crate::agents_view_state::reconcile_unified_sessions;
use crate::subagents::SessionIdentity;

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct Cost {
    pub(crate) input: f64,
    pub(crate) output: f64,
    pub(crate) cache_read: f64,
    pub(crate) cache_write: f64,
    pub(crate) unclassified: f64,
    pub(crate) total: f64,
}

impl Cost {
    pub(crate) fn from_wire(usage: &Value) -> Self {
        let total = usage["cost"].as_f64().unwrap_or_default();
        let breakdown = &usage["costBreakdown"];
        Self {
            input: breakdown["input"].as_f64().unwrap_or_default(),
            output: breakdown["output"].as_f64().unwrap_or_default(),
            cache_read: breakdown["cacheRead"].as_f64().unwrap_or_default(),
            cache_write: breakdown["cacheWrite"].as_f64().unwrap_or_default(),
            unclassified: difference(total, breakdown["total"].as_f64().unwrap_or_default()),
            total,
        }
    }

    pub(crate) fn since(self, baseline: Self) -> Self {
        let total = difference(self.total, baseline.total);
        if total == 0.0 {
            return Self::default();
        }
        // Legacy or imported rows may not have category data. An old baseline
        // still subtracts from the total even if the split becomes available later.
        let mut left = Self {
            input: difference(self.input, baseline.input),
            output: difference(self.output, baseline.output),
            cache_read: difference(self.cache_read, baseline.cache_read),
            cache_write: difference(self.cache_write, baseline.cache_write),
            unclassified: 0.0,
            total,
        };
        let classified = left.input + left.output + left.cache_read + left.cache_write;
        if classified > total + 1e-9 {
            // A change in category coverage made it impossible to determine the split.
            left = Self {
                total,
                ..Self::default()
            };
        }
        left.unclassified = difference(
            total,
            left.input + left.output + left.cache_read + left.cache_write,
        );
        left
    }
}

fn difference(current: f64, baseline: f64) -> f64 {
    let delta = current - baseline;
    if delta > 1e-9 {
        delta
    } else {
        0.0
    }
}

impl AddAssign for Cost {
    fn add_assign(&mut self, other: Self) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
        self.unclassified += other.unclassified;
        self.total += other.total;
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct Baseline {
    pub(crate) parent: Option<String>,
    pub(crate) generation: u64,
    pub(crate) own: Cost,
    pub(crate) deleted: Cost,
}

type Baselines = HashMap<String, Baseline>;

fn path() -> Option<PathBuf> {
    Some(bh_types::platform::agent_dir()?.join("cost-baselines.json"))
}

fn read(path: &Path) -> Result<Baselines> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("Read saved cost baselines"),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Baselines::new()),
        Err(error) => Err(error).context("Open saved cost baselines"),
    }
}

struct Cached {
    path: PathBuf,
    modified: SystemTime,
    len: u64,
    baselines: Arc<Baselines>,
}

static CACHE: Mutex<Option<Cached>> = Mutex::new(None);

pub(crate) fn load() -> Arc<Baselines> {
    let Some(path) = path() else {
        return Arc::default();
    };
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Ok(metadata) = fs::metadata(&path) else {
        cache.take();
        return Arc::default();
    };
    let Ok(modified) = metadata.modified() else {
        return Arc::new(read(&path).unwrap_or_default());
    };
    if let Some(entry) = cache.as_ref().filter(|entry| {
        entry.path == path && entry.modified == modified && entry.len == metadata.len()
    }) {
        return Arc::clone(&entry.baselines);
    }
    let baselines = Arc::new(read(&path).unwrap_or_default());
    *cache = Some(Cached {
        path,
        modified,
        len: metadata.len(),
        baselines: Arc::clone(&baselines),
    });
    baselines
}

pub(crate) fn reset(roster: &[Value], session: &SessionIdentity) -> Result<()> {
    let path = path().context("Cannot resolve the Beehive data directory")?;
    let records = reconcile_unified_sessions(roster, &[], session.session_id.as_deref());
    let keys = session.keys();
    let root = records
        .iter()
        .position(|record| record.aliases.iter().any(|key| keys.contains(key)))
        .context("Session cost is not available yet")?;
    let snapshot = snapshot_costs(&records, root);
    // The initial roster can contain edge-only rows before their usage has
    // been loaded. Do not checkpoint those descendants as if they cost zero.
    anyhow::ensure!(
        !roster.iter().any(|row| {
            row["seededCwd"].as_bool() == Some(true)
                && row["summary"]["sessionFile"].as_str().is_some_and(|file| {
                    snapshot.contains_key(&crate::agents_view_state::file_identity(file))
                })
        }),
        "Some sub-agent costs are still loading; try /cost reset again after the roster updates"
    );
    let parent = path.parent().context("Missing cost-baseline directory")?;
    fs::create_dir_all(parent)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(parent.join("cost-baselines.lock"))?;
    lock.lock()?;
    let mut baselines = read(&path)?;
    let generation = baselines
        .values()
        .map(|baseline| baseline.generation)
        .max()
        .unwrap_or_default()
        .checked_add(1)
        .context("Cost baseline generation exhausted")?;
    baselines.extend(snapshot.into_iter().map(|(key, mut baseline)| {
        baseline.generation = generation;
        (key, baseline)
    }));
    atomic_write_with(
        &path,
        &serde_json::to_string(&baselines)?,
        AtomicWriteOptions { fsync: true },
    )?;
    CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    Ok(())
}

pub(crate) fn displayed(roster: &[Value], session: &SessionIdentity) -> Option<Cost> {
    let records = reconcile_unified_sessions(roster, &[], session.session_id.as_deref());
    let keys = session.keys();
    let root = records
        .iter()
        .position(|record| record.aliases.iter().any(|key| keys.contains(key)))?;
    display_cost(&records, root)
}
