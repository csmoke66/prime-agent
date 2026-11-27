//! The `/heartbeats` management view (redesigned per the operator's
//! 2026-09-23 directive): a columned table, Enter opens the detail
//! drill-in and the management actions in the `/mcp` control pattern.
//! The table fills the full TUI width (the operator's 2026-09-24 ruling).

use serde_json::Value;

use crate::keybindings::{format_key_text, KeybindingsManager};
use crate::menu_panel::{fill_row, hug_row, menu_list_layout, plain_cell, status_dot};
use crate::theme::{Theme, ThemeColor};
use crate::search_input::SearchInput;
use crate::width::{str_width, truncate_line, wrap_text};
use crate::{Line, Span};

mod data;

pub use data::{
    default_heartbeat_name, dock_heartbeat_items, format_timestamp, next_run_label,
    parse_heartbeat_job, parse_heartbeats, row_primary, scope_heartbeats, session_label,
    single_line, sort_heartbeats, source_label, HeartbeatEntry, HeartbeatJob,
};

mod schedule;

pub use schedule::human_schedule;

mod render;

use render::{
    action_row, detail_block_lines, detail_pairs, error_line, hint_line, pane_header_lines, Columns,
};

const PREFERRED_VISIBLE: usize = 8;

/// Rows the list reserves outside its items (rule, title, blank, column
/// header, blank, hint, blank; the scroll-indicator row rides
/// `menu_list_layout`'s reservation, never counted twice).
const LIST_FRAME_ROWS: usize = 7;

/// The detail pane's labeled-pair row budget: the seven base pairs all
/// fit — the schedule fact never displaces the error row.
const MAX_DETAIL_ROWS: usize = 7;

/// Editable heartbeat fields sent with the `edit` management action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatEdit {
    pub schedule: String,
    pub prompt: String,
    pub label: Option<String>,
    pub delivery_mode: String,
}

/// The management-action vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeartbeatAction {
    Pause,
    Resume,
    Stop,
    Edit(HeartbeatEdit),
}

impl HeartbeatAction {
    #[must_use]
    pub fn as_wire(&self) -> &'static str {
        match self {
            HeartbeatAction::Pause => "pause",
            HeartbeatAction::Resume => "resume",
            HeartbeatAction::Stop => "stop",
            HeartbeatAction::Edit(_) => "edit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EditForm {
    heartbeat_id: String,
    active_session_id: String,
    schedule: SearchInput,
    prompt: SearchInput,
    label: SearchInput,
    delivery_mode: String,
    focus: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    List,
    Detail {
        heartbeat_id: String,
        action_index: usize,
    },
    Edit(EditForm),
}

/// One key press while the view is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeartbeatsPickerAction {
    /// Esc / Ctrl+C / back on the list: close without acting.
    Close,
    /// Enter on an action row: the caller runs the management request.
    Manage {
        active_session_id: String,
        job_id: String,
        action: HeartbeatAction,
    },
    /// Navigation only.
    None,
}

/// The `/heartbeats` management view.
#[derive(Debug)]
pub struct HeartbeatsPicker {
    /// The scoped, sorted catalog.
    heartbeats: Vec<HeartbeatEntry>,
    /// The selected heartbeat's job id.
    selected_heartbeat_id: Option<String>,
    mode: Mode,
    /// The last catalog fetch failure.
    fetch_error: Option<String>,
    /// The last management-action error.
    error: Option<String>,
    viewport_rows: usize,
}

impl HeartbeatsPicker {
    /// Build the view over a fetched (scoped, sorted) catalog.
    #[must_use]
    pub fn new(
        heartbeats: Vec<HeartbeatEntry>,
        fetch_error: Option<String>,
        preselect: Option<String>,
        viewport_rows: usize,
    ) -> Self {
        let mut picker = HeartbeatsPicker {
            heartbeats,
            selected_heartbeat_id: None,
            mode: Mode::List,
            fetch_error,
            error: None,
            viewport_rows,
        };
        // A carried selection (the dock's chosen row) survives the
        // open; anything else lands on the first row (TS default).
        picker.selected_heartbeat_id = preselect
            .filter(|id| picker.heartbeats.iter().any(|entry| &entry.job.id == id))
            .or_else(|| picker.heartbeats.first().map(|entry| entry.job.id.clone()));
        picker
    }

    /// A landed catalog refresh: replace the rows, clear the fetch
    /// error, and keep the selection when the job survived.
    pub fn apply_catalog(&mut self, heartbeats: Vec<HeartbeatEntry>, fetch_error: Option<String>) {
        self.heartbeats = heartbeats;
        self.fetch_error = fetch_error;
        let selected = self.selected_heartbeat_id.clone();
        self.conform_selection(selected.as_deref());
        self.dirty_conform_mode();
    }

    /// A management result: a stop removes the row, anything else
    /// replaces the job in place; the pane returns to the list.
    pub fn apply_managed_job(&mut self, updated: HeartbeatJob, stopped: bool) {
        if stopped {
            self.heartbeats.retain(|entry| entry.job.id != updated.id);
        } else if let Some(entry) = self
            .heartbeats
            .iter_mut()
            .find(|entry| entry.job.id == updated.id)
        {
            entry.job = updated;
        }
        let selected = self.selected_heartbeat_id.clone();
        self.conform_selection(selected.as_deref());
        self.mode = Mode::List;
        self.error = None;
    }

    /// Surface a management-action failure without discarding an edit form.
    pub fn set_action_error(&mut self, error: String) {
        self.error = Some(error);
        if !matches!(self.mode, Mode::Edit(_)) {
            self.mode = Mode::List;
        }
    }

    pub fn set_fetch_error(&mut self, error: Option<String>) {
        self.fetch_error = error;
    }

    pub fn back_to_list(&mut self) {
        self.mode = Mode::List;
        self.error = None;
    }

    /// Reset the selection to the first row when the selected job vanished.
    fn conform_selection(&mut self, selected: Option<&str>) {
        let exists =
            selected.is_some_and(|id| self.heartbeats.iter().any(|entry| entry.job.id == id));
        if !exists {
            self.selected_heartbeat_id = self.heartbeats.first().map(|entry| entry.job.id.clone());
        }
    }

    /// Drop a detail or edit pane whose heartbeat vanished.
    fn dirty_conform_mode(&mut self) {
        let heartbeat_id = match &self.mode {
            Mode::Detail { heartbeat_id, .. } => Some(heartbeat_id),
            Mode::Edit(form) => Some(&form.heartbeat_id),
            Mode::List => None,
        };
        if heartbeat_id.is_some_and(|id| !self.heartbeats.iter().any(|entry| entry.job.id == id.as_str())) {
            self.mode = Mode::List;
        }
    }

    /// The selected row's index (first when unset).
    fn selected_index(&self) -> usize {
        self.heartbeats
            .iter()
            .position(|entry| Some(&entry.job.id) == self.selected_heartbeat_id.as_ref())
            .unwrap_or(0)
    }

    fn find_entry(&self, id: &str) -> Option<&HeartbeatEntry> {
        self.heartbeats.iter().find(|entry| entry.job.id == id)
    }

    /// Keep pause/resume and stop in their established positions, then edit.
    fn available_actions(
        entry: &HeartbeatEntry,
    ) -> [(&'static str, Option<HeartbeatAction>, &'static str); 3] {
        let toggle = if entry.job.is_active() {
            ("Pause heartbeat", Some(HeartbeatAction::Pause), "Stop deliveries until resumed")
        } else {
            ("Resume heartbeat", Some(HeartbeatAction::Resume), "Continue scheduled deliveries")
        };
        [
            toggle,
            ("Stop heartbeat", Some(HeartbeatAction::Stop), "Permanently remove this heartbeat"),
            ("Edit heartbeat", None, "Change schedule, prompt, label, or delivery"),
        ]
    }

    /// Paste into the focused edit control. Prompt pastes preserve line breaks.
    pub fn paste(&mut self, text: &str) {
        let Mode::Edit(form) = &mut self.mode else { return };
        let focus = form.focus;
        let input = match focus {
            0 => Some(&mut form.schedule),
            1 => Some(&mut form.prompt),
            2 => Some(&mut form.label),
            _ => None,
        };
        if let Some(input) = input {
            if focus == 1 {
                input.paste_multiline(text);
            } else {
                input.paste(text);
            }
            self.error = None;
        }
    }

    pub fn handle_key(&mut self, key: &str, kb: &KeybindingsManager) -> HeartbeatsPickerAction {
        if matches!(self.mode, Mode::Edit(_)) {
            return self.handle_edit_key(key, kb);
        }
        if key == "ctrl+c" || kb.matches(key, "tui.select.cancel") {
            return HeartbeatsPickerAction::Close;
        }
        if kb.matches(key, "app.modal.back") {
            if matches!(self.mode, Mode::List) {
                return HeartbeatsPickerAction::Close;
            }
            self.mode = Mode::List;
            self.error = None;
            return HeartbeatsPickerAction::None;
        }
        if kb.matches(key, "tui.select.up") || kb.matches(key, "tui.select.down") {
            self.move_selection(if kb.matches(key, "tui.select.up") { -1 } else { 1 });
            return HeartbeatsPickerAction::None;
        }
        if matches!(self.mode, Mode::List) && kb.matches(key, "app.modal.open") {
            self.open_detail();
            return HeartbeatsPickerAction::None;
        }
        if kb.matches(key, "tui.select.confirm") {
            return self.confirm_selection();
        }
        HeartbeatsPickerAction::None
    }

    fn handle_edit_key(&mut self, key: &str, kb: &KeybindingsManager) -> HeartbeatsPickerAction {
        let Mode::Edit(form) = &mut self.mode else { return HeartbeatsPickerAction::None };
        if key == "ctrl+c" {
            return HeartbeatsPickerAction::Close;
        }
        if kb.matches(key, "tui.select.cancel") {
            let heartbeat_id = form.heartbeat_id.clone();
            self.mode = Mode::Detail { heartbeat_id, action_index: 2 };
            self.error = None;
            return HeartbeatsPickerAction::None;
        }
        match key {
            "tab" | "down" | "enter" if form.focus < 4 => form.focus += 1,
            "shift+tab" | "up" => form.focus = (form.focus + 4) % 5,
            "left" | "right" | "space" if form.focus == 3 => {
                form.delivery_mode = if form.delivery_mode == "steer" { "follow_up" } else { "steer" }.into();
            }
            "shift+enter" if form.focus == 1 => {
                form.prompt.paste_multiline("\n");
            }
            "enter" | "space" if form.focus == 4 => {
                if form.schedule.value().trim().is_empty() || form.prompt.value().trim().is_empty() {
                    self.error = Some("Schedule and prompt are required".into());
                    return HeartbeatsPickerAction::None;
                }
                return HeartbeatsPickerAction::Manage {
                    active_session_id: form.active_session_id.clone(),
                    job_id: form.heartbeat_id.clone(),
                    action: HeartbeatAction::Edit(HeartbeatEdit {
                        schedule: form.schedule.value().to_string(),
                        prompt: form.prompt.value().to_string(),
                        label: (!form.label.value().trim().is_empty()).then(|| form.label.value().trim().to_string()),
                        delivery_mode: form.delivery_mode.clone(),
                    }),
                };
            }
            _ => match form.focus {
                0 => form.schedule.handle_key(key, kb),
                1 => form.prompt.handle_key(key, kb),
                2 => form.label.handle_key(key, kb),
                _ => {}
            },
        }
        self.error = None;
        HeartbeatsPickerAction::None
    }

    fn move_selection(&mut self, delta: isize) {
        match self.mode.clone() {
            Mode::List => {
                if self.heartbeats.is_empty() { return; }
                let index = self.selected_index() as isize;
                let next = (index + delta).clamp(0, self.heartbeats.len() as isize - 1) as usize;
                self.selected_heartbeat_id = Some(self.heartbeats[next].job.id.clone());
            }
            Mode::Detail { heartbeat_id, action_index } => {
                let Some(entry) = self.find_entry(&heartbeat_id) else { return };
                let count = Self::available_actions(entry).len();
                let next = (action_index as isize + delta).clamp(0, count as isize - 1) as usize;
                self.mode = Mode::Detail { heartbeat_id, action_index: next };
            }
            Mode::Edit(_) => {}
        }
    }

    fn confirm_selection(&mut self) -> HeartbeatsPickerAction {
        match self.mode.clone() {
            Mode::List => {
                self.open_detail();
                HeartbeatsPickerAction::None
            }
            Mode::Detail { heartbeat_id, action_index } => {
                let Some(entry) = self.find_entry(&heartbeat_id) else {
                    self.mode = Mode::List;
                    return HeartbeatsPickerAction::None;
                };
                let actions = Self::available_actions(entry);
                let Some((_, action, _)) = actions.into_iter().nth(action_index) else {
                    return HeartbeatsPickerAction::None;
                };
                let Some(action) = action else {
                    self.open_edit(heartbeat_id);
                    return HeartbeatsPickerAction::None;
                };
                HeartbeatsPickerAction::Manage {
                    active_session_id: entry.job.active_session_id.clone(),
                    job_id: entry.job.id.clone(),
                    action,
                }
            }
            Mode::Edit(_) => HeartbeatsPickerAction::None,
        }
    }

    fn open_detail(&mut self) {
        let Some(id) = self.selected_heartbeat_id.clone() else { return };
        if self.find_entry(&id).is_some() {
            self.mode = Mode::Detail { heartbeat_id: id, action_index: 0 };
        }
    }

    fn open_edit(&mut self, heartbeat_id: String) {
        let Some(entry) = self.find_entry(&heartbeat_id) else { return };
        let active_session_id = entry.job.active_session_id.clone();
        let schedule_value = entry.job.schedule_expression.clone();
        let prompt_value = entry.job.prompt.clone();
        let label_value = entry.job.label.clone().unwrap_or_default();
        let delivery_mode = entry.job.delivery_mode.clone().unwrap_or_else(|| "steer".into());
        let mut schedule = SearchInput::new();
        schedule.prefill(&schedule_value);
        let mut prompt = SearchInput::new();
        prompt.prefill(&prompt_value);
        let mut label = SearchInput::new();
        label.prefill(&label_value);
        self.mode = Mode::Edit(EditForm {
            heartbeat_id, active_session_id, schedule, prompt, label, delivery_mode, focus: 0,
        });
        self.error = None;
    }

    /// The list's visible-row budget (inline shape).
    fn visible_items(&self) -> usize {
        // Both error blocks render two rows each when present (the
        // fetch failure and the action failure stack in the footer).
        let reserved = LIST_FRAME_ROWS
            + match (self.error.is_some(), self.fetch_error.is_some()) {
                (true, true) => 4,
                (some, _) if some => 2,
                (_, true) => 2,
                _ => 0,
            };
        // The shared layout floors at one row; this view must never render past
        // its viewport, so a frame too short for any row renders none.
        if self.viewport_rows <= reserved {
            return 0;
        }
        menu_list_layout(
            Some(self.viewport_rows),
            PREFERRED_VISIBLE,
            self.heartbeats.len(),
            reserved,
            1,
        )
    }

    #[must_use]
    pub fn render(&self, theme: &Theme, width: usize, kb: &KeybindingsManager) -> Vec<Line> {
        match &self.mode {
            Mode::List => self.render_list(theme, width, kb),
            Mode::Detail { heartbeat_id, action_index } => {
                self.render_detail(theme, width, kb, heartbeat_id, *action_index)
            }
            Mode::Edit(form) => self.render_edit(theme, width, form),
        }
    }

    /// The list pane (the `/model` picker idiom over a columned table).
    fn render_list(&self, theme: &Theme, width: usize, kb: &KeybindingsManager) -> Vec<Line> {
        let counts: Vec<(ThemeColor, String)> = self.status_counts();
        let mut lines = pane_header_lines(theme, width, "Heartbeats", &counts, None);
        if self.heartbeats.is_empty() {
            lines.push(vec![
                Span::raw("  "),
                theme.fg_span(ThemeColor::Muted, "No running or paused heartbeats"),
            ]);
        } else {
            let columns = Columns::new(width, &self.heartbeats);
            lines.push(columns.header_row(theme, width));
            let selected = self.selected_index();
            let visible = self.visible_items();
            let start = selected
                .saturating_sub(visible / 2)
                .min(self.heartbeats.len().saturating_sub(visible));
            let end = (start + visible).min(self.heartbeats.len());
            let now = crate::agents_view_state::now_ms();
            for (index, entry) in self.heartbeats[start..end].iter().enumerate() {
                let is_selected = start + index == selected;
                lines.push(columns.entry_row(theme, width, entry, is_selected, now));
            }
            if visible > 0 && (start > 0 || end < self.heartbeats.len()) {
                lines.push(vec![
                    Span::raw("  "),
                    theme.fg_span(
                        ThemeColor::Muted,
                        format!("({}/{})", selected + 1, self.heartbeats.len()),
                    ),
                ]);
            }
        }
        lines.extend(self.pane_footer(theme, width, &Self::list_hint(kb)));
        // The budget math keeps every normal viewport exact; a terminal shorter
        // than the frame itself degrades by truncation — the pane never renders
        // past its allocated rows.
        lines.truncate(self.viewport_rows.max(1));
        lines
    }

    /// The active/paused counts for the title line's right-aligned cluster, in the status colors.
    fn status_counts(&self) -> Vec<(ThemeColor, String)> {
        let active = self
            .heartbeats
            .iter()
            .filter(|entry| entry.job.is_active())
            .count();
        let paused = self.heartbeats.len() - active;
        let mut counts = Vec::new();
        if active > 0 {
            counts.push((ThemeColor::Success, format!("{active} active")));
        }
        if paused > 0 {
            counts.push((ThemeColor::Warning, format!("{paused} paused")));
        }
        counts
    }

    /// The detail drill-in: the full prompt text (wrapped, never
    /// single-lined), the created-by facts, and the action rows.
    fn render_detail(
        &self,
        theme: &Theme,
        width: usize,
        kb: &KeybindingsManager,
        heartbeat_id: &str,
        action_index: usize,
    ) -> Vec<Line> {
        let Some(entry) = self.find_entry(heartbeat_id) else {
            // The heartbeat vanished: the TS panel degrades to this text.
            let mut lines = pane_header_lines(theme, width, "Heartbeats", &[], None);
            lines.push(vec![
                Span::raw("  "),
                theme.fg_span(ThemeColor::Muted, "This heartbeat is no longer available."),
            ]);
            lines.extend(self.pane_footer(theme, width, &Self::detail_hint(kb)));
            return lines;
        };
        let name = entry
            .job
            .label
            .as_deref()
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map_or_else(|| default_heartbeat_name(entry).to_string(), str::to_string);
        let subtitle = format!(
            "{} \u{b7} {}",
            human_schedule(&entry.job.schedule_expression),
            entry.job.status
        );
        let mut lines = pane_header_lines(theme, width, &name, &[], Some(&subtitle));
        let actions = Self::available_actions(entry);
        let pairs = detail_pairs(entry, crate::agents_view_state::now_ms());
        // The pane's fixed rows: the header block (rule, title, subtitle,
        // blank), the blank before the actions, the action rows, the
        // footer (blank, hint, blank), and the error rows when present.
        let error_rows = match (self.fetch_error.is_some(), self.error.is_some()) {
            (true, true) => 4,
            (some, _) if some => 2,
            (_, true) => 2,
            _ => 0,
        };
        let fixed = 4 + 1 + actions.len() + 3 + error_rows;
        // The created-by pairs shrink first (they summarize), then the prompt
        // clips its tail — the action rows never yield.
        let prompt_width = width.saturating_sub(4).max(10);
        // Non-newline control characters scrub before the wrap (an escape
        // sequence in a prompt can never execute terminal control operations
        // when rendered).
        let prompt = entry
            .job
            .prompt
            .chars()
            .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
            .collect::<String>();
        let wrapped = wrap_text(&prompt, prompt_width);
        let mut pairs_rows = pairs.len().min(MAX_DETAIL_ROWS);
        let mut prompt_budget = self
            .viewport_rows
            .saturating_sub(fixed + 1 + pairs_rows + 2);
        // When the default math starves the prompt, the pairs shrink first
        // until at least one prompt row renders.
        let has_prompt = !entry.job.prompt.trim().is_empty();
        if prompt_budget == 0 && pairs_rows > 0 && has_prompt {
            pairs_rows = pairs_rows.min(self.viewport_rows.saturating_sub(fixed + 4));
            prompt_budget = self
                .viewport_rows
                .saturating_sub(fixed + 1 + pairs_rows + 2);
        }
        if prompt_budget > 0 && !entry.job.prompt.trim().is_empty() {
            lines.push(Vec::new());
            lines.push(vec![
                Span::raw("  "),
                theme.fg_span(ThemeColor::Dim, "Prompt".to_string()),
            ]);
            let mut shown = wrapped.len().min(prompt_budget);
            let mut clipped = false;
            if wrapped.len() > shown && shown > 1 {
                shown -= 1;
                clipped = true;
            }
            for line in &wrapped[..shown] {
                let mut row = vec![Span::raw("  ")];
                row.extend(line.iter().cloned());
                lines.push(truncate_line(&row, width, ""));
            }
            if clipped {
                lines.push(vec![
                    Span::raw("  "),
                    theme.fg_span(ThemeColor::Dim, "\u{2026}".to_string()),
                ]);
            }
        }
        if pairs_rows > 0 {
            lines.push(Vec::new());
            lines.extend(detail_block_lines(
                theme,
                width,
                &pairs.into_iter().take(pairs_rows).collect::<Vec<_>>(),
            ));
        }
        lines.push(Vec::new());
        for (index, (label, _, description)) in actions.iter().enumerate() {
            lines.push(action_row(
                theme,
                width,
                label,
                description,
                index == action_index,
            ));
        }
        lines.extend(self.pane_footer(theme, width, &Self::detail_hint(kb)));
        lines.truncate(self.viewport_rows.max(1));
        lines
    }

    fn render_edit(&self, theme: &Theme, width: usize, form: &EditForm) -> Vec<Line> {
        let mut lines = pane_header_lines(theme, width, "Edit heartbeat", &[], None);
        for (index, (label, input)) in [
            ("Schedule", &form.schedule),
            ("Prompt", &form.prompt),
            ("Label (optional)", &form.label),
        ].into_iter().enumerate() {
            let focused = form.focus == index;
            let cursor = if focused { input.cursor() } else { 0 };
            let value = input.value();
            let at = value.char_indices().nth(cursor).map_or(value.len(), |(at, _)| at);
            let start = value[..at].rfind('\n').map_or(0, |at| at + 1);
            let line = value[start..].split('\n').next().unwrap_or_default();
            let line_cursor = value[start..at].chars().count();
            lines.push(vec![
                Span::raw(if focused { " › " } else { "   " }),
                theme.fg_span(ThemeColor::Dim, format!("{label}:")),
            ]);
            lines.push(crate::menu_panel::search_field_plain_row(
                theme, width, line, line_cursor, focused, " ",
            ));
        }
        lines.push(action_row(theme, width, "Delivery", &form.delivery_mode, form.focus == 3));
        lines.push(Vec::new());
        lines.push(action_row(
            theme, width, "Save changes", "Apply this heartbeat edit", form.focus == 4,
        ));
        let footer = self.pane_footer(
            theme, width,
            "Tab/↑/↓ move · Space toggle/save · Shift+Enter newline · Esc cancel",
        );
        lines.truncate(self.viewport_rows.saturating_sub(footer.len()));
        lines.extend(footer);
        lines.truncate(self.viewport_rows.max(1));
        lines
    }

    /// The list's bottom hint line: every shortcut in one line. An override
    /// that empties one of the open/close key pairs drops that key — the
    /// hint never advertises a key the handler does not take.
    fn list_hint(kb: &KeybindingsManager) -> String {
        let key = |binding: &str, fallback: &str| {
            kb.first_key(binding)
                .map_or_else(|| fallback.to_string(), |key| format_key_text(&key))
        };
        let bound = |binding: &str| kb.first_key(binding).map(|key| format_key_text(&key));
        let open = match bound("app.modal.open") {
            Some(detail) => format!("{}/{}", key("tui.select.confirm", "Enter"), detail),
            None => key("tui.select.confirm", "Enter"),
        };
        let close = match bound("app.modal.back") {
            Some(back) => format!("{}/{}", back, key("tui.select.cancel", "Esc")),
            None => key("tui.select.cancel", "Esc"),
        };
        format!(
            "{}/{} move \u{b7} {open} open \u{b7} {close} close",
            key("tui.select.up", "\u{2191}"),
            key("tui.select.down", "\u{2193}"),
        )
    }

    /// The detail pane's bottom hint line.
    fn detail_hint(kb: &KeybindingsManager) -> String {
        let key = |binding: &str, fallback: &str| {
            kb.first_key(binding)
                .map_or_else(|| fallback.to_string(), |key| format_key_text(&key))
        };
        format!(
            "{}/{} move \u{b7} {} run \u{b7} {} back \u{b7} {} close",
            key("tui.select.up", "\u{2191}"),
            key("tui.select.down", "\u{2193}"),
            key("tui.select.confirm", "Enter"),
            key("app.modal.back", "\u{2190}"),
            key("tui.select.cancel", "Esc"),
        )
    }

    /// The pane footer: the fetch and action errors, a blank, the hint line,
    /// and one blank below (no rule under the hint — the /model geometry, per
    /// the operator's 2026-09-24 ruling).
    fn pane_footer(&self, theme: &Theme, width: usize, hint: &str) -> Vec<Line> {
        let mut lines = Vec::new();
        if let Some(fetch_error) = &self.fetch_error {
            lines.push(Vec::new());
            let line = vec![
                Span::raw("  "),
                theme.fg_span(
                    ThemeColor::Warning,
                    format!("Heartbeat refresh failed: {}", single_line(fetch_error)),
                ),
            ];
            lines.push(truncate_line(&line, width, ""));
        }
        if let Some(error) = &self.error {
            lines.push(Vec::new());
            lines.push(error_line(theme, width, error));
        }
        lines.push(Vec::new());
        lines.push(hint_line(theme, width, hint));
        lines.push(Vec::new());
        lines
    }
}

#[cfg(test)]
mod tests;
