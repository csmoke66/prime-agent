//! The child-model settings draft. Filtering moves the cursor, never the checkmarks.

use super::{hint_row, SettingsMenuAction};
use crate::client_settings::{ChildModelChoices, ChildPolicy, InheritedChildModels};
use crate::keybindings::KeybindingsManager;
use crate::menu_panel::{self, MenuSegment};
use crate::search_input::SearchInput;
use crate::theme::Theme;
use crate::{Line, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Inherit,
    All,
    Selected,
}

impl From<&ChildPolicy> for Mode {
    fn from(policy: &ChildPolicy) -> Self {
        match policy.model_rules {
            None => Self::Inherit,
            Some(0) => Self::All,
            Some(_) => Self::Selected,
        }
    }
}

pub(super) fn policy_label(policy: &ChildPolicy) -> String {
    match policy.model_rules {
        None => match policy.inherited {
            InheritedChildModels::All => "Inherit (all allowed)".into(),
            InheritedChildModels::Patterns(count) => format!("Inherit ({count} global rules)"),
            InheritedChildModels::Unreadable => "Inherit (global settings unreadable)".into(),
        },
        Some(0) => "All allowed (session)".into(),
        Some(count) => format!("{count} model rules (session)"),
    }
}

#[derive(Debug)]
pub(super) struct ChildModelPicker {
    choices: ChildModelChoices,
    mode: Mode,
    checked: Vec<bool>,
    /// With a query, only matching model indices appear; no policy-mode rows.
    filtered: Option<Vec<usize>>,
    selected: usize,
    search: SearchInput,
    empty_selection: bool,
}

impl ChildModelPicker {
    pub(super) fn new(choices: ChildModelChoices) -> Self {
        let mode = Mode::from(&choices.policy);
        let selected = usize::from(mode == Mode::All);
        let checked = choices.models.iter().map(|model| model.allowed).collect();
        Self {
            choices,
            mode,
            checked,
            filtered: None,
            selected,
            search: SearchInput::new(),
            empty_selection: false,
        }
    }

    pub(super) fn into_policy(self) -> ChildPolicy {
        self.choices.policy
    }

    fn row_count(&self) -> usize {
        self.filtered
            .as_ref()
            .map_or(self.choices.models.len() + 2, Vec::len)
    }

    fn model_index(&self, position: usize) -> Option<usize> {
        match &self.filtered {
            Some(filtered) => filtered.get(position).copied(),
            None => position.checked_sub(2).filter(|index| *index < self.checked.len()),
        }
    }

    fn apply_filter(&mut self) {
        let query = self.search.value().trim();
        self.filtered = (!query.is_empty()).then(|| {
            let indices: Vec<_> = (0..self.choices.models.len()).collect();
            crate::fuzzy::fuzzy_filter(&indices, query, |&index| {
                let model = &self.choices.models[index];
                format!("{} {}", model.selector, model.name)
            })
        });
        self.selected = 0;
    }

    pub(super) fn paste(&mut self, text: &str) {
        self.search.paste(text);
        self.apply_filter();
    }

    pub(super) fn handle_key(&mut self, key: &str, kb: &KeybindingsManager) -> SettingsMenuAction {
        if kb.matches(key, "tui.select.cancel") || key == "ctrl+c" {
            return SettingsMenuAction::SubmenuClosed;
        }
        let count = self.row_count();
        if kb.matches(key, "tui.select.up") {
            if count > 0 {
                self.selected = (self.selected + count - 1) % count;
            }
        } else if kb.matches(key, "tui.select.down") {
            if count > 0 {
                self.selected = (self.selected + 1) % count;
            }
        } else if key == "space" {
            if let Some(index) = self.model_index(self.selected) {
                match self.mode {
                    Mode::Inherit => {
                        for (check, model) in self.checked.iter_mut().zip(&self.choices.models) {
                            *check = model.inherited;
                        }
                    }
                    Mode::All => self.checked.fill(true),
                    Mode::Selected => {}
                }
                self.mode = Mode::Selected;
                self.checked[index] = !self.checked[index];
                self.empty_selection = false;
            } else if self.filtered.is_none() {
                self.mode = if self.selected == 0 { Mode::Inherit } else { Mode::All };
                self.empty_selection = false;
            }
        } else if kb.matches(key, "tui.select.confirm") {
            // An unchanged selection leaves legacy patterns (and unavailable models) untouched.
            if self.mode == Mode::from(&self.choices.policy)
                && (self.mode != Mode::Selected
                    || self.checked.iter().copied().eq(self.choices.models.iter().map(|m| m.allowed)))
            {
                return SettingsMenuAction::SubmenuClosed;
            }
            return match self.mode {
                Mode::Inherit => SettingsMenuAction::SaveChildModels(None),
                Mode::All => SettingsMenuAction::SaveChildModels(Some(Vec::new())),
                Mode::Selected => {
                    let models: Vec<_> = self.choices.models.iter().zip(&self.checked)
                        .filter(|(_, checked)| **checked)
                        .map(|(model, _)| model.selector.clone())
                        .collect();
                    if models.is_empty() {
                        // The persisted empty list means unrestricted, never deny-all.
                        self.empty_selection = true;
                        SettingsMenuAction::None
                    } else {
                        SettingsMenuAction::SaveChildModels(Some(models))
                    }
                }
            };
        } else {
            let previous = self.search.value().to_owned();
            self.search.handle_key(key, kb);
            if self.search.value() != previous {
                self.apply_filter();
            }
        }
        SettingsMenuAction::None
    }

    pub(super) fn render(&self, theme: &Theme, width: usize, kb: &KeybindingsManager) -> Vec<Line> {
        let mut lines = menu_panel::search_field_lines(
            theme,
            width,
            self.search.value(),
            self.search.cursor(),
            true,
            "Search child models",
        );
        let status = match self.mode {
            Mode::Inherit => match self.choices.policy.inherited {
                InheritedChildModels::All => "Inheriting global settings: all models allowed",
                InheritedChildModels::Patterns(_) => "Inheriting global model patterns",
                InheritedChildModels::Unreadable => "Global model policy is unreadable; no models can be inherited",
            },
            Mode::All => "Allowing all child models in this session family",
            Mode::Selected => "Changes save an explicit list of checked models",
        };
        lines.push(hint_row(theme, width, status));
        let count = self.row_count();
        let start = self.selected.saturating_sub(5).min(count.saturating_sub(10));
        let end = (start + 10).min(count);
        for position in start..end {
            let (checked, label, name) = if let Some(index) = self.model_index(position) {
                let model = &self.choices.models[index];
                let checked = match self.mode {
                    Mode::Inherit => model.inherited,
                    Mode::All => true,
                    Mode::Selected => self.checked[index],
                };
                (checked, model.selector.as_str(), Some(model.name.as_str()))
            } else if position == 0 {
                (self.mode == Mode::Inherit, "Inherit global settings", None)
            } else {
                (self.mode == Mode::All, "Allow all models", None)
            };
            let marker = if checked { "✓ " } else { "  " };
            let name = name.map(MenuSegment::muted);
            lines.push(menu_panel::menu_row(
                theme,
                width,
                vec![Span::raw(format!("{marker}{label}"))],
                name.as_slice(),
                position == self.selected,
            ));
        }
        if self.choices.models.is_empty() {
            lines.push(menu_panel::no_match_row(theme, width, "No authenticated child models"));
        } else if count == 0 {
            lines.push(menu_panel::no_match_row(theme, width, "No matching child models"));
        } else if start > 0 || end < count {
            lines.push(menu_panel::scroll_row(theme, width, self.selected + 1, count));
        }
        if self.empty_selection {
            lines.push(hint_row(theme, width, "Check at least one model, or choose Inherit or Allow all"));
        }
        let mut hints = "Space toggle".to_owned();
        for hint in [
            menu_panel::key_hint(kb, &["tui.select.confirm"], "save"),
            menu_panel::key_hint(kb, &["tui.select.cancel"], "cancel"),
        ].into_iter().flatten() {
            hints.push_str(" · ");
            hints.push_str(&hint);
        }
        lines.push(hint_row(theme, width, &hints));
        lines
    }
}
