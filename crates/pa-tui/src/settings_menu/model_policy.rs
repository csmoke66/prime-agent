//! The global and child model settings drafts. Filtering never changes the checkmarks.

use super::{hint_row, SettingsMenuAction};
use crate::client_settings::{
    ChildModelChoice, ChildModelChoices, ChildPolicy, GlobalModelChoices, InheritedChildModels,
};
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
enum Scope {
    Child(ChildPolicy),
    Global { unrestricted: bool },
}

impl Scope {
    fn initial_mode(&self) -> Mode {
        match self {
            Self::Child(policy) => Mode::from(policy),
            Self::Global { unrestricted: true } => Mode::All,
            Self::Global {
                unrestricted: false,
            } => Mode::Selected,
        }
    }

    fn policy_rows(&self) -> usize {
        match self {
            Self::Child(_) => 2,
            Self::Global { .. } => 1,
        }
    }
}

#[derive(Debug)]
pub(super) struct ModelPolicyPicker {
    scope: Scope,
    models: Vec<ChildModelChoice>,
    mode: Mode,
    checked: Vec<bool>,
    /// With a query, only matching model indices appear; no policy-mode rows.
    filtered: Option<Vec<usize>>,
    selected: usize,
    search: SearchInput,
    empty_selection: bool,
}

impl ModelPolicyPicker {
    pub(super) fn child(choices: ChildModelChoices) -> Self {
        Self::new(Scope::Child(choices.policy), choices.models)
    }

    pub(super) fn global(choices: GlobalModelChoices) -> Self {
        Self::new(
            Scope::Global {
                unrestricted: choices.unrestricted,
            },
            choices.models,
        )
    }

    fn new(scope: Scope, models: Vec<ChildModelChoice>) -> Self {
        let mode = scope.initial_mode();
        let selected = if mode == Mode::All {
            scope.policy_rows() - 1
        } else {
            0
        };
        let checked = models.iter().map(|model| model.allowed).collect();
        Self {
            scope,
            models,
            mode,
            checked,
            filtered: None,
            selected,
            search: SearchInput::new(),
            empty_selection: false,
        }
    }

    pub(super) fn into_child_policy(self) -> Option<ChildPolicy> {
        match self.scope {
            Scope::Child(policy) => Some(policy),
            Scope::Global { .. } => None,
        }
    }

    fn row_count(&self) -> usize {
        self.filtered
            .as_ref()
            .map_or(self.models.len() + self.scope.policy_rows(), Vec::len)
    }

    fn model_index(&self, position: usize) -> Option<usize> {
        match &self.filtered {
            Some(filtered) => filtered.get(position).copied(),
            None => position
                .checked_sub(self.scope.policy_rows())
                .filter(|index| *index < self.checked.len()),
        }
    }

    fn apply_filter(&mut self) {
        let query = self.search.value().trim();
        self.filtered = (!query.is_empty()).then(|| {
            let indices: Vec<_> = (0..self.models.len()).collect();
            crate::fuzzy::fuzzy_filter(&indices, query, |&index| {
                let model = &self.models[index];
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
                        for (check, model) in self.checked.iter_mut().zip(&self.models) {
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
                self.mode = if self.selected == self.scope.policy_rows() - 1 {
                    Mode::All
                } else {
                    Mode::Inherit
                };
                self.empty_selection = false;
            }
        } else if kb.matches(key, "tui.select.confirm") {
            // An unchanged selection leaves legacy patterns (and unavailable models) untouched.
            if self.mode == self.scope.initial_mode()
                && (self.mode != Mode::Selected
                    || self
                        .checked
                        .iter()
                        .copied()
                        .eq(self.models.iter().map(|m| m.allowed)))
            {
                return SettingsMenuAction::SubmenuClosed;
            }
            return match self.mode {
                Mode::Inherit => SettingsMenuAction::SaveChildModels(None),
                Mode::All => self.save_selected(Vec::new()),
                Mode::Selected => {
                    let models: Vec<_> = self
                        .models
                        .iter()
                        .zip(&self.checked)
                        .filter(|(_, checked)| **checked)
                        .map(|(model, _)| model.selector.clone())
                        .collect();
                    if models.is_empty() {
                        // The persisted empty list means unrestricted, never deny-all.
                        self.empty_selection = true;
                        SettingsMenuAction::None
                    } else {
                        self.save_selected(models)
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

    fn save_selected(&self, models: Vec<String>) -> SettingsMenuAction {
        match self.scope {
            Scope::Child(_) => SettingsMenuAction::SaveChildModels(Some(models)),
            Scope::Global { .. } => SettingsMenuAction::SaveGlobalModels(models),
        }
    }

    pub(super) fn render(&self, theme: &Theme, width: usize, kb: &KeybindingsManager) -> Vec<Line> {
        let child = matches!(self.scope, Scope::Child(_));
        let mut lines = menu_panel::search_field_lines(
            theme,
            width,
            self.search.value(),
            self.search.cursor(),
            true,
            if child {
                "Search child models"
            } else {
                "Search global models"
            },
        );
        let status = match (&self.scope, self.mode) {
            (Scope::Child(policy), Mode::Inherit) => match policy.inherited {
                InheritedChildModels::All => "Inheriting global settings: all models allowed",
                InheritedChildModels::Patterns(_) => "Inheriting global model patterns",
                InheritedChildModels::Unreadable => {
                    "Global model policy is unreadable; no models can be inherited"
                }
            },
            (Scope::Child(_), Mode::All) => "Allowing all child models in this session family",
            (Scope::Child(_), Mode::Selected) => "Changes save an explicit list of checked models",
            (Scope::Global { .. }, Mode::All) => "Global model policy: all models allowed",
            (Scope::Global { .. }, Mode::Selected) => {
                "Global model policy: changes replace rules with checked models"
            }
            (Scope::Global { .. }, Mode::Inherit) => unreachable!("global policy cannot inherit"),
        };
        lines.push(hint_row(theme, width, status));
        let count = self.row_count();
        let start = self
            .selected
            .saturating_sub(5)
            .min(count.saturating_sub(10));
        let end = (start + 10).min(count);
        for position in start..end {
            let (checked, label, name) = if let Some(index) = self.model_index(position) {
                let model = &self.models[index];
                let checked = match self.mode {
                    Mode::Inherit => model.inherited,
                    Mode::All => true,
                    Mode::Selected => self.checked[index],
                };
                (checked, model.selector.as_str(), Some(model.name.as_str()))
            } else if child && position == 0 {
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
        if self.models.is_empty() {
            lines.push(menu_panel::no_match_row(
                theme,
                width,
                if child {
                    "No authenticated child models"
                } else {
                    "No models in catalog"
                },
            ));
        } else if count == 0 {
            lines.push(menu_panel::no_match_row(theme, width, "No matching models"));
        } else if start > 0 || end < count {
            lines.push(menu_panel::scroll_row(
                theme,
                width,
                self.selected + 1,
                count,
            ));
        }
        if self.empty_selection {
            lines.push(hint_row(
                theme,
                width,
                if child {
                    "Check at least one model, or choose Inherit or Allow all"
                } else {
                    "Check at least one model, or choose Allow all"
                },
            ));
        }
        let mut hints = "Space toggle".to_owned();
        for hint in [
            menu_panel::key_hint(kb, &["tui.select.confirm"], "save"),
            menu_panel::key_hint(kb, &["tui.select.cancel"], "cancel"),
        ]
        .into_iter()
        .flatten()
        {
            hints.push_str(" · ");
            hints.push_str(&hint);
        }
        lines.push(hint_row(theme, width, &hints));
        lines
    }
}
