//! Read-only browser for stored harness entries, with an explicit delete action.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::theme::{Theme, ThemeColor};
use crate::width::{truncate_line, wrap_text};
use crate::Line;

#[derive(Debug, PartialEq)]
pub(crate) enum HarnessAction {
    None,
    Close,
    Refresh,
    Delete(Value),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Part {
    Scope(String),
    Kind(String),
    Folder(String),
    Entry(String),
}

type Key = Vec<Part>;

struct Node {
    label: String,
    entry: Option<usize>,
}

pub(crate) struct HarnessBrowser {
    entries: Vec<Value>,
    nodes: BTreeMap<Key, Node>,
    expanded: BTreeSet<Key>,
    visible: Vec<Key>,
    cursor: usize,
    tree_start: usize,
    detail_only: bool,
    detail_start: usize,
    detail_page: usize,
    detail_width: usize,
    detail_lines: Vec<Line>,
    error: Option<String>,
    loading: bool,
    fresh: bool,
    confirm: Option<usize>,
}

impl HarnessBrowser {
    pub(crate) fn loading() -> Self {
        Self {
            entries: Vec::new(),
            nodes: BTreeMap::new(),
            expanded: BTreeSet::new(),
            visible: Vec::new(),
            cursor: 0,
            tree_start: 0,
            detail_only: false,
            detail_start: 0,
            detail_page: 1,
            detail_width: 0,
            detail_lines: Vec::new(),
            error: None,
            loading: true,
            fresh: false,
            confirm: None,
        }
    }

    #[cfg(test)]
    fn new(entries: anyhow::Result<Vec<Value>>) -> Self {
        let mut browser = Self::loading();
        browser.replace_entries(entries);
        browser
    }

    /// Apply a read result, returning whether the browser needs repainting.
    pub(crate) fn replace_entries(&mut self, entries: anyhow::Result<Vec<Value>>) -> bool {
        let entries = match entries {
            Ok(entries) => entries,
            Err(error) => return self.show_error(format!("{error:#}")),
        };
        // An idle poll must not reset preview scrolling or race the user's confirmation.
        if self.fresh && self.entries == entries {
            return false;
        }
        let selected = self.visible.get(self.cursor).cloned();
        let entry_identity = selected.as_ref().and_then(|key| match key.last()? {
            Part::Entry(id) => Some((&key[..2], id)),
            _ => None,
        });
        let mut nodes = BTreeMap::new();
        let mut identities = BTreeSet::new();
        for (index, entry) in entries.iter().enumerate() {
            let (scope, kind, id) = (field(entry, "scope"), field(entry, "kind"), field(entry, "id"));
            if !matches!(scope, "local" | "global") || kind.is_empty() || id.is_empty() {
                return self.show_error("Invalid harness entry: expected scope, kind, and ID".into());
            }
            if !identities.insert((scope, kind, id)) {
                return self.show_error("Duplicate harness entry identity".into());
            }
            let mut key = vec![Part::Scope(scope.to_owned())];
            add_branch(&mut nodes, &key, scope);
            key.push(Part::Kind(kind.to_owned()));
            add_branch(&mut nodes, &key, kind);
            for folder in field(entry, "path").split('/').filter(|part| !part.is_empty()) {
                key.push(Part::Folder(folder.to_owned()));
                add_branch(&mut nodes, &key, folder);
            }
            key.push(Part::Entry(id.to_owned()));
            let title = field(entry, "title");
            nodes.insert(
                key,
                Node {
                    label: single_line(if title.is_empty() { id } else { title }),
                    entry: Some(index),
                },
            );
        }
        for key in nodes.keys().filter(|key| key.len() == 1) {
            if !self.nodes.contains_key(key) {
                self.expanded.insert(key.clone());
            }
        }
        self.expanded.retain(|key| nodes.contains_key(key));
        let relocated = selected.as_ref().filter(|key| !nodes.contains_key(*key))
            .and(entry_identity)
            .and_then(|(scope_and_kind, id)| {
                nodes.keys().find(|key| {
                    key.starts_with(scope_and_kind)
                        && matches!(key.last(), Some(Part::Entry(candidate)) if candidate == id)
                }).cloned()
            });
        if let Some(key) = &relocated {
            for depth in 1..key.len() {
                self.expanded.insert(key[..depth].to_vec());
            }
        }
        self.entries = entries;
        self.loading = false;
        self.confirm = None;
        self.nodes = nodes;
        self.error = None;
        self.fresh = true;
        self.rebuild_visible(relocated.as_deref().or(selected.as_deref()));
        self.reset_detail();
        true
    }

    /// Drop the confirmed row before readback so a failed reload cannot show it as present.
    pub(crate) fn remove_entry(&mut self, entry: &Value) {
        let mut entries = std::mem::take(&mut self.entries);
        entries.retain(|candidate| candidate != entry);
        self.fresh = false;
        self.replace_entries(Ok(entries));
        // Only a successful readback re-enables deletion.
        self.fresh = false;
    }

    pub(crate) fn set_error(&mut self, error: impl Into<String>) {
        self.show_error(error.into());
    }

    fn show_error(&mut self, error: String) -> bool {
        if self.error.as_ref() == Some(&error) {
            return false;
        }
        self.loading = false;
        self.fresh = false;
        self.confirm = None;
        self.error = Some(error);
        self.reset_detail();
        true
    }

    fn rebuild_visible(&mut self, selected: Option<&[Part]>) {
        self.visible = self.nodes.keys().filter(|key| {
            (1..key.len()).all(|depth| self.expanded.contains(&key[..depth]))
        }).cloned().collect();
        self.cursor = selected.and_then(|key| {
            (1..=key.len()).rev().find_map(|depth| {
                self.visible.binary_search_by(|row| row.as_slice().cmp(&key[..depth])).ok()
            })
        }).unwrap_or_else(|| self.cursor.min(self.visible.len().saturating_sub(1)));
    }

    fn reset_detail(&mut self) {
        self.detail_start = 0;
        self.detail_width = 0;
        self.detail_lines.clear();
    }

    fn selected_entry(&self) -> Option<usize> {
        self.nodes.get(self.visible.get(self.cursor)?)?.entry
    }

    fn move_cursor(&mut self, cursor: usize) {
        if self.cursor != cursor {
            self.cursor = cursor;
            self.reset_detail();
        }
    }

    fn expand(&mut self, toggle: bool) {
        let Some(key) = self.visible.get(self.cursor) else { return };
        if self.nodes[key].entry.is_some() {
            if toggle {
                self.detail_only = !self.detail_only;
            }
        } else if !self.expanded.contains(key) || toggle {
            let key = key.clone();
            if !self.expanded.remove(&key) {
                self.expanded.insert(key.clone());
            }
            self.rebuild_visible(Some(&key));
            self.reset_detail();
        } else {
            self.move_cursor(self.cursor + 1);
        }
    }

    fn collapse(&mut self) {
        let Some(key) = self.visible.get(self.cursor).cloned() else { return };
        if self.expanded.remove(&key) {
            self.rebuild_visible(Some(&key));
            self.reset_detail();
        } else if key.len() > 1 {
            self.rebuild_visible(Some(&key[..key.len() - 1]));
            self.reset_detail();
        }
    }

    fn scroll_detail(&mut self, down: bool, amount: usize) {
        self.detail_start = if down {
            self.detail_start.saturating_add(amount)
                .min(self.detail_lines.len().saturating_sub(self.detail_page))
        } else {
            self.detail_start.saturating_sub(amount)
        };
    }

    pub(crate) fn handle_key(&mut self, key: &str) -> HarnessAction {
        if let Some(index) = self.confirm {
            match key {
                "up" => self.scroll_detail(false, 1),
                "down" => self.scroll_detail(true, 1),
                "pageup" | "pageUp" | "pgup" => self.scroll_detail(false, self.detail_page),
                "pagedown" | "pageDown" | "pgdown" => self.scroll_detail(true, self.detail_page),
                "y" if self.fresh && self.selected_entry() == Some(index) => {
                    self.confirm = None;
                    self.fresh = false;
                    self.reset_detail();
                    return HarnessAction::Delete(self.entries[index].clone());
                }
                "escape" | "esc" | "ctrl+c" | "enter" | "return" | "n" => {
                    self.confirm = None;
                    self.reset_detail();
                }
                _ => {}
            }
            return HarnessAction::None;
        }
        match key {
            "escape" | "esc" | "ctrl+c" => return HarnessAction::Close,
            "r" => return HarnessAction::Refresh,
            "tab" => self.detail_only = !self.detail_only,
            "up" if self.detail_only => self.scroll_detail(false, 1),
            "down" if self.detail_only => self.scroll_detail(true, 1),
            "up" => self.move_cursor(self.cursor.saturating_sub(1)),
            "down" => self.move_cursor((self.cursor + 1).min(self.visible.len().saturating_sub(1))),
            "pageup" | "pageUp" | "pgup" => self.scroll_detail(false, self.detail_page),
            "pagedown" | "pageDown" | "pgdown" => self.scroll_detail(true, self.detail_page),
            "left" | "enter" | "return" if self.detail_only => self.detail_only = false,
            "left" => self.collapse(),
            "right" if !self.detail_only => self.expand(false),
            "enter" | "return" => self.expand(true),
            "delete" | "d" if self.fresh => {
                if let Some(index) = self.selected_entry() {
                    self.confirm = Some(index);
                    self.reset_detail();
                }
            }
            _ => {}
        }
        HarnessAction::None
    }

    fn update_detail(&mut self, width: usize) {
        if self.detail_width == width {
            return;
        }
        let mut text = String::new();
        if let Some(index) = self.confirm {
            let entry = &self.entries[index];
            text.push_str(&format!("Delete {} entry?\n", field(entry, "scope")));
            for key in ["title", "kind", "path", "id"] {
                text.push_str(&format!("{key}: {}\n", single_line(field(entry, key))));
            }
            text.push_str("y: delete · Enter/Esc: cancel");
        } else {
            if let Some(error) = &self.error {
                text.push_str("Harness error: ");
                text.push_str(&clean_text(error));
                text.push_str("\n\n");
            }
            if let Some(index) = self.selected_entry() {
                let entry = &self.entries[index];
                if let Some(content) = entry.get("content") {
                    append_value(&mut text, "content", content, 0);
                    text.push('\n');
                }
                for field in ["title", "scope", "kind", "path", "id", "source", "created_at", "updated_at", "version"] {
                    if let Some(value) = entry.get(field) {
                        append_value(&mut text, field, value, 0);
                    }
                }
                for field in ["reference", "arguments", "metadata"] {
                    if let Some(value) = entry.get(field) {
                        text.push('\n');
                        append_value(&mut text, field, value, 0);
                    }
                }
            } else if self.loading {
                text.push_str("Loading harness entries…");
            } else if self.nodes.is_empty() && self.error.is_none() {
                text.push_str("No harness entries.");
            } else if self.error.is_none() {
                text.push_str("Right/Enter: expand · Left: collapse");
            }
        }
        self.detail_lines = wrap_text(text.trim_end_matches('\n'), width);
        self.detail_width = width;
    }

    pub(crate) fn render(&mut self, theme: &Theme, width: usize, height: usize) -> Vec<Line> {
        if width == 0 || height == 0 {
            return Vec::new();
        }
        self.update_detail(width);
        let confirmation = self.confirm.is_some();
        let show_tree = !self.detail_only && !confirmation && !self.visible.is_empty()
            && (height > 2 || self.error.is_none());
        let footer = usize::from(height > 1);
        let body = height - footer;
        let tree_rows = if !show_tree {
            0
        } else if body == 1 {
            1
        } else {
            self.visible.len().min(body / 2)
        };
        let detail_rows = body - tree_rows;
        self.detail_page = detail_rows.max(1);
        self.detail_start = self.detail_start.min(self.detail_lines.len().saturating_sub(self.detail_page));
        let mut lines = Vec::with_capacity(height);
        if tree_rows > 0 {
            self.tree_start = self.tree_start.min(self.cursor)
                .max((self.cursor + 1).saturating_sub(tree_rows))
                .min(self.visible.len() - tree_rows);
            for (offset, key) in self.visible[self.tree_start..][..tree_rows].iter().enumerate() {
                let node = &self.nodes[key];
                let selected = self.tree_start + offset == self.cursor;
                let marker = if node.entry.is_some() {
                    "·"
                } else if self.expanded.contains(key) {
                    "▾"
                } else {
                    "▸"
                };
                let indent = " ".repeat(((key.len() - 1) * 2).min(width / 3));
                lines.push(truncate_line(&vec![theme.fg_span(
                    if selected { ThemeColor::Accent } else { ThemeColor::Muted },
                    format!("{}{indent}{marker} {}", if selected { "›" } else { " " }, node.label),
                )], width, ""));
            }
        }
        lines.extend(
            self.detail_lines
                .iter()
                .skip(self.detail_start)
                .take(detail_rows)
                .cloned(),
        );
        if footer != 0 {
            let hint = if confirmation {
                "Enter/Esc: cancel · y: delete · ↑↓: scroll"
            } else if self.detail_only {
                "Tab: tree · ↑↓: scroll · Esc: close"
            } else if self.error.is_some() {
                "Harness error · Tab: detail · r: refresh · Esc: close"
            } else {
                "Tab: detail · PgUp/Dn: scroll · d: delete · r: refresh · Esc: close"
            };
            let color = if confirmation { ThemeColor::Warning } else { ThemeColor::Muted };
            lines.push(truncate_line(&vec![theme.fg_span(color, hint)], width, ""));
        }
        lines
    }
}

fn field<'a>(entry: &'a Value, key: &str) -> &'a str {
    entry.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn add_branch(nodes: &mut BTreeMap<Key, Node>, key: &[Part], label: &str) {
    if !nodes.contains_key(key) {
        nodes.insert(key.to_vec(), Node { label: single_line(label), entry: None });
    }
}

fn clean_text(text: &str) -> String {
    let mut text = crate::ansi::strip_ansi(text)
        .replace("\r\n", "\n")
        .replace(['\r', '\u{2028}', '\u{2029}'], "\n")
        .replace('\t', "    ");
    text.retain(|ch| {
        ch == '\n'
            || (!ch.is_control()
                && !matches!(
                    ch,
                    '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
                ))
    });
    text
}

fn single_line(text: &str) -> String {
    clean_text(text).replace('\n', " ")
}

// Render strings, including inline skill code, as text rather than JSON escapes.
// Objects and arrays keep their keys/indices so nested reference fields are readable.
fn append_value(out: &mut String, label: &str, value: &Value, depth: usize) {
    let indent = " ".repeat(depth * 2);
    out.push_str(&indent);
    out.push_str(&single_line(label));
    out.push(':');
    match value {
        Value::Object(fields) if !fields.is_empty() => {
            out.push('\n');
            for (key, value) in fields {
                append_value(out, key, value, depth + 1);
            }
        }
        Value::Array(items) if !items.is_empty() => {
            out.push('\n');
            for (index, value) in items.iter().enumerate() {
                append_value(out, &format!("[{index}]"), value, depth + 1);
            }
        }
        Value::String(value) => {
            let value = clean_text(value);
            if value.contains('\n') || label == "content" {
                out.push('\n');
            } else {
                out.push(' ');
            }
            out.push_str(&value);
            out.push('\n');
        }
        _ => {
            out.push(' ');
            out.push_str(&value.to_string());
            out.push('\n');
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::theme::ColorMode;

    fn entry(scope: &str, kind: &str, path: &str, id: &str) -> Value {
        json!({
            "scope": scope, "kind": kind, "path": path, "id": id, "title": id,
            "content": format!("{id} body"), "version": 1,
        })
    }

    fn keys(browser: &mut HarnessBrowser, keys: &[&str]) {
        for key in keys {
            assert_eq!(browser.handle_key(key), HarnessAction::None);
        }
    }

    fn screen(browser: &mut HarnessBrowser, width: usize, height: usize) -> String {
        browser
            .render(&Theme::builtin("beehive", ColorMode::TrueColor), width, height)
            .into_iter()
            .map(|line| line.into_iter().map(|span| span.content.to_string()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn tree_folds_nested_paths_and_refresh_tracks_scoped_kind_identity() {
        let global = entry("global", "prompt", "a/b", "same");
        let local = entry("local", "prompt", "a/b", "same");
        let mut browser = HarnessBrowser::new(Ok(vec![local.clone(), global]));
        assert_eq!(browser.visible.len(), 4);
        keys(&mut browser, &["right", "enter", "right", "right", "right", "right", "right"]);
        assert_eq!(browser.selected_entry(), Some(1));
        keys(&mut browser, &["left", "left"]);
        assert!(!browser.visible.iter().any(|key| key.len() == 5));
        keys(&mut browser, &["right", "right"]);
        assert_eq!(browser.selected_entry(), Some(1));

        let moved = entry("global", "prompt", "c/d", "same");
        let other_kind = entry("global", "memory", "a/b", "same");
        browser.replace_entries(Ok(vec![moved.clone(), local, other_kind]));
        assert_eq!(browser.selected_entry(), Some(0));
        assert!(screen(&mut browser, 60, 12).contains(" c"));
        assert!(!browser.visible.iter().any(|key| key.contains(&Part::Folder("a".into()))));
    }

    #[test]
    fn delete_is_scoped_default_cancel_and_invalidated_by_reload() {
        let mut local = entry("local", "prompt", "general", "same");
        let global = entry("global", "prompt", "general", "same");
        let mut browser = HarnessBrowser::new(Ok(vec![global.clone(), local.clone()]));
        keys(&mut browser, &["down", "down", "right", "right", "right", "right", "right"]);
        assert_eq!(browser.selected_entry(), Some(1));
        keys(&mut browser, &["delete"]);
        assert!(screen(&mut browser, 50, 8).contains("Delete local entry?"));
        keys(&mut browser, &["enter", "y", "delete"]);
        local["version"] = json!(2);
        browser.replace_entries(Ok(vec![global, local.clone()]));
        keys(&mut browser, &["y", "delete"]);
        assert_eq!(browser.handle_key("y"), HarnessAction::Delete(local));
    }

    #[test]
    fn detail_follows_selection_and_inline_reference_is_scrollable_and_safe() {
        let mut skill = entry("local", "skill", "a", "a");
        skill["reference"] = json!({
            "python_import": "example", "callable": "run",
            "inline": {"source": "def run():\n    return 123\u{1b}[31m\u{7}\u{202e}"},
        });
        let other = entry("local", "skill", "a", "b");
        let mut browser = HarnessBrowser::new(Ok(vec![other, skill]));
        keys(&mut browser, &["right", "right", "right", "right", "right"]);
        let first = screen(&mut browser, 36, 8);
        assert!(first.contains("a body"));
        keys(&mut browser, &["down"]);
        assert!(screen(&mut browser, 36, 8).contains("b body"));
        keys(&mut browser, &["up", "tab"]);
        let mut seen = String::new();
        for _ in 0..40 {
            let row = screen(&mut browser, 24, 1);
            assert!(!row.contains(['\u{1b}', '\u{7}', '\u{202e}']));
            seen.push_str(&row);
            keys(&mut browser, &["down"]);
        }
        assert!(seen.contains("python_import: example"));
        assert!(seen.contains("def run():"));
        assert!(seen.contains("return 123"));
    }

    #[test]
    fn empty_and_read_errors_are_rendered_and_stale_entries_cannot_be_deleted() {
        let mut browser = HarnessBrowser::new(Ok(Vec::new()));
        assert!(screen(&mut browser, 40, 1).contains("No harness entries"));
        browser.replace_entries(Ok(vec![entry("local", "memory", "", "memory")]));
        keys(&mut browser, &["right", "right", "right", "delete"]);
        browser.replace_entries(Err(anyhow::anyhow!("cannot reload")));
        keys(&mut browser, &["y", "delete", "y"]);
        assert!(screen(&mut browser, 40, 8).contains("cannot reload"));
        assert_eq!(browser.handle_key("r"), HarnessAction::Refresh);
        assert_eq!(browser.handle_key("escape"), HarnessAction::Close);
    }

    #[test]
    fn unchanged_polls_do_not_request_repaint_or_cancel_confirmation() {
        let rows = vec![entry("local", "memory", "", "memory")];
        let mut browser = HarnessBrowser::loading();
        assert!(browser.replace_entries(Ok(rows.clone())));
        keys(&mut browser, &["right", "right", "right", "delete"]);
        assert!(!browser.replace_entries(Ok(rows)));
        assert!(browser.confirm.is_some());
        assert!(browser.replace_entries(Err(anyhow::anyhow!("cannot reload"))));
        assert!(browser.confirm.is_none());
        assert!(!browser.replace_entries(Err(anyhow::anyhow!("cannot reload"))));
    }

    #[test]
    fn confirmed_removal_survives_failed_readback() {
        let mut browser = HarnessBrowser::loading();
        assert!(screen(&mut browser, 50, 4).contains("Loading harness entries"));

        let local = json!({"scope": "local", "kind": "prompt", "id": "same"});
        let global = json!({"scope": "global", "kind": "prompt", "id": "same"});
        browser.replace_entries(Ok(vec![local.clone(), global.clone()]));
        browser.remove_entry(&local);
        assert!(!browser.fresh);
        browser.replace_entries(Err(anyhow::anyhow!("cannot reload")));
        assert_eq!(browser.entries, vec![global]);
    }
}
