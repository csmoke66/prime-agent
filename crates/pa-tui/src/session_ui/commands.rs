//! The slash-command ladder (`handle_slash`), the builtin client-command dispatch tail, the
//! command-catalog refresh/fold, and the shared connection-state read.
use super::{
    create_session, info_commands, terminal_columns, AgentView, AuthSelectorKind,
    CommandCatalogUpdate, DaemonCommand, DockFold, Duration, InfoContent, Map,
    PendingConfirm, RebuildKind, Result, SessionUi, SlashCommandExecution, SlashCommandRegistry,
    StatusKind, SubmitBehavior, Value, UI_REQUEST_TIMEOUT_MS,
};

fn workflow_target(args: &str) -> std::result::Result<(Option<String>, bool, bool, &str), String> {
    let mut rest = args.trim();
    let mut target = None;
    let mut interrupt = false;
    let mut continue_turn = false;
    loop {
        if let Some(after) = rest.strip_prefix("--interrupt") {
            if after.is_empty() || after.starts_with(char::is_whitespace) {
                interrupt = true;
                rest = after.trim_start();
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix("--continue") {
            if after.is_empty() || after.starts_with(char::is_whitespace) {
                continue_turn = true;
                rest = after.trim_start();
                continue;
            }
        }
        let after = rest
            .strip_prefix("--session=")
            .or_else(|| rest.strip_prefix("--session "));
        if let Some(after) = after {
            let after = after.trim_start();
            if after.is_empty() {
                return Err("--session requires a session selector".to_string());
            }
            let (selector, remaining) = after
                .split_once(char::is_whitespace)
                .map_or((after, ""), |(selector, remaining)| (selector, remaining.trim_start()));
            target = Some(selector.to_owned());
            rest = remaining;
            continue;
        }
        if rest == "--session" {
            return Err("--session requires a session selector".to_string());
        }
        break;
    }
    Ok((target, interrupt, continue_turn, rest))
}

#[cfg(test)]
mod workflow_target_tests {
    use super::workflow_target;

    #[test]
    fn parses_targeted_and_local_workflow_arguments() {
        assert_eq!(
            workflow_target("--interrupt --session target-id --continue review {\"scope\":\"local\"}").unwrap(),
            (
                Some("target-id".to_string()),
                true,
                true,
                "review {\"scope\":\"local\"}"
            )
        );
        assert_eq!(
            workflow_target("review {\"scope\":\"local\"}").unwrap(),
            (None, false, false, "review {\"scope\":\"local\"}")
        );
        assert!(workflow_target("--session ").is_err());
    }
}

impl SessionUi {
    pub(super) async fn submit_workflow(
        &mut self,
        name: String,
        input: Value,
        target_session: Option<String>,
        interrupt: bool,
        continue_turn: bool,
        view: &mut AgentView,
    ) -> Result<()> {
        let active_session_id = target_session
            .clone()
            .unwrap_or_else(|| self.active_session_id.clone());
        let command = DaemonCommand::RunWorkflow {
            id: None,
            active_session_id,
            name: name.clone(),
            input,
            interrupt,
            continue_turn,
        };
        match self.bounded_request(Duration::from_millis(UI_REQUEST_TIMEOUT_MS), command).await {
            Ok(receipt) => {
                let suffix = target_session
                    .as_deref()
                    .map_or_else(String::new, |target| format!(" on {target}"));
                if receipt.get("deduplicated").and_then(Value::as_bool) == Some(true) {
                    self.note(&format!("Workflow {name} is already queued or running{suffix}."), view)
                } else {
                    self.note(&format!("Workflow {name} started{suffix}."), view)
                }
            }
            Err(e) => self.error_row(&format!("Cannot start workflow: {e:#}"), view),
        }
        Ok(())
    }

    /// Slash-command dispatch: builtin client commands without a UI report unavailability, session
    /// commands forward to the session, unknown commands get the suggestion error or pass through
    /// as a prompt; `behavior` passes through, so a slash-prefixed follow-up keeps its lane.
    pub(super) async fn handle_slash(
        &mut self,
        text: &str,
        behavior: SubmitBehavior,
        view: &mut AgentView,
    ) -> Result<()> {
        let registry = SlashCommandRegistry::builtin();
        let (name, args) = pa_types::slash_commands::parse_slash_command(text)
            .unwrap_or_else(|| (String::new(), String::new()));

        // A bare `/skill:<name>` never sends: the daemon's admission seam would hand the model the
        // expanded protocol as its only user message with no task attached.
        if name.starts_with("skill:") && args.trim().is_empty() {
            // The draft restores into the argument position: the next keystrokes
            // become the request.
            view.editor.set_text(&format!("{text} "));
            self.note(
                "add your request after the skill, e.g. /skill:prime-agent-release make a release of PR #2731",
                view,
            );
            return Ok(());
        }

        // Client-local commands this build implements (not TS builtins).
        match name.as_str() {
            "help" => {
                self.note(
                    "/help           this list\n/list           live sessions\n/switch <n|id>  switch to a session from /list\n/new            start a new session\n/harness        browse local and global harness entries\n/run [--session ID] [--interrupt] [--continue] [name]  run a workflow\n/settings       configure models, sub-agents, and preferences\n/exit           detach and exit",
                    view,
                );
                return Ok(());
            }
            "list" => {
                self.refresh_list(view).await?;
                return Ok(());
            }
            "switch" => {
                if args.is_empty() {
                    self.note("usage: /switch <n|id> (run /list first)", view);
                } else {
                    self.switch_to(&args, view).await?;
                }
                return Ok(());
            }
            "exit" => {
                self.exit_requested = true;
                return Ok(());
            }
            name if pa_types::slash_commands::moved_to_settings(name) => {
                self.note("This option moved to /settings.", view);
                return Ok(());
            }
            _ => {}
        }

        let Some(resolved) = registry.parse(text) else {
            // Oversized names are prompts; close typos get the suggestion error,
            // everything else passes through to the model.
            if name.chars().count() > 64 {
                return self.send_prompt(text, behavior, view);
            }
            let candidates = registry.suggestion_candidates();
            return match pa_types::slash_commands::find_slash_command_suggestion(&name, &candidates)
            {
                Some(suggestion) => {
                    self.note(
                        &format!("Unknown command: /{name}. Did you mean /{suggestion}?"),
                        view,
                    );
                    Ok(())
                }
                None => self.send_prompt(text, behavior, view),
            };
        };

        let command = registry
            .get(resolved.name)
            .expect("resolved name is builtin");
        // `agent command used` (TS `captureAgentCommandUsed`): one report
        // per submitted builtin, client and session commands alike, by its
        // canonical name, before the command runs.
        self.track_command_used(resolved.name);
        match command.execution {
            SlashCommandExecution::Session => self.send_prompt(text, behavior, view),
            SlashCommandExecution::Client => {
                self.dispatch_client_command(&resolved, text, view).await
            }
        }
    }

    /// Create a fresh session and rebind the view to it (the `/new` flow,
    /// shared with the `app.session.new` action).
    pub(super) async fn start_new_session(&mut self, view: &mut AgentView) -> Result<()> {
        let id = create_session(&self.client, &self.create_options(), None).await?;
        self.attach_session(&id, DockFold::Fresh).await?;
        // The tray's context usage rode the attach snapshot: the
        // rebuild copies it into the chrome without a stats round-trip.
        self.rebuild_view(view, &RebuildKind::Rebind);
        // A new session starts with no draft and no prompt history (the submitted
        // `/new` drains the draft already).
        view.editor.clear_history();
        view.editor.set_text("");
        self.note(&format!("started session {id}"), view);
        self.track_feature_outcome("new", "completed", None);
        Ok(())
    }

    /// A builtin client command: only the implemented subset runs locally;
    /// the rest report unavailability.
    async fn dispatch_client_command(
        &mut self,
        resolved: &pa_types::slash_commands::ResolvedSlashCommand,
        text: &str,
        view: &mut AgentView,
    ) -> Result<()> {
        match resolved.name {
            // `/clear` stays the no-argument alias of `/new`.
            "new" if resolved.original_name == "clear" && !resolved.args.is_empty() => {
                self.note("Usage: /clear", view);
            }
            "new" => {
                // The raw suffix after the typed command name (TS parses
                // `text.slice(4)`): the leading separator and the verbatim
                // prompt survive, where the trimmed `resolved.args` would
                // lose them.
                let options = match crate::new_session_command::parse_new_session_command(
                    &text[1 + resolved.original_name.len()..],
                ) {
                    Ok(options) => options,
                    Err(message) => {
                        // The draft the submit drained restores next to the
                        // error row (TS `editor.setText(text)` + `showError`).
                        view.editor.set_text(text);
                        self.error_row(&message, view);
                        return Ok(());
                    }
                };
                self.start_new_session(view).await?;
                if let Some(name) = options.name {
                    if !self.set_session_name(&name, view).await {
                        // TS parity (`handleClearCommand`'s catch): the
                        // rename failed after the session exists — the
                        // prompt restores into the editor and never submits.
                        if let Some(prompt) = options.prompt.as_deref() {
                            view.editor.set_text(prompt);
                        }
                        return Ok(());
                    }
                }
                if let Some(prompt) = options.prompt {
                    view.editor.add_to_history(&prompt);
                    self.send_prompt(&prompt, SubmitBehavior::Steer, view)?;
                }
            }
            // `/quit` detaches and exits (the session keeps running in the daemon).
            "quit" => {
                self.exit_requested = true;
            }
            "resume" => {
                if resolved.args.is_empty() {
                    self.open_agents_view = true;
                    self.exit_requested = true;
                } else {
                    match self.resolve_resume_selector(&resolved.args) {
                        Some(selection) => {
                            self.pending_selection = Some(selection);
                            self.exit_requested = true;
                        }
                        None => {
                            self.note(
                                &format!("could not resolve session \"{}\"", resolved.args),
                                view,
                            );
                        }
                    }
                }
            }
            "tree" => {
                if resolved.args.is_empty() {
                    self.track_feature_outcome("tree", "initiated", None);
                    self.open_tree_selector(view, None).await?;
                } else {
                    self.note("Usage: /tree", view);
                }
            }
            "fork" => {
                if resolved.args.is_empty() {
                    self.track_feature_outcome("fork", "initiated", None);
                    self.open_fork_selector(view).await?;
                } else {
                    self.note("Usage: /fork", view);
                }
            }
            "clone" => {
                if resolved.args.is_empty() {
                    self.track_feature_outcome("clone", "initiated", None);
                    self.handle_clone_command(view).await?;
                } else {
                    self.note("Usage: /clone", view);
                }
            }
            "copy" => {
                if resolved.args.is_empty() {
                    self.handle_copy_command(view).await?;
                } else {
                    view.editor
                        .set_text(&format!("/{} {}", resolved.original_name, resolved.args));
                    self.error_row("Usage: /copy", view);
                }
            }
            // `/login` opens the providers selector (the full configuration menu
            // stays unported).
            "login" => {
                if resolved.args.is_empty() {
                    self.open_provider_auth(AuthSelectorKind::Login, view)
                        .await?;
                    self.track_feature_outcome("login", "initiated", None);
                } else {
                    view.editor
                        .set_text(&format!("/{} {}", resolved.original_name, resolved.args));
                    self.error_row("Usage: /login", view);
                }
            }
            "logout" => {
                if resolved.args.is_empty() {
                    self.open_provider_auth(AuthSelectorKind::Logout, view)
                        .await?;
                    self.track_feature_outcome("logout", "initiated", None);
                } else {
                    view.editor
                        .set_text(&format!("/{} {}", resolved.original_name, resolved.args));
                    self.error_row("Usage: /logout", view);
                }
            }
            "import" => {
                let command_text = if resolved.args.is_empty() {
                    "/import".to_string()
                } else {
                    format!("/import {}", resolved.args)
                };
                self.open_import_confirm(&command_text, view);
            }
            "traces" => {
                self.handle_traces_command(resolved, view).await?;
            }
            // `/update` (the TS->Rust migration path): the confirm, then
            // the download+install runs OUT-OF-BAND (a background task —
            // the TUI stays mounted, the daemon keeps running, and the
            // update replaces only the on-disk binary, so the new build
            // takes effect on restart and nothing here blocks or tears
            // down; there is no busy guard to keep). The confirm carries
            // the preserve invariant: sessions and configuration
            // (~/.prime/agent) are never touched.
            "update" => {
                if !resolved.args.trim().is_empty() {
                    view.editor.set_text(text);
                    self.error_row("Usage: /update", view);
                    return Ok(());
                }
                if self.update_in_flight {
                    self.note_as(
                        "An update is already running; its outcome lands here when it finishes.",
                        StatusKind::Warning,
                        view,
                    );
                    return Ok(());
                }
                view.editor.set_text("");
                // The message is hand-wrapped to fit the panel rows (the
                // confirm renders each line truncated, never wrapped).
                view.confirm = Some(crate::confirm::ConfirmPanel::yes_no(
                    "Update Prime Agent",
                    "Update installs the latest Prime Agent build;\nyour sessions and configuration (~/.prime/agent) are never touched.\n\nThe update runs in the background — restart prime-agent after it\nfinishes to run the new build.",
                ));
                self.pending_confirm = Some(PendingConfirm::Update);
                self.dirty = true;
            }
            // The auth flows run in the client process; other management subcommands
            // surface through the `mcp` CLI command instead of the TUI.
            "mcp" => self.handle_mcp_command(resolved, view).await?,
            "run" => {
                let parsed = workflow_target(resolved.args.trim());
                let (target_session, interrupt, continue_turn, args) = match parsed {
                    Ok(parsed) => parsed,
                    Err(error) => {
                        view.editor.set_text(text);
                        self.error_row(&error, view);
                        return Ok(());
                    }
                };
                if let Some((name, raw)) = args.split_once(char::is_whitespace) {
                    match serde_json::from_str::<Map<String, Value>>(raw.trim()) {
                        Ok(input) => {
                            self.submit_workflow(
                                name.to_owned(),
                                Value::Object(input),
                                target_session,
                                interrupt,
                                continue_turn,
                                view,
                            )
                            .await?;
                        }
                        Err(error) => {
                            view.editor.set_text(text);
                            self.error_row(&format!("Invalid workflow input object: {error}"), view);
                        }
                    }
                } else {
                    let command = DaemonCommand::ListWorkflows {
                        id: None,
                        active_session_id: target_session
                            .clone()
                            .unwrap_or_else(|| self.active_session_id.clone()),
                    };
                    let form = match self
                        .bounded_request(Duration::from_millis(UI_REQUEST_TIMEOUT_MS), command)
                        .await
                    {
                        Ok(result) => crate::workflow_form::WorkflowForm::open_for_invocation(
                            args,
                            result,
                            target_session,
                            interrupt,
                            continue_turn,
                        ),
                        Err(error) => Err(format!("Cannot list workflows: {error:#}")),
                    };
                    match form {
                        Ok(form) => {
                            view.workflow_form = Some(form);
                            self.dirty = true;
                        }
                        Err(error) => {
                            view.editor.set_text(text);
                            self.error_row(&error, view);
                        }
                    }
                }
            }
            "harness" => {
                if !resolved.args.is_empty() {
                    view.editor.set_text(text);
                    self.error_row("Usage: /harness", view);
                    return Ok(());
                }
                self.open_harness_browser(view);
            }
            // `/plugins [search]` opens the `/mcp` view (this client folds the
            // catalog into it); an argument prefills its search field.
            "plugins" => {
                self.open_mcp_view("/plugins", view, "").await?;
                let search = resolved.args.trim();
                if !search.is_empty() {
                    if let Some(mcp) = view.mcp_view.as_mut() {
                        mcp.paste(search);
                    }
                }
            }
            // An explicit `.jsonl` path exports the current branch; anything else
            // exports HTML.
            "export" => {
                self.handle_export_command(resolved, view).await?;
            }
            // The session exports to a temp file and uploads as a secret gist.
            "share" => {
                if resolved.args.is_empty() {
                    self.handle_share_command(view).await?;
                } else {
                    view.editor
                        .set_text(&format!("/{} {}", resolved.original_name, resolved.args));
                    self.error_row("Usage: /share", view);
                }
            }
            // `/hotkeys`: the full keyboard-shortcut reference renders from the EFFECTIVE bindings
            // so user `keybindings.json` overrides show their keys (client-side rows only).
            "hotkeys" => {
                if !resolved.args.is_empty() {
                    view.editor
                        .set_text(&format!("/{} {}", resolved.original_name, resolved.args));
                    self.error_row("Usage: /hotkeys", view);
                    return Ok(());
                }
                // The operator's 2026-09-26 directive: the guide renders as the read-only
                // info panel instead of the transcript flood.
                self.open_info_panel(
                    view,
                    Some("Hotkeys".to_string()),
                    InfoContent::Markdown(crate::hotkeys::hotkeys_guide(view.editor.keybindings())),
                );
                self.track_menu_opened("hotkeys", "command");
            }

            "session" => {
                if !resolved.args.is_empty() {
                    view.editor.set_text(text);
                    self.error_row("Usage: /session", view);
                    return Ok(());
                }
                let stats = self
                    .bounded_request(
                        Duration::from_millis(UI_REQUEST_TIMEOUT_MS),
                        DaemonCommand::GetSessionStats {
                            id: None,
                            active_session_id: self.active_session_id.clone(),
                            rest: Map::default(),
                        },
                    )
                    .await;
                match stats {
                    Ok(stats) => {
                        let name = self.session_name.clone();
                        self.open_info_panel(
                            view,
                            None,
                            InfoContent::Rows(info_commands::session_info_rows(
                                &stats,
                                name.as_deref(),
                            )),
                        );
                        self.track_menu_opened("session", "command");
                    }
                    Err(error) => {
                        self.error_row(&format!("{error:#}"), view);
                    }
                }
            }
            // `/context` (and its `/usage` alias): the agent tree with token/cost columns and
            // context utilization. The optional `all` argument is a deliberate TS delta (TS takes
            // none): past the row budget the default view collapses to the highest-usage agents;
            // `all` renders the whole tree.
            "context" => {
                let scope = match resolved.args.as_str() {
                    "" => info_commands::ContextTreeScope::Collapsed,
                    "all" => info_commands::ContextTreeScope::EveryAgent,
                    _ => {
                        view.editor.set_text(text);
                        self.error_row("Usage: /context [all]", view);
                        return Ok(());
                    }
                };
                let tree = self
                    .bounded_request(
                        Duration::from_millis(UI_REQUEST_TIMEOUT_MS),
                        DaemonCommand::GetContextTree {
                            id: None,
                            active_session_id: self.active_session_id.clone(),
                            rest: Map::default(),
                        },
                    )
                    .await;
                match tree {
                    Ok(tree) => {
                        // TS render width: clamp(columns - 2, 60, 120).
                        let width = terminal_columns().saturating_sub(2).clamp(60, 120);
                        let factory_runs =
                            crate::factory_view::parse_factory_runs(&self.factory_graph);
                        self.open_info_panel(
                            view,
                            None,
                            InfoContent::Rows(info_commands::context_tree_rows(
                                &tree,
                                width,
                                scope,
                                &factory_runs,
                            )),
                        );
                        self.track_menu_opened("context", "command");
                    }
                    Err(error) => {
                        self.error_row(&format!("{error:#}"), view);
                    }
                }
            }
            // The exact assembled prompt is unbounded, so it renders in the
            // scrollable info panel instead of flooding the transcript.
            "system-prompt" => {
                if !resolved.args.is_empty() {
                    view.editor.set_text(text);
                    self.error_row("Usage: /system-prompt", view);
                    return Ok(());
                }
                let prompt = self
                    .bounded_request(
                        Duration::from_millis(UI_REQUEST_TIMEOUT_MS),
                        DaemonCommand::GetSystemPrompt {
                            id: None,
                            active_session_id: self.active_session_id.clone(),
                            rest: Map::default(),
                        },
                    )
                    .await;
                match prompt {
                    Ok(data) => {
                        let prompt = data
                            .get("systemPrompt")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let mut rows = info_commands::system_prompt_header_rows(prompt);
                        rows.push(Vec::new());
                        rows.extend(info_commands::system_prompt_body_rows(prompt));
                        self.open_info_panel(view, None, InfoContent::Rows(rows));
                        self.track_menu_opened("system-prompt", "command");
                    }
                    Err(error) => {
                        self.error_row(&format!("{error:#}"), view);
                    }
                }
            }
            // A client-side read of the logs directory (the daemon writes it, this
            // client lists it).
            "logs" => {
                if !resolved.args.is_empty() {
                    view.editor.set_text(text);
                    self.error_row("Usage: /logs", view);
                    return Ok(());
                }
                let Some(agent_dir) = pa_types::platform::agent_dir() else {
                    self.error_row(
                        "home directory not found: set HOME (or USERPROFILE on Windows)",
                        view,
                    );
                    return Ok(());
                };
                self.open_info_panel(
                    view,
                    None,
                    InfoContent::Rows(info_commands::logs_rows(&agent_dir.join("logs"))),
                );
                self.track_menu_opened("logs", "command");
            }
            // The shipped CHANGELOG.md entries, newest first; the TS accent `What's
            // New` title is the panel's title.
            "changelog" => {
                if !resolved.args.is_empty() {
                    view.editor.set_text(text);
                    self.error_row("Usage: /changelog", view);
                    return Ok(());
                }
                self.open_info_panel(
                    view,
                    Some("What's New".to_string()),
                    InfoContent::Markdown(info_commands::changelog_markdown(
                        &Self::changelog_path(),
                    )),
                );
                self.track_menu_opened("changelog", "command");
            }
            "settings" => {
                if !resolved.args.is_empty() {
                    view.editor
                        .set_text(&format!("/{} {}", resolved.original_name, resolved.args));
                    self.error_row("Usage: /settings", view);
                    return Ok(());
                }
                self.open_settings_menu(view).await;
            }
            // Start a side question without touching the transcript; the pane stays
            // open for follow-ups until esc returns to the main thread.
            "btw" => {
                if resolved.args.is_empty() {
                    self.note_as("Usage: /btw <question>", StatusKind::Warning, view);
                    return Ok(());
                }
                self.start_side_question(&resolved.args, view).await?;
            }
            "name" => {
                let name = resolved.args.trim();
                if name.is_empty() {
                    match &self.session_name {
                        Some(current) => self.note(&format!("Session name: {current}"), view),
                        None => self.note_as("Usage: /name <name>", StatusKind::Warning, view),
                    }
                    return Ok(());
                }
                self.set_session_name(name, view).await;
            }
            "reload" => {
                if !resolved.args.is_empty() {
                    view.editor
                        .set_text(&format!("/{} {}", resolved.original_name, resolved.args));
                    self.error_row("Usage: /reload", view);
                    return Ok(());
                }
                if self.turn_active || view.working.is_some() || self.work_in_flight() {
                    self.note_as(
                        "Wait for the current response to finish before reloading.",
                        StatusKind::Warning,
                        view,
                    );
                    return Ok(());
                }
                if view.compaction.is_some() {
                    self.note_as(
                        "Wait for compaction to finish before reloading.",
                        StatusKind::Warning,
                        view,
                    );
                    return Ok(());
                }
                self.handle_reload_command(view)?;
            }
            "heartbeat" => self.handle_heartbeat_command(text, view).await,
            "heartbeats" => {
                if !resolved.args.is_empty() {
                    view.editor
                        .set_text(&format!("/{} {}", resolved.original_name, resolved.args));
                    self.error_row("Usage: /heartbeats", view);
                    return Ok(());
                }
                self.open_heartbeats_view(view);
            }
            other => {
                self.note(
                    &format!("/{other} is not available in this client yet"),
                    view,
                );
            }
        }
        Ok(())
    }

    /// Rename the active session through the daemon's `SetSessionName` seam —
    /// the one path the `/name` and `/new --name` arms share. On success the
    /// local name, the chrome's chat title, and the `Session name set:` row
    /// update; on failure the error row reports and the caller learns the
    /// rename did not land.
    async fn set_session_name(&mut self, name: &str, view: &mut AgentView) -> bool {
        match self
            .bounded_request(
                Duration::from_millis(UI_REQUEST_TIMEOUT_MS),
                DaemonCommand::SetSessionName {
                    id: None,
                    active_session_id: self.active_session_id.clone(),
                    name: name.to_string(),
                    worker_token: None,
                    rest: Map::default(),
                },
            )
            .await
        {
            Ok(_) => {
                self.session_name = Some(name.to_string());
                view.chrome.chat_name = self.session_display();
                self.plain_row(&format!("Session name set: {name}"), view);
                true
            }
            Err(error) => {
                self.error_row(&format!("{error:#}"), view);
                false
            }
        }
    }

    /// Report a menu surface opening (`tui menu opened`), fire-and-forget;
    /// `menu` names the surface, `source` how it opened.
    pub(super) fn track_menu_opened(&self, menu: &'static str, source: &'static str) {
        if let Some(telemetry) = self.telemetry.clone() {
            tokio::spawn(async move {
                telemetry.menu_opened(menu, source).await;
            });
        }
    }

    /// Fetch the session's slash-command catalog in the background: the response carries the
    /// `skill:` commands; a fetch races a rebind silently — the epoch drops the stale response.
    pub(crate) fn spawn_command_catalog_refresh(&mut self) {
        self.command_refresh_epoch += 1;
        let epoch = self.command_refresh_epoch;
        // The clear rides the same FIFO channel ahead of the fetch's response, so the old rows drop
        // immediately — a rebind never offers stale cross-session commands.
        let _ = self.command_updates.send(CommandCatalogUpdate {
            epoch,
            skill_commands: Vec::new(),
        });
        let updates = self.command_updates.clone();
        let client = self.client.clone();
        let active_session_id = self.active_session_id.clone();
        tokio::spawn(async move {
            let request = DaemonCommand::GetCommands {
                id: None,
                active_session_id,
                rest: Map::default(),
            };
            let fetched = tokio::time::timeout(
                Duration::from_millis(UI_REQUEST_TIMEOUT_MS),
                client.request_ok(request),
            )
            .await;
            let skill_commands = match fetched {
                Ok(Ok(data)) => crate::autocomplete::skill_command_entries(&data),
                // The catalog clears on failure.
                Ok(Err(_)) | Err(_) => Vec::new(),
            };
            let _ = updates.send(CommandCatalogUpdate {
                epoch,
                skill_commands,
            });
        });
    }

    /// Fold a landed command-catalog refresh into the session: the `skill:` commands replace the
    /// provider's list, gated by the `enableSkillCommands` setting; a stale epoch never applies.
    pub(crate) fn apply_command_catalog(
        &mut self,
        update: CommandCatalogUpdate,
        view: &mut AgentView,
    ) {
        if update.epoch < self.command_refresh_epoch {
            return;
        }
        // The cache keeps the raw fetch (the toggle re-applies it under the
        // setting's new value); the default applies when no settings seam exists.
        self.skill_commands_cache = update.skill_commands;
        let skills = if self
            .client_settings
            .as_ref()
            .is_none_or(|settings| settings.enable_skill_commands())
        {
            self.skill_commands_cache.clone()
        } else {
            Vec::new()
        };
        view.editor.set_autocomplete_skill_commands(skills);
        self.dirty = true;
    }

    /// The session's connection state (`get_connection_state` carries the connection fields;
    /// `get_state` serves the roster summary instead). `None` surfaces the failure as a note.
    pub(super) async fn connection_state(&mut self, view: &mut AgentView) -> Option<Value> {
        match self
            .bounded_request(
                Duration::from_millis(UI_REQUEST_TIMEOUT_MS),
                DaemonCommand::GetConnectionState {
                    id: None,
                    active_session_id: self.active_session_id.clone(),
                    rest: Map::default(),
                },
            )
            .await
        {
            Ok(data) => {
                // The queue delivery mode rides the state and is cached here (every state
                // read is the single refresh seam).
                if let Some(mode) = data.get("steeringMode").and_then(Value::as_str) {
                    self.steering_mode = mode.to_string();
                }
                Some(data)
            }
            Err(error) => {
                self.note(&format!("{error:#}"), view);
                None
            }
        }
    }
}
