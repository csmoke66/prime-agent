use std::collections::{HashMap, HashSet};

use serde_json::Value;

use super::{AgentsViewScope, Rollup, SelectionKey};
use crate::agents_view_state::{summary_for_record, UnifiedRecord};
use crate::session_cost::{Baseline, Cost};
use crate::subagents::{depth_consistent_binding, is_subagent_summary, summary_parent_keys};

/// The record hierarchy: every record by its aliases, and each record's children by parent
/// linkage.
struct RecordIndex {
    by_key: HashMap<String, usize>,
    children_by_parent: HashMap<usize, Vec<usize>>,
}

/// Build the record hierarchy.
fn build_record_index(records: &[UnifiedRecord]) -> RecordIndex {
    let mut by_key: HashMap<String, usize> = HashMap::new();
    for (index, record) in records.iter().enumerate() {
        for alias in &record.aliases {
            by_key.insert(alias.clone(), index);
        }
    }
    let mut children_by_parent: HashMap<usize, Vec<usize>> = HashMap::new();
    for (index, _) in records.iter().enumerate() {
        let Some(parent) = find_parent_index(records, &by_key, index) else {
            continue;
        };
        if parent == index {
            continue;
        }
        children_by_parent.entry(parent).or_default().push(index);
    }
    RecordIndex {
        by_key,
        children_by_parent,
    }
}

/// The record one parent-reference key list resolves to (daemon summary keys first, then the
/// saved catalog's parent path).
fn find_parent_index(
    records: &[UnifiedRecord],
    by_key: &HashMap<String, usize>,
    index: usize,
) -> Option<usize> {
    parent_keys(&records[index])
        .iter()
        .find_map(|key| by_key.get(key).copied())
        .filter(|parent| *parent != index)
}

/// Every parent-reference key of one record: daemon summary keys first, then the saved catalog's
/// parent path.
fn parent_keys(record: &UnifiedRecord) -> Vec<String> {
    let mut keys = parent_reference_keys(record);
    if let Some(saved) = &record.saved {
        if let Some(parent_path) = saved.get("parentSessionPath").and_then(Value::as_str) {
            if !parent_path.is_empty() {
                keys.push(crate::agents_view_state::file_identity(parent_path));
            }
        }
    }
    keys
}

/// The parent-reference keys of one record's daemon summary.
fn parent_reference_keys(record: &UnifiedRecord) -> Vec<String> {
    record
        .daemon
        .as_ref()
        .map(summary_parent_keys)
        .unwrap_or_default()
}

/// The `parent` record's session file, live summary first, saved catalog
/// row second (both serve absolute paths; an empty string is absent — a
/// `--no-session` worker's in-memory store publishes one, and every other
/// TUI accessor of the fact reads it the same way).
fn parent_record_file(parent: &UnifiedRecord) -> Option<&str> {
    parent
        .daemon
        .as_ref()
        .and_then(|daemon| daemon.get("sessionFile"))
        .and_then(Value::as_str)
        .filter(|file| !file.is_empty())
        .or_else(|| {
            parent
                .saved
                .as_ref()
                .and_then(|saved| saved.get("path"))
                .and_then(Value::as_str)
                .filter(|file| !file.is_empty())
        })
}

/// Whether `parent` sits exactly one level above `child`'s live summary: a fork's source binding
/// sits at the SAME depth and is a sibling, never a parent.
pub(super) fn depth_consistent_parent(daemon: &Value, parent: &UnifiedRecord) -> bool {
    let parent_depth = parent
        .daemon
        .as_ref()
        .and_then(|daemon| daemon.get("rlmDepth"))
        .and_then(Value::as_u64)
        .or_else(|| {
            parent
                .saved
                .as_ref()
                .and_then(|saved| saved.get("rlmDepth"))
                .and_then(Value::as_u64)
        });
    depth_consistent_binding(daemon, parent_record_file(parent), parent_depth)
}

/// Whether `child` rolls up under `parent`: agent lineage only — a branched/forked session links
/// to its source but is a sibling chat, so it never nests or double-books totals. Resident
/// children carry the subagent runtime kind; saved children go by depth.
pub(super) fn is_subagent_descendant(child: &UnifiedRecord, parent: &UnifiedRecord) -> bool {
    if let Some(daemon) = &child.daemon {
        return is_subagent_summary(daemon) || depth_consistent_parent(daemon, parent);
    }
    let child_depth = child
        .saved
        .as_ref()
        .and_then(|saved| saved.get("rlmDepth"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let parent_depth = parent
        .daemon
        .as_ref()
        .and_then(|daemon| daemon.get("rlmDepth"))
        .and_then(Value::as_u64)
        .or_else(|| {
            parent
                .saved
                .as_ref()
                .and_then(|saved| saved.get("rlmDepth"))
                .and_then(Value::as_u64)
        })
        .unwrap_or(0);
    child_depth > parent_depth
}

fn baseline_key(record: &UnifiedRecord) -> &str {
    record
        .aliases
        .iter()
        .find(|key| key.starts_with("file:"))
        .or_else(|| record.aliases.iter().find(|key| key.starts_with("session:")))
        .map_or(record.identity.as_str(), String::as_str)
}

fn raw_cost(record: &UnifiedRecord, field: &str) -> Cost {
    record
        .daemon
        .as_ref()
        .and_then(|row| row.get(field))
        .filter(|usage| !usage.is_null())
        .or_else(|| record.saved.as_ref().and_then(|row| row.get(field)))
        .map_or_else(Cost::default, Cost::from_wire)
}

pub(crate) fn snapshot_costs(records: &[UnifiedRecord], root: usize) -> HashMap<String, Baseline> {
    let index = build_record_index(records);
    let mut stack = vec![root];
    let mut snapshot = HashMap::new();
    while let Some(position) = stack.pop() {
        let record = &records[position];
        let key = baseline_key(record);
        if snapshot.contains_key(key) {
            continue;
        }
        let parent = find_parent_index(records, &index.by_key, position)
            .filter(|parent| is_subagent_descendant(record, &records[*parent]))
            .map(|parent| baseline_key(&records[parent]).to_string());
        snapshot.insert(
            key.to_string(),
            Baseline {
                parent,
                own: raw_cost(record, "usage"),
                deleted: raw_cost(record, "deletedDescendantUsage"),
                ..Baseline::default()
            },
        );
        stack.extend(
            index
                .children_by_parent
                .get(&position)
                .into_iter()
                .flatten()
                .copied()
                .filter(|child| is_subagent_descendant(&records[*child], record)),
        );
    }
    snapshot
}

// A deleted session's raw spend transfers to its nearest surviving ancestor.
// Move its checkpoint along with it unless that ancestor was reset later,
// in which case its own deleted-spend checkpoint already covers the transfer.
fn displaced_baselines<'a>(
    records: &'a [UnifiedRecord],
    baselines: &HashMap<String, Baseline>,
) -> HashMap<&'a str, Cost> {
    if baselines.is_empty() {
        return HashMap::new();
    }
    let visible: HashSet<_> = records.iter().map(baseline_key).collect();
    let mut displaced = HashMap::<_, Cost>::new();
    for (key, baseline) in baselines {
        if visible.contains(key.as_str()) {
            continue;
        }
        let mut ancestor = baseline.parent.as_deref();
        for _ in 0..baselines.len() {
            let Some(parent) = ancestor else { break };
            let parent_baseline = baselines.get(parent);
            if parent_baseline.is_some_and(|current| current.generation > baseline.generation) {
                break;
            }
            if let Some(&present) = visible.get(parent) {
                let moved = displaced.entry(present).or_default();
                *moved += baseline.own;
                *moved += baseline.deleted;
                break;
            }
            ancestor = parent_baseline.and_then(|parent| parent.parent.as_deref());
        }
    }
    displaced
}

pub(crate) fn display_rollups(records: &[UnifiedRecord]) -> HashMap<String, Rollup> {
    map_rollups(records, fold_rollups(records, &crate::session_cost::load()))
}

pub(crate) fn display_cost(records: &[UnifiedRecord], root: usize) -> Option<Cost> {
    fold_rollups(records, &crate::session_cost::load())
        .get(root)
        .map(|(_, cost)| *cost)
}

/// Roll costs and descendant counts over the whole hierarchy, keyed by record identity so
/// filters never change a row's totals.
pub fn compute_rollups(records: &[UnifiedRecord]) -> HashMap<String, Rollup> {
    map_rollups(records, fold_rollups(records, &HashMap::new()))
}

fn fold_rollups(
    records: &[UnifiedRecord],
    baselines: &HashMap<String, Baseline>,
) -> Vec<(Rollup, Cost)> {
    let displaced = displaced_baselines(records, baselines);
    let index = build_record_index(records);
    // Roots first, then breadth-first: the bottom-up pass below sees every
    // child before its parent.
    let mut order: Vec<usize> = records
        .iter()
        .enumerate()
        .filter(|(position, _)| find_parent_index(records, &index.by_key, *position).is_none())
        .map(|(position, _)| position)
        .collect();
    // A growing `order` needs a re-evaluated bound: a `0..order.len()` range captures the roots'
    // length once, and every depth-2+ descendant would silently drop out of the rollup walk.
    let mut slot = 0;
    while slot < order.len() {
        for child in index
            .children_by_parent
            .get(&order[slot])
            .into_iter()
            .flatten()
        {
            order.push(*child);
        }
        slot += 1;
    }
    let mut rollups = vec![(Rollup::default(), Cost::default()); records.len()];
    for position in order.iter().rev() {
        let record = &records[*position];
        let key = baseline_key(record);
        let baseline = baselines.get(key);
        let mut deleted_baseline = baseline.map_or_else(Cost::default, |baseline| baseline.deleted);
        if let Some(moved) = displaced.get(key) {
            deleted_baseline += *moved;
        }
        let mut deleted = raw_cost(record, "deletedDescendantUsage");
        let mut own = raw_cost(record, "usage");
        if deleted_baseline.total != 0.0 {
            deleted = deleted.since(deleted_baseline);
        }
        if let Some(baseline) = baseline {
            own = own.since(baseline.own);
        }
        own += deleted;
        let mut rollup = Rollup {
            cost: own.total,
            descendants: deleted.total,
            descendant_count: 0,
        };
        for child in index.children_by_parent.get(position).into_iter().flatten() {
            if !is_subagent_descendant(&records[*child], &records[*position]) {
                continue;
            }
            let (child_rollup, child_cost) = rollups[*child];
            own += child_cost;
            rollup.descendants += child_rollup.cost;
            rollup.descendant_count += 1 + child_rollup.descendant_count;
        }
        rollup.cost = own.total;
        rollups[*position] = (rollup, own);
    }
    rollups
}

fn map_rollups(records: &[UnifiedRecord], rollups: Vec<(Rollup, Cost)>) -> HashMap<String, Rollup> {
    records
        .iter()
        .zip(rollups)
        .map(|(record, (rollup, _))| (record.identity.clone(), rollup))
        .collect()
}

#[cfg(test)]
mod cost_tests {
    use super::*;
    use crate::agents_view_state::reconcile_unified_sessions;
    use serde_json::json;

    fn usage(input: f64, output: f64, read: f64, write: f64) -> Value {
        let total = input + output + read + write;
        json!({
            "cost": total,
            "costBreakdown": {
                "input": input, "output": output, "cacheRead": read,
                "cacheWrite": write, "total": total
            }
        })
    }

    fn row(id: &str, depth: u64, parent: Option<&str>, usage: Value) -> Value {
        json!({"summary": {
            "sessionId": id, "lifecycle": "live", "rlmDepth": depth,
            "runtimeKind": if depth == 0 { "root" } else { "subagent" },
            "parentSessionId": parent, "usage": usage
        }})
    }

    #[test]
    fn reset_cost_on_subtree_counts_only_new_spend_even_when_children_are_deleted() {
        let root = row("root", 0, None, usage(1.0, 0.0, 0.0, 0.0));
        let child = row("child", 1, Some("root"), usage(0.0, 2.0, 0.0, 0.0));
        let grandchild = row("nested", 2, Some("child"), usage(0.0, 0.0, 3.0, 0.0));
        let sibling = row("fork", 0, Some("root"), usage(0.0, 0.0, 0.0, 7.0));
        let mut roster = vec![root, child, grandchild, sibling];
        let records = reconcile_unified_sessions(&roster, &[], None);
        let checkpoint = snapshot_costs(&records, 0);
        let mut baselines: HashMap<String, Baseline> =
            serde_json::from_slice(&serde_json::to_vec(&checkpoint).unwrap()).unwrap();
        let costs = fold_rollups(&records, &baselines);
        assert_eq!(
            costs.iter().map(|(_, cost)| cost.total).collect::<Vec<_>>(),
            [0.0, 0.0, 0.0, 7.0]
        );

        roster[0]["summary"]["usage"] = usage(2.0, 0.0, 0.0, 0.0);
        roster[1]["summary"]["usage"] = usage(0.0, 2.0, 0.0, 0.5);
        roster[2]["summary"]["usage"] = usage(0.0, 0.0, 4.0, 0.0);
        let records = reconcile_unified_sessions(&roster, &[], None);
        let spent = fold_rollups(&records, &baselines);
        assert_eq!(
            spent[0].1,
            Cost {
                input: 1.0,
                cache_read: 1.0,
                cache_write: 0.5,
                total: 2.5,
                ..Cost::default()
            }
        );
        assert_eq!(spent[1].1.total, 1.5);
        assert_eq!(spent[2].1.total, 1.0);

        // If a child is reset after its child was deleted, their eventual
        // combined deletion must not apply the earlier checkpoint twice.
        let mut sequential = roster.clone();
        sequential.remove(2);
        sequential[1]["summary"]["deletedDescendantUsage"] = usage(0.0, 0.0, 4.0, 0.0);
        let records = reconcile_unified_sessions(&sequential, &[], None);
        let mut later: HashMap<String, Baseline> =
            serde_json::from_slice(&serde_json::to_vec(&baselines).unwrap()).unwrap();
        later.extend(
            snapshot_costs(&records, 1)
                .into_iter()
                .map(|(key, mut baseline)| {
                    baseline.generation = 1;
                    (key, baseline)
                }),
        );
        sequential.remove(1);
        sequential[0]["summary"]["deletedDescendantUsage"] = usage(0.0, 2.0, 4.0, 1.0);
        let records = reconcile_unified_sessions(&sequential, &[], None);
        assert_eq!(fold_rollups(&records, &later)[0].1.total, 1.5);

        roster.drain(1..3);
        roster[0]["summary"]["deletedDescendantUsage"] = usage(0.0, 2.0, 4.0, 0.5);
        let records = reconcile_unified_sessions(&roster, &[], None);
        assert_eq!(fold_rollups(&records, &baselines)[0].1, spent[0].1);

        baselines.extend(
            snapshot_costs(&records, 0)
                .into_iter()
                .map(|(key, mut baseline)| {
                    baseline.generation = 1;
                    (key, baseline)
                }),
        );
        roster[0]["summary"]["deletedDescendantUsage"] = usage(0.0, 2.0, 4.0, 1.0);
        let records = reconcile_unified_sessions(&roster, &[], None);
        assert_eq!(fold_rollups(&records, &baselines)[0].1.cache_write, 0.5);
        assert_eq!(compute_rollups(&records)[&records[0].identity].cost, 9.0);
    }
}

/// The record a scope key resolves to (active id first, then session id).
fn scope_root_index(records: &[UnifiedRecord], scope: &AgentsViewScope) -> Option<usize> {
    if let Some(active) = &scope.active_session_id {
        if let Some(position) = records.iter().position(|record| {
            summary_for_record(record)
                .get("activeSessionId")
                .and_then(Value::as_str)
                == Some(active.as_str())
        }) {
            return Some(position);
        }
    }
    scope.session_id.as_ref().and_then(|session| {
        records.iter().position(|record| {
            summary_for_record(record)
                .get("sessionId")
                .and_then(Value::as_str)
                == Some(session.as_str())
        })
    })
}

/// Restrict records to the scoped root and every descendant (the root itself is included —
/// row building excludes it from the visible roots). `None` when the scope root is not in the
/// record set.
#[must_use]
pub fn scope_to_subtree(
    records: &[UnifiedRecord],
    scope: &AgentsViewScope,
) -> Option<Vec<UnifiedRecord>> {
    let index = build_record_index(records);
    let root = scope_root_index(records, scope)?;
    let mut retained: HashSet<usize> = HashSet::new();
    let mut queue = vec![root];
    let mut cursor = 0;
    while cursor < queue.len() {
        let current = queue[cursor];
        cursor += 1;
        if !retained.insert(current) {
            continue;
        }
        queue.extend(index.children_by_parent.get(&current).into_iter().flatten());
    }
    Some(
        records
            .iter()
            .enumerate()
            .filter(|(position, _)| retained.contains(position))
            .map(|(_, record)| record.clone())
            .collect(),
    )
}

/// Session ids of the scope root's own ancestors, root-most first.
pub fn scope_ancestors(records: &[UnifiedRecord], scope: &AgentsViewScope) -> Vec<String> {
    let index = build_record_index(records);
    let Some(mut current) = scope_root_index(records, scope) else {
        return Vec::new();
    };
    let mut visited: HashSet<usize> = HashSet::new();
    let mut ancestors: Vec<String> = Vec::new();
    while let Some(parent) = find_parent_index(records, &index.by_key, current) {
        if !visited.insert(parent) {
            break;
        }
        ancestors.insert(
            0,
            summary_for_record(&records[parent])
                .get("sessionId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        );
        current = parent;
    }
    ancestors
}

/// Whether `anchor` sits in the subtree of the session `root`. `None` while the record set cannot
/// tell: the anchor is unknown, or its parent chain ends on a parent that has not loaded yet.
pub(crate) fn subtree_contains(
    records: &[UnifiedRecord],
    root: &str,
    anchor: &AgentsViewScope,
) -> Option<bool> {
    let index = build_record_index(records);
    let mut current = scope_root_index(records, anchor)?;
    let mut visited: HashSet<usize> = HashSet::new();
    while visited.insert(current) {
        if summary_for_record(&records[current])
            .get("sessionId")
            .and_then(Value::as_str)
            == Some(root)
        {
            return Some(true);
        }
        if parent_keys(&records[current]).is_empty() {
            return Some(false);
        }
        current = find_parent_index(records, &index.by_key, current)?;
    }
    None
}

/// Whether the session has direct children on the record set.
#[must_use]
pub fn has_session_children(records: &[UnifiedRecord], key: &SelectionKey) -> bool {
    let index = build_record_index(records);
    let root = records.iter().position(|record| {
        let summary = summary_for_record(record);
        let active = summary.get("activeSessionId").and_then(Value::as_str);
        let session = summary.get("sessionId").and_then(Value::as_str);
        match (&key.active_session_id, &key.session_id) {
            (Some(active_key), _) if Some(active_key.as_str()) == active => true,
            (None, Some(session_key)) if Some(session_key.as_str()) == session => true,
            _ => false,
        }
    });
    root.is_some_and(|root| {
        index
            .children_by_parent
            .get(&root)
            .is_some_and(|children| !children.is_empty())
    })
}

/// The scope root's facts the scoped view renders and creates with:
/// `child_depth` is the root's `rlmDepth + 1` (TS `getAgentsViewDepth`) —
/// the view's depth label and the depth a session created in this scope
/// runs at; `session_file` is the root's file exactly as the forest links
/// children to it ([`parent_record_file`]); `cwd` is the root's directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopeRoot {
    pub(crate) child_depth: u32,
    pub(crate) session_file: Option<String>,
    pub(crate) cwd: Option<String>,
}

/// The scope root's facts, `None` when the root is not in the record set.
pub(crate) fn scope_root(records: &[UnifiedRecord], scope: &AgentsViewScope) -> Option<ScopeRoot> {
    scope_root_index(records, scope).map(|root| {
        let summary = summary_for_record(&records[root]);
        // `rlmDepth` rides the wire as a u32 (the create check enforces
        // it); an out-of-range value falls back to the root's 0.
        let root_depth = summary
            .get("rlmDepth")
            .and_then(Value::as_u64)
            .and_then(|depth| u32::try_from(depth).ok())
            .unwrap_or(0);
        ScopeRoot {
            child_depth: root_depth + 1,
            session_file: parent_record_file(&records[root]).map(str::to_string),
            cwd: summary
                .get("cwd")
                .and_then(Value::as_str)
                .filter(|cwd| !cwd.is_empty())
                .map(str::to_string),
        }
    })
}
