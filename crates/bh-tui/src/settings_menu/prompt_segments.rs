//! Keyboard editor for the current session's named system-prompt segments.

use bh_types::daemon::{PromptSegmentEdit, PromptSegmentState, SessionPromptSegment};

use crate::editor::Editor;
use crate::keybindings::KeybindingsManager;
use crate::menu_panel::{self, MenuSegment};
use crate::search_input::SearchInput;
use crate::theme::Theme;
use crate::{Line, Span};

use super::{hint_row, SettingsMenuAction};

enum Entry {
    List,
    Name(SearchInput),
    Body {
        name: String,
        added: bool,
        editor: Box<Editor>,
    },
}

pub(super) struct PromptSegmentsMenu {
    rows: Vec<SessionPromptSegment>,
    filtered: Vec<usize>,
    selected: usize,
    search: SearchInput,
    entry: Entry,
}

impl std::fmt::Debug for PromptSegmentsMenu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PromptSegmentsMenu")
            .field("selected", &self.selected)
            .finish_non_exhaustive()
    }
}

impl PromptSegmentsMenu {
    pub(super) fn new(rows: Vec<SessionPromptSegment>) -> Self {
        Self {
            filtered: (0..rows.len()).collect(),
            rows,
            selected: 0,
            search: SearchInput::new(),
            entry: Entry::List,
        }
    }

    pub(super) fn saved(&mut self, rows: Vec<SessionPromptSegment>) {
        self.rows = rows;
        self.entry = Entry::List;
        self.filter();
    }

    fn filter(&mut self) {
        let query = self.search.value().trim().to_ascii_lowercase();
        self.filtered = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                row.name
                    .to_ascii_lowercase()
                    .contains(&query)
                    .then_some(index)
            })
            .collect();
        self.selected = self.selected.min(self.filtered.len().saturating_sub(1));
    }

    fn edit_body(&mut self, name: String, added: bool, body: &str, keys: &KeybindingsManager) {
        let mut editor = Editor::new();
        editor.set_keybindings(keys.clone());
        editor.set_text(body);
        self.entry = Entry::Body {
            name,
            added,
            editor: Box::new(editor),
        };
    }

    pub(super) fn paste(&mut self, text: &str) {
        match &mut self.entry {
            Entry::List => {
                self.search.paste(text);
                self.filter();
            }
            Entry::Name(input) => input.paste(text),
            Entry::Body { editor, .. } => editor.insert_text_at_cursor(text),
        }
    }

    pub(super) fn handle_key(
        &mut self,
        key: &str,
        keys: &KeybindingsManager,
    ) -> SettingsMenuAction {
        use SettingsMenuAction::{ChangePromptSegment, None, SubmenuClosed};
        if keys.matches(key, "tui.select.cancel") {
            if matches!(self.entry, Entry::List) {
                return SubmenuClosed;
            }
            self.entry = Entry::List;
            return None;
        }
        match &mut self.entry {
            Entry::Body {
                name,
                added,
                editor,
            } => {
                if key == "ctrl+s" {
                    let name = name.clone();
                    let body = editor.get_text();
                    return ChangePromptSegment(if *added {
                        PromptSegmentEdit::Add { name, body }
                    } else {
                        PromptSegmentEdit::Replace { name, body }
                    });
                }
                if key == "enter" {
                    editor.insert_text_at_cursor("\n");
                } else {
                    editor.handle_input(key);
                }
            }
            Entry::Name(input) => {
                if keys.matches(key, "tui.select.confirm") && !input.value().trim().is_empty() {
                    let name = input.value().trim().to_owned();
                    self.edit_body(name, /*added*/ true, "", keys);
                } else {
                    input.handle_key(key, keys);
                }
            }
            Entry::List => {
                if key == "ctrl+n" {
                    self.entry = Entry::Name(SearchInput::new());
                } else if keys.matches(key, "tui.select.up") {
                    self.selected = self.selected.saturating_sub(1);
                } else if keys.matches(key, "tui.select.down") {
                    self.selected = (self.selected + 1).min(self.filtered.len().saturating_sub(1));
                } else if key == "ctrl+d"
                    || key == "ctrl+r"
                    || keys.matches(key, "tui.select.confirm")
                {
                    if let Some(&index) = self.filtered.get(self.selected) {
                        let row = &self.rows[index];
                        let name = row.name.clone();
                        match key {
                            "ctrl+d" => {
                                return ChangePromptSegment(PromptSegmentEdit::Remove { name })
                            }
                            "ctrl+r" => {
                                return ChangePromptSegment(PromptSegmentEdit::Reset { name })
                            }
                            _ => {
                                let body = row.body.clone();
                                self.edit_body(name, /*added*/ false, &body, keys);
                            }
                        }
                    }
                } else {
                    self.search.handle_key(key, keys);
                    self.selected = 0;
                    self.filter();
                }
            }
        }
        None
    }

    pub(super) fn render(&self, theme: &Theme, width: usize) -> Vec<Line> {
        match &self.entry {
            Entry::Name(input) => {
                let mut lines = menu_panel::search_field_lines(
                    theme,
                    width,
                    input.value(),
                    input.cursor(),
                    true,
                    "New segment name",
                );
                lines.push(hint_row(theme, width, "Enter: edit body · Esc: cancel"));
                lines
            }
            Entry::Body { name, editor, .. } => {
                let heading = format!("Editing {name}");
                let mut lines = vec![hint_row(theme, width, &heading)];
                let layout = editor.layout_text(width.saturating_sub(2).max(1));
                let cursor = layout.iter().position(|row| row.has_cursor).unwrap_or(0);
                let first = cursor
                    .saturating_sub(7)
                    .min(layout.len().saturating_sub(10));
                for line in layout.into_iter().skip(first).take(10) {
                    let mut spans = vec![Span::raw("  ")];
                    if line.has_cursor {
                        let before: String = line.text.chars().take(line.cursor_pos).collect();
                        let rest: String = line.text.chars().skip(line.cursor_pos).collect();
                        let cursor = rest.chars().next().unwrap_or(' ');
                        spans.extend([
                            Span::raw(before),
                            Span::styled(
                                cursor.to_string(),
                                theme
                                    .fg_style(crate::theme::ThemeColor::Accent)
                                    .add_modifier(ratatui::style::Modifier::REVERSED),
                            ),
                            Span::raw(rest.chars().skip(1).collect::<String>()),
                        ]);
                    } else {
                        spans.push(Span::raw(line.text));
                    }
                    lines.push(spans);
                }
                lines.push(hint_row(
                    theme,
                    width,
                    "Ctrl+S: save · Enter: new line · Esc: cancel",
                ));
                lines
            }
            Entry::List => {
                let mut lines = menu_panel::search_field_lines(
                    theme,
                    width,
                    self.search.value(),
                    self.search.cursor(),
                    true,
                    "Search prompt segment names",
                );
                let first = self
                    .selected
                    .saturating_sub(5)
                    .min(self.filtered.len().saturating_sub(10));
                for (position, &index) in self.filtered.iter().enumerate().skip(first).take(10) {
                    let row = &self.rows[index];
                    let label = match row.state {
                        PromptSegmentState::Default => "Default",
                        PromptSegmentState::Modified => "Modified",
                        PromptSegmentState::Removed => "Removed",
                        PromptSegmentState::Added => "Added",
                    };
                    lines.push(menu_panel::menu_row(
                        theme,
                        width,
                        vec![Span::raw(row.name.clone())],
                        &[MenuSegment::muted(label)],
                        position == self.selected,
                    ));
                }
                if self.filtered.is_empty() {
                    lines.push(menu_panel::no_match_row(
                        theme,
                        width,
                        "No matching prompt segments",
                    ));
                }
                lines.push(hint_row(
                    theme,
                    width,
                    "Enter: edit · Ctrl+N: add · Ctrl+D: remove · Ctrl+R: reset · Esc: back",
                ));
                lines
            }
        }
    }
}
