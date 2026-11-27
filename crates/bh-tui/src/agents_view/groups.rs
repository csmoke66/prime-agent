//! Local presentation groups, keyed by durable session ID, never roster status.
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{AgentsViewMode, AgentsViewRow, Composer, RowKind, UiInput};
use crate::editor::Editor;
use tokio::sync::mpsc;

pub(super) type Groups = HashMap<String, String>;

pub(super) struct GroupComposer {
    pub(super) editor: Editor,
    session: String,
}

pub(super) struct GroupChange {
    session: String,
    label: String,
}

fn group_path() -> io::Result<PathBuf> {
    bh_types::platform::dirs::agent_dir()
        .map(|dir| dir.join("agents-view-groups"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No Beehive settings directory"))
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn read_groups(dir: &Path) -> io::Result<Groups> {
    let mut groups = Groups::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(groups),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        let file = entry.file_name();
        if let Some(id) = file.to_str().filter(|id| valid_id(id)) {
            let label = match std::fs::read_to_string(entry.path()) {
                Ok(label) => label,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            if !label.is_empty() {
                groups.insert(id.to_owned(), label);
            }
        }
    }
    Ok(groups)
}

fn save_group(dir: &Path, change: &GroupChange) -> io::Result<()> {
    let target = dir.join(&change.session);
    if change.label.is_empty() {
        return match std::fs::remove_file(target) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        };
    }
    std::fs::create_dir_all(dir)?;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let temp = dir.join(format!(
        ".{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let result = std::fs::write(&temp, &change.label).and_then(|()| std::fs::rename(&temp, target));
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

pub(super) fn spawn_load(tx: mpsc::UnboundedSender<UiInput>) {
    tokio::task::spawn_blocking(move || {
        let result = group_path().and_then(|dir| read_groups(&dir));
        let _ = tx.send(UiInput::GroupsLoaded(result.map_err(|e| e.to_string())));
    });
}

pub(super) fn spawn_save(
    tx: mpsc::UnboundedSender<UiInput>,
    change: GroupChange,
) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        let result = group_path().and_then(|dir| save_group(&dir, &change));
        let _ = tx.send(UiInput::GroupSaved(change, result.map_err(|e| e.to_string())));
    })
}

pub(super) fn session_id(row: &AgentsViewRow) -> Option<&str> {
    if row.depth != 0 || row.kind != RowKind::Agent {
        return None;
    }
    row.summary.get("sessionId")?.as_str().filter(|id| valid_id(id))
}

/// Sort whole trees, preserving status and ordering within every group.
pub(super) fn group_rows(rows: &mut Vec<AgentsViewRow>, groups: &Groups) {
    if groups.is_empty() {
        return;
    }
    let mut custom: BTreeMap<&str, Vec<AgentsViewRow>> = BTreeMap::new();
    let mut ungrouped = Vec::new();
    let mut label = None;
    for row in rows.drain(..) {
        if row.depth == 0 {
            label = session_id(&row).and_then(|id| groups.get(id)).map(String::as_str);
        }
        if let Some(label) = label {
            custom.entry(label).or_default().push(row);
        } else {
            ungrouped.push(row);
        }
    }
    rows.extend(custom.into_values().flatten());
    rows.extend(ungrouped);
}

impl AgentsViewMode {
    pub(super) fn group_target(&self) -> Option<&str> {
        if self.scope_active || !self.groups_ready || self.saving_group {
            return None;
        }
        self.rows.get(self.selected).and_then(session_id)
    }

    pub(super) fn enter_group_mode(&mut self) {
        let Some(session) = self.group_target() else {
            return;
        };
        let mut editor = Editor::new();
        editor.set_keybindings(self.keybindings.clone());
        editor.set_text(self.groups.get(session).map_or("", String::as_str));
        self.composer = Composer::Group(Box::new(GroupComposer {
            editor,
            session: session.to_owned(),
        }));
    }

    pub(super) fn handle_group_key(&mut self, mut group: Box<GroupComposer>, key: &str) {
        if self.keybindings.matches(key, "tui.select.cancel") {
            return;
        }
        if self.keybindings.matches(key, "tui.select.confirm") {
            let draft = group.editor.get_text();
            let label = draft.trim();
            if label.chars().count() > 64 || label.chars().any(char::is_control) {
                self.set_status("Use a single-line group name of at most 64 characters");
                self.composer = Composer::Group(group);
                return;
            }
            self.saving_group = true;
            self.pending_group = Some(GroupChange {
                session: group.session,
                label: label.to_owned(),
            });
            return;
        }
        group.editor.handle_input(key);
        let _ = group.editor.take_events();
        self.composer = Composer::Group(group);
    }

    pub(super) fn group_saved(&mut self, change: GroupChange, outcome: Result<(), String>) {
        self.saving_group = false;
        match outcome {
            Ok(()) => {
                if change.label.is_empty() {
                    self.groups.remove(&change.session);
                    self.set_status("Removed from custom group");
                } else {
                    self.set_status(&format!("Moved to {}", change.label));
                    self.groups.insert(change.session, change.label);
                }
                self.rebuild_rows();
            }
            Err(error) => self.set_status(&format!("Could not save group: {error}")),
        }
    }
}
