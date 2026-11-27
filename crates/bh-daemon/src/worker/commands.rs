//! The dispatch surface: command routing, the command handlers,
//! and the abort family.
use super::{
    clear_withdrawn_progress_note, json, persist_custom_row, response_failure, response_success,
    KillCloseReason, Lane, QueueCheckpoint, QueuedItem, Result, SessionFile, TurnSettle, VecDeque,
    Worker, PROMPT_ABORTED_BEFORE_DELIVERY, SIDE_QUESTION_SETTLE_TIMEOUT,
};

use serde_json::Value;

use crate::engine::GoalControlRequest;
use crate::protocol::DaemonResponse;
use crate::session_store::SessionMessagesQuery;

impl Worker {
    pub(crate) async fn dispatch(&self, command_type: &str, payload: &Value) -> DaemonResponse {
        // Only operations that inspect or change historical branches need hydration.
        // Cancellation is deliberately excluded: it must reach the live turn immediately.
        let history_page = command_type == "get_messages"
            && transcript_page(payload).is_some_and(|page| page.reads_history());
        if history_page
            || matches!(
                command_type,
                "get_session_tree"
                    | "get_context_tree"
                    | "get_user_messages_for_forking"
                    | "set_session_entry_label"
                    | "navigate_tree"
                    | "fork"
                    | "export_html"
                    | "export_jsonl"
            )
        {
            if let Err(error) = self.hydrate_history().await {
                return response_failure(None, command_type, &error.to_string(), None);
            }
        }
        match command_type {
            "create" => self.handle_create(payload).await,
            "attach" => self.handle_attach(payload),
            "detach" => self.handle_detach(payload),
            "prompt" => self.handle_prompt(payload, false).await,
            "run_workflow" => self.start_workflow(payload),
            "get_workflow_run" => {
                let id = payload.get("runId").and_then(Value::as_str).unwrap_or_default();
                match self.workflow_runs.status(id) {
                    Some(status) => response_success(None, command_type, Some(status)),
                    None => response_failure(None, command_type, "Unknown workflow run", None),
                }
            }
            "list_workflows" => {
                let cwd = self.core.lock().unwrap().cwd.clone();
                let agent_dir = self.config.agent_dir.clone();
                match tokio::task::spawn_blocking(move || -> anyhow::Result<Value> {
                    let cwd = std::path::Path::new(&cwd);
                    let names = bh_core::workflows::list(cwd, &agent_dir)?;
                    let mut definitions = serde_json::Map::new();
                    let mut errors = serde_json::Map::new();
                    for name in &names {
                        match bh_core::workflows::load(cwd, &agent_dir, name) {
                            Ok(def) => {
                                definitions.insert(name.clone(), serde_json::to_value(def)?);
                            }
                            Err(e) => {
                                errors.insert(name.clone(), Value::String(format!("{e:#}")));
                            }
                        }
                    }
                    Ok(json!({"names": names, "definitions": definitions, "errors": errors}))
                })
                .await
                {
                    Ok(Ok(listing)) => response_success(None, command_type, Some(listing)),
                    Ok(Err(e)) => response_failure(None, command_type, &format!("{e:#}"), None),
                    Err(e) => response_failure(None, command_type, &e.to_string(), None),
                }
            }
            "prompt_and_wait" => self.handle_prompt(payload, true).await,
            "steer" => self.handle_queue(payload, Lane::Steering),
            "follow_up" => self.handle_queue(payload, Lane::FollowUp),
            "abort" => self.handle_abort(),
            "abort_and_send_queued" => self.handle_abort_and_send_queued(),
            "start_side_question" => {
                if let Err(response) = self.require_created("start_side_question") {
                    return response;
                }
                self.side_questions.start(payload)
            }
            "abort_side_question" => {
                if let Err(response) = self.require_created("abort_side_question") {
                    return response;
                }
                self.side_questions.abort(payload)
            }
            "compact" => self.handle_compaction(payload).await,
            "abort_compaction" => {
                self.compaction.abort();
                response_success(None, "abort_compaction", None)
            }
            "set_auto_compaction" => self.handle_set_auto_compaction(payload).await,
            "wait_for_idle" => self.handle_wait_for_idle(payload).await,
            "wait_for_headless_completion" => {
                self.handle_wait_for_headless_completion(payload).await
            }
            "get_state" => self.handle_get_state().await,
            "goal_control" => self.handle_goal_control(payload).await,
            "get_messages" => self.handle_get_messages(payload),
            "get_session_messages" => self.handle_get_session_messages(payload).await,
            "get_session_header" => self.handle_get_session_header(),
            "get_session_stats" => self.handle_get_session_stats(),
            "get_model_catalog" => self.handle_get_model_catalog().await,
            "get_queue" => self.handle_get_queue(),
            "clear_queue" => self.handle_clear_queue(),
            "abort_and_clear_queue" => self.handle_abort_and_clear_queue(),
            "get_last_assistant_text" => self.handle_get_last_assistant_text(),
            "get_connection_state" => self.handle_get_connection_state(),
            "get_mcp_connections" => self.handle_get_mcp_connections().await,
            "set_mcp_static_token" => self.handle_set_mcp_static_token(payload).await,
            "remove_mcp_connection" => self.handle_remove_mcp_connection(payload).await,
            "get_rlm_children" => self.handle_get_rlm_children().await,
            "get_context_tree" => self.handle_get_context_tree().await,
            "get_commands" => self.handle_get_commands().await,
            "get_resource_snapshot" => self.handle_get_resource_snapshot().await,
            "get_session_context" => self.handle_get_session_context(),
            "set_session_context" => self.handle_set_session_context(payload).await,
            "get_system_prompt" => self.handle_get_system_prompt().await,
            "session_prompt_segments" => self.handle_prompt_segments(payload).await,
            "get_tool_definition" => self.handle_get_tool_definition(payload).await,
            "get_rlm_max_depth_status" => self.handle_get_rlm_max_depth_status(),
            "get_available_models" => self.handle_get_available_models(),
            "worker_deliver_message" => self.handle_worker_deliver_message(payload),
            "update_snapshot" => self.handle_update_snapshot(),
            "kill" => self.handle_kill(payload).await,
            "shutdown" => self.handle_shutdown(payload).await,
            "rename" => self.handle_rename("rename", payload),
            "set_session_name" => self.handle_rename("set_session_name", payload),
            "mark_anthropic_warning_shown" => self.handle_mark_anthropic_warning_shown(),
            "rename_saved_session" => self.handle_rename_saved_session(payload),
            "delete_saved_session" => self.handle_delete_saved_session(payload).await,
            "replace_acp_mcp_servers" => self.handle_replace_acp_mcp_servers(payload),
            "set_model" => self.handle_set_model(payload).await,
            "set_thinking_level" => self.handle_set_thinking_level(payload).await,
            "cycle_model" => self.handle_cycle_model(payload).await,
            "set_scoped_models" => self.handle_set_scoped_models(payload),
            "cycle_thinking_level" => self.handle_cycle_thinking_level().await,
            "set_service_tier" => self.handle_set_service_tier(payload).await,
            "set_transport" => self.handle_set_transport(payload),
            "set_steering_mode" => self.handle_set_queue_mode("set_steering_mode", payload),
            "set_follow_up_mode" => self.handle_set_queue_mode("set_follow_up_mode", payload),
            "set_auto_retry" => self.handle_set_auto_retry(payload),
            "abort_retry" => self.handle_abort_retry(),
            "get_session_tree" => self.tree_navigation.get_session_tree(),
            "get_user_messages_for_forking" => self.tree_navigation.get_user_messages_for_forking(),
            "set_session_entry_label" => self.tree_navigation.set_session_entry_label(payload),
            "navigate_tree" => self.handle_navigate_tree(payload).await,
            "fork" => self.handle_fork(payload).await,
            "abort_branch_summary" => {
                self.tree_navigation.abort();
                response_success(None, "abort_branch_summary", None)
            }
            "export_html" => self.exports.export_html(payload).await,
            "export_jsonl" => self.exports.export_jsonl(payload),
            "mutate_queued_message" => self.handle_mutate_queued_message(payload),
            "resume_queue" => self.handle_resume_queue(),
            "factory_activity" => self.handle_factory_activity(payload).await,
            "execute_bash" => self.handle_execute_bash(payload),
            "execute_bash_and_wait" => self.handle_execute_bash_and_wait(payload).await,
            "abort_bash" => self.handle_abort_bash(),
            "list_kernel_bash" | "tail_kernel_bash" | "kill_kernel_bash" => {
                self.handle_kernel_bash_activity(command_type, payload)
                    .await
            }
            "append_custom_message" => self.handle_append_custom_message(payload),
            "restore_next_turn" => self.handle_restore_next_turn(payload),
            "restore_actions" => self.handle_restore_actions(payload),
            "refine" => self.handle_refine(payload).await,
            "reload" => self.handle_reload().await,
            "cancel_rlm_child" => self.handle_cancel_rlm_child(payload).await,
            "delete_rlm_subagent" => self.handle_delete_rlm_subagent(payload).await,
            // The engine call blocks on the engine runtime, so it
            // runs on a blocking thread like every other engine call.
            "set_rlm_max_depth" => self.handle_set_rlm_max_depth(payload).await,
            "acquire_session_input_pause" => self.handle_acquire_session_input_pause(payload),
            "release_session_input_pause" => self.handle_release_session_input_pause(payload),
            "cancel_prompt_admission" => self.handle_cancel_prompt_admission(payload),
            "new_session" => self.handle_new_session(payload).await,
            "switch_session" => self.handle_switch_session(payload).await,
            "import_jsonl" => self.handle_import_jsonl(payload).await,
            "agent_messages_status" => self.handle_agent_messages_status(),
            "agent_messages_pause" => self.handle_agent_messages_pause(),
            "agent_messages_resume" => self.handle_agent_messages_resume(),
            "agent_messages_clear" => self.handle_agent_messages_clear(),
            "cron_list" => self.handle_cron_list(payload),
            "heartbeats_list" => self.handle_heartbeats_list(),
            "heartbeat_manage" => self.handle_heartbeat_manage(payload).await,
            "cron_add" => self.handle_cron_add(payload).await,
            "cron_cancel" => self.handle_cron_cancel(payload).await,
            "heartbeat_get" => self.handle_heartbeat_get(payload),
            "heartbeat_set" => self.handle_heartbeat_set(payload).await,
            "heartbeat_update" => self.handle_heartbeat_update(payload).await,
            other => response_failure(
                None,
                command_type,
                &format!("Unknown worker command: {other}"),
                None,
            ),
        }
    }

    fn start_workflow(&self, payload: &Value) -> DaemonResponse {
        const COMMAND: &str = "run_workflow";
        if let Err(reply) = self.require_created(COMMAND) {
            return reply;
        }
        let Some(name) = payload["name"].as_str().filter(|s| !s.trim().is_empty()) else {
            return response_failure(None, COMMAND, "Workflow name is required", None);
        };
        let host = match self.engine.workflow_host() {
            Ok(host) => host,
            Err(e) => return response_failure(None, COMMAND, &e.to_string(), None),
        };
        let cwd = self.core.lock().unwrap().cwd.clone();
        let definition =
            match bh_core::workflows::load(std::path::Path::new(&cwd), &self.config.agent_dir, name) {
            Ok(definition) => definition,
            Err(error) => {
                return response_failure(None, COMMAND, &format!("{error:#}"), None);
            }
        };
        let apply_context_field = definition.interactive_apply_context_field;
        let input = payload["input"].clone();
        let memory_apply = match definition.harness_memory {
            Some(memory) => {
                let Some(data) = input.as_object() else {
                    return response_failure(None, COMMAND, "Workflow input must be an object", None);
                };
                let scope = match crate::rlm_children::workflow_memory_scope(
                    data,
                    &memory.scope_input_field,
                ) {
                    Ok(scope) => scope,
                    Err(error) => {
                        return response_failure(None, COMMAND, &error.to_string(), None);
                    }
                };
                let dir = match scope {
                    bh_core::refinement::HarnessScope::Global => {
                        bh_core::refinement::get_global_harness_state_dir(&self.config.agent_dir)
                    }
                    bh_core::refinement::HarnessScope::Local => {
                        let core = self.core.lock().unwrap();
                        let Some(log) = core.store.as_ref().map(|store| store.path.as_path()) else {
                            return response_failure(
                                None,
                                COMMAND,
                                "A saved session is required to rewrite local memory",
                                None,
                            );
                        };
                        let Some(dir) =
                            bh_core::session_engine::harness_digest::local_harness_dir_for_log(log)
                        else {
                            return response_failure(
                                None,
                                COMMAND,
                                "Cannot resolve the local harness directory",
                                None,
                            );
                        };
                        dir
                    }
                };
                Some((memory.interactive_apply_output_field, dir, scope))
            }
            None => None,
        };
        let applies_state = apply_context_field.is_some() || memory_apply.is_some();
        let interrupt = payload
            .get("interrupt")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let continue_turn = payload
            .get("continueTurn")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let (session, queued_for_idle, run_id, deduplicated) = {
            let mut core = self.core.lock().unwrap();
            let session = core.generation.clone();
            let queued = applies_state && (core.busy || core.compacting);
            let (run_id, deduplicated) = self.workflow_runs.begin(&session, name, &input);
            if applies_state && !queued && !deduplicated {
                // Reuse the compaction admission gate: no parent turn may start
                // while an isolated reviewer is preparing a replacement.
                core.compacting = true;
            }
            (session, queued, run_id, deduplicated)
        };
        if deduplicated {
            return response_success(
                None,
                COMMAND,
                Some(json!({"started":false,"queued":queued_for_idle,"deduplicated":true,"runId":run_id})),
            );
        }
        if interrupt {
            self.request_abort();
        }
        let name = name.to_owned();
        let core = self.core.clone();
        let events = self.events.clone();
        let engine = std::sync::Arc::clone(&self.engine);
        let idle_notify = std::sync::Arc::clone(&self.idle_notify);
        let queued_idle_notify = std::sync::Arc::clone(&idle_notify);
        let queued_core = self.core.clone();
        let queued_session = session.clone();
        let workflow_runs = std::sync::Arc::clone(&self.workflow_runs);
        let recovery = std::sync::Arc::clone(&self.recovery);
        let work_notify = std::sync::Arc::clone(&self.work_notify);
        let task_run_id = run_id.clone();
        tokio::spawn(async move {
            if queued_for_idle {
                loop {
                    let idle = queued_idle_notify.notified();
                    tokio::pin!(idle);
                    idle.as_mut().enable();
                    let admitted = {
                        let mut core = queued_core.lock().unwrap();
                        if core.generation != queued_session || core.shutdown_requested {
                            workflow_runs.fail(
                                &task_run_id,
                                "target session changed before workflow admission",
                            );
                            return;
                        }
                        if core.busy || core.compacting {
                            false
                        } else {
                            core.compacting = true;
                            true
                        }
                    };
                    if admitted {
                        break;
                    }
                    idle.await;
                }
            }
            workflow_runs.mark_running(&task_run_id);
            let mut result = host.run_workflow(&name, input).await;
            let mut memory_backup = None;
            if let (Ok(output), Some((field, dir, scope))) = (&result, &memory_apply) {
                let applied = output
                    .get(field)
                    .ok_or_else(|| {
                        anyhow::anyhow!("workflow memory output field {field:?} is missing")
                    })
                    .and_then(|memories| {
                        bh_core::refinement::apply_workflow_memories(&dir, *scope, memories)
                    });
                match applied {
                    Ok(backup) => memory_backup = backup,
                    Err(error) => {
                        result = Err(anyhow::anyhow!(
                            "apply workflow memory rewrite: {error:#}"
                        ));
                    }
                }
            }
            let (mut text, mut details) = match &result {
                Ok(output) => (
                    format!("Workflow {name}\n{output:#}"),
                    json!({"workflow":name,"severity":"info","output":output}),
                ),
                Err(error) => (
                    format!("Workflow {name} failed: {error:#}"),
                    json!({"workflow":name,"severity":"error","error":format!("{error:#}")}),
                ),
            };
            if let Some(backup) = memory_backup {
                details["memoryApplied"] = json!(true);
                details["memoryBackup"] = json!(backup);
            }
            let summary = result.as_ref().ok().and_then(|output| {
                apply_context_field
                    .as_deref()
                    .and_then(|field| output.get(field))
                    .and_then(Value::as_str)
                    .filter(|summary| !summary.trim().is_empty())
                    .map(str::to_owned)
            });
            if apply_context_field.is_some() && summary.is_none() && result.is_ok() {
                text.push_str("\nContext rewrite failed: the configured output field was empty.");
                details["severity"] = json!("error");
                details["contextApplied"] = json!(false);
            }
            let row = json!({"role":"custom","customType":bh_types::slash_commands::SESSION_SLASH_COMMAND_RESULT_CUSTOM_TYPE,
                "content":text,"display":true,"timestamp":crate::util::now_ms(),"details":details});
            let branch_entries = {
                let mut core = core.lock().unwrap();
                if core.generation != session {
                    drop(core);
                    workflow_runs.fail(
                        &task_run_id,
                        "target session changed while workflow was running",
                    );
                    return;
                }
                persist_custom_row(&mut core, &row);
                let branch_entries = summary.as_ref().and_then(|summary| {
                    let store = core.store.as_mut()?;
                    store
                        .persist_entry(
                            "compaction",
                            workflow_context_compaction(summary, &name),
                        )
                        .ok()?;
                    Some(store.branch_file_entries())
                });
                super::broadcast_custom_row_locked(&mut core, &events, &row);
                branch_entries
            };
            if let Some(branch_entries) = branch_entries {
                if let Err(error) = crate::branch_navigation::rebuild_engine_context(
                    &engine,
                    branch_entries,
                    bh_core::session_engine::goal_driver::GoalBranchReload::SameTimeline,
                )
                .await
                {
                    eprintln!("bh-daemon: workflow context rebuild failed: {error}");
                }
            }
            if applies_state {
                let mut core = core.lock().unwrap();
                if core.generation == session {
                    core.compacting = false;
                }
                idle_notify.notify_waiters();
            }
            if continue_turn && result.is_ok() {
                let queued = {
                    let mut core = core.lock().unwrap();
                    if core.generation != session || core.shutdown_requested {
                        false
                    } else {
                        let busy = core.busy;
                        core.queued_input_suspended = false;
                        let item = QueuedItem {
                            priority: super::QueuePriority::Human,
                            preview: None,
                            message: "Continue from the updated session state. Resume the interrupted task without repeating completed work.".to_string(),
                            custom_message: None,
                            agent_message: None,
                            queue_key: Some(format!("workflow-continuation:{task_run_id}")),
                            admission_id: None,
                            images: Vec::new(),
                            done: None,
                            queue_visible: busy,
                            policy: if busy { super::TurnPolicy::Queued } else { super::TurnPolicy::Direct },
                            forced_batch: false,
                        };
                        if busy {
                            super::enqueue_priority(&mut core.follow_up, item);
                        } else {
                            super::enqueue_priority(&mut core.steering, item);
                        }
                        true
                    }
                };
                if queued {
                    super::checkpoint_queue_recovery(
                        &recovery,
                        &core,
                        QueueCheckpoint::Admitted {
                            operation: "workflow_continuation",
                        },
                    );
                    work_notify.notify_one();
                }
            }
            workflow_runs.finish(&task_run_id, &result);
        });
        response_success(
            None,
            COMMAND,
            Some(json!({"started":true,"queued":queued_for_idle,"deduplicated":false,"runId":run_id})),
        )
    }

    // DaemonResponse is the wire response struct and is deliberately wide; the
    // error channel here carries the whole response, so allow the large-err lint.
    #[allow(clippy::result_large_err)]
    pub(crate) fn require_created(&self, command_type: &str) -> Result<(), DaemonResponse> {
        let core = self.core.lock().unwrap();
        if !core.created {
            return Err(response_failure(
                None,
                command_type,
                "Session is still initializing",
                None,
            ));
        }
        // The shutdown admission gate: a command dispatched while the graceful stop
        // is closing must not start new work the exit would orphan (an execute_bash
        // racing the shutdown would spawn a child the exit leaves running).
        if core.shutdown_requested {
            return Err(response_failure(
                None,
                command_type,
                "Session is shutting down",
                None,
            ));
        }
        Ok(())
    }

    /// `navigate_tree` with the reload's announcement: announced before the
    /// response reaches the client, so surfaces never show the pre-navigation goal.
    /// The whole navigation runs under the queued-work pause (TS
    /// `_navigateTree`'s `acquireQueuedWorkPause` + `waitForIdle`).
    async fn handle_navigate_tree(&self, payload: &Value) -> DaemonResponse {
        let _queued_work = self.hold_queued_work_pause("navigate");
        let response = self.tree_navigation.navigate_tree(payload).await;
        if response.success {
            if let Some(goal) = self.engine.goal_update_after_rebuild() {
                self.emit_worker_event(json!({
                    "type": "goal_update",
                    "goal": goal,
                }));
            }
        }
        response
    }

    /// The session's owner-fenced ACP MCP store: the ACP transport resolves and
    /// validates the servers before sending them; the worker fences ownership,
    /// guards the busy turn, and rolls back a failed replacement.
    fn handle_replace_acp_mcp_servers(&self, payload: &Value) -> DaemonResponse {
        let owner_id = payload
            .get("ownerId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if owner_id.is_empty() {
            return response_failure(
                None,
                "replace_acp_mcp_servers",
                "ACP MCP owner id is required",
                None,
            );
        }
        let servers: Vec<bh_core::mcp::AcpMcpServerConfig> = payload
            .get("servers")
            .cloned()
            .map(|servers| serde_json::from_value(servers).unwrap_or_default())
            .unwrap_or_default();
        // The agent cannot adopt a different MCP tool list mid-turn (TS
        // `session.isStreaming` guard).
        if !servers.is_empty() && self.core.lock().unwrap().busy {
            return response_failure(
                None,
                "replace_acp_mcp_servers",
                "Cannot replace ACP MCP servers while the agent is running",
                None,
            );
        }
        // The real agent engine owns the session's MCP store (one store for admission
        // and prompt gating); scripted harness engines fall back to the worker-level store.
        let manager = self
            .engine
            .acp_mcp_manager()
            .unwrap_or_else(|| std::sync::Arc::clone(&self.acp_mcp));
        let manager = manager.lock().unwrap();
        match manager.replace_acp_servers(&servers, owner_id) {
            // An unchanged list (same owner, identical servers) is a
            // no-op success.
            Ok(_) => response_success(None, "replace_acp_mcp_servers", None),
            Err(error) => {
                // Roll back any partially applied configuration with
                // the owner-scoped clear before surfacing the failure.
                if manager.can_release_acp_servers(owner_id) {
                    let _ = manager.replace_acp_servers(&[], owner_id);
                }
                response_failure(None, "replace_acp_mcp_servers", &error.to_string(), None)
            }
        }
    }

    /// Arm the one-shot forced steering batch: the visible plain-user steering
    /// items deliver as ONE batched turn at the next boundary, even under queue
    /// mode "one-at-a-time". Returns whether anything armed.
    pub(crate) fn arm_forced_all_steering(&self) -> bool {
        let mut core = self.core.lock().unwrap();
        let armable = |item: &QueuedItem| {
            item.queue_visible && item.agent_message.is_none() && item.custom_message.is_none()
        };
        if !core.steering.iter().any(armable) {
            return false;
        }
        core.forced_all_steering = true;
        for item in &mut core.steering {
            if armable(item) {
                item.forced_batch = true;
            }
        }
        true
    }

    /// `abort_and_send_queued` (schema 29): abort the active run, keep the queue
    /// flowing — the armed steering rows co-deliver, then the follow-up drains.
    ///
    /// SANCTIONED DIVERGENCE (operator ruling 2026-09-25): TS parks the queue
    /// whenever nothing is armable; here the abort resumes. The same divergence
    /// covers a held admission pause: the abort clears the suspension and the
    /// pump's own pause gate holds the delivery until the release re-notifies
    /// it. Returns whether the queue was resumed.
    pub(crate) fn abort_and_send_queued(&self) -> bool {
        // TS `canResume` also parks under an admission pause; the pump's own
        // pause gate is the one hold that matters, so only a shutting-down
        // worker (whose queue survives via the restart journal) stays parked.
        let can_resume = !self.core.lock().unwrap().shutdown_requested;
        if !can_resume {
            self.request_abort();
            return false;
        }
        self.arm_forced_all_steering();
        // The cancel sweep keeps only the queue-visible rows, so the emptiness
        // read below measures exactly the work the abort leaves behind.
        self.request_abort();
        // The resumed pump — not this funnel — owns the delivery at
        // the settled turn's boundary.
        let queued_work = {
            let core = self.core.lock().unwrap();
            !core.steering.is_empty() || !core.follow_up.is_empty()
        };
        if !queued_work {
            return false;
        }
        self.resume_queued_input();
        true
    }

    /// The abort funnel behind both abort commands: suspend queued-input
    /// admission, cancel the queue-invisible turn actions, abort the in-flight
    /// compaction, and cancel the running turn.
    fn request_abort(&self) {
        {
            let mut core = self.core.lock().unwrap();
            core.abort_requested = true;
            core.turn_cancelled = true;
            // The queue parks and a plain prompt is rejected until a
            // resume site fires.
            core.queued_input_suspended = true;
        }
        // Queue-INVISIBLE turn actions cancel: a direct prompt admitted on an idle
        // session never became a queue row, so the abort must resolve its waiting
        // response, not park it behind the suspension forever. Queue-visible lanes
        // survive parked — the suspension defers the pump, it never drops the queue.
        {
            let mut core = self.core.lock().unwrap();
            let cancel = |lane: &mut VecDeque<QueuedItem>| {
                let mut kept = VecDeque::new();
                while let Some(item) = lane.pop_front() {
                    if item.queue_visible {
                        kept.push_back(item);
                    } else {
                        if let Some(id) = &item.admission_id {
                            // A withdrawn prompt clears its admission (TS clearAdmission).
                            self.prompt_admissions.clear(id);
                        }
                        if let Some(done) = item.done {
                            let _ = done.send(TurnSettle::Withdrawn(
                                PROMPT_ABORTED_BEFORE_DELIVERY.to_string(),
                            ));
                        }
                    }
                }
                *lane = kept;
            };
            cancel(&mut core.steering);
            cancel(&mut core.follow_up);
            clear_withdrawn_progress_note(&core, self.engine.as_ref());
        }
        // A withdrawn continuation's pending guard already released at ITS admission,
        // so the withdraw clears nothing (a mirror clear could drop an unrelated guard).
        self.compaction.abort();
        // The in-flight turn's fetch cancels now, not at its next
        // streamed event.
        self.engine.abort_in_flight_turn();
        // An idle withdrawal has no turn-end event to refresh the roster.
        self.push_roster_delta();
    }

    fn handle_abort(&self) -> DaemonResponse {
        self.request_abort();
        response_success(None, "abort", None)
    }

    fn handle_abort_and_send_queued(&self) -> DaemonResponse {
        // The response acknowledges the abort itself, never the
        // async deliveries the resumed pump runs at the settle.
        self.abort_and_send_queued();
        response_success(None, "abort_and_send_queued", None)
    }

    async fn handle_get_state(&self) -> DaemonResponse {
        if let Err(response) = self.require_created("get_state") {
            return response;
        }
        let mut data = {
            let (core, inputs) = self.summary_inputs();
            let summary = self.summary_locked(&core, inputs);
            serde_json::to_value(&summary).unwrap_or(Value::Null)
        };
        let daybreak_blue = match &self.agent_engine {
            Some(engine) => engine.daybreak_blue().await,
            None => false,
        };
        if let Some(fields) = data.as_object_mut() {
            fields.insert("daybreakBlue".to_string(), Value::Bool(daybreak_blue));
        }
        response_success(None, "get_state", Some(data))
    }

    /// `get_session_header`: the persisted session header line (TS wraps it
    /// in `{ header: ... }`).
    fn handle_get_session_header(&self) -> DaemonResponse {
        if let Err(response) = self.require_created("get_session_header") {
            return response;
        }
        let core = self.core.lock().unwrap();
        let Some(store) = core.store.as_ref() else {
            return response_failure(
                None,
                "get_session_header",
                "Session is still initializing",
                None,
            );
        };
        response_success(
            None,
            "get_session_header",
            Some(json!({ "header": crate::session_store::session_header_line(&store.header) })),
        )
    }

    /// `get_session_stats`: counts, token totals, and the context-usage
    /// estimate over the persisted branch (TS `getSessionStats`).
    /// `goal_control`: another session's goal operation on this session's goal.
    async fn handle_goal_control(&self, payload: &Value) -> DaemonResponse {
        const COMMAND: &str = "goal_control";
        if let Err(response) = self.require_created(COMMAND) {
            return response;
        }
        let field = |name: &str| payload.get(name).filter(|value| !value.is_null());
        let request = match payload.get("op").and_then(Value::as_str) {
            Some("get") => GoalControlRequest::Get,
            Some("pause") => GoalControlRequest::Pause,
            Some("resume") => GoalControlRequest::Resume,
            Some("stop") => GoalControlRequest::Stop,
            Some("update") => GoalControlRequest::Update {
                objective: field("objective").and_then(Value::as_str).map(str::to_string),
                token_budget: field("tokenBudget").and_then(Value::as_u64),
            },
            Some(_) | None => {
                return response_failure(
                    None,
                    COMMAND,
                    "goal_control op must be get, pause, resume, update, or stop",
                    None,
                )
            }
        };
        let engine = std::sync::Arc::clone(&self.engine);
        match tokio::task::spawn_blocking(move || engine.control_goal(request)).await {
            Ok(Ok(outcome)) => {
                if let Some(goal) = outcome.goal_update {
                    self.emit_worker_event(json!({ "type": "goal_update", "goal": goal }));
                }
                response_success(None, COMMAND, Some(outcome.response))
            }
            Ok(Err(error)) => response_failure(None, COMMAND, &format!("{error:#}"), None),
            Err(error) => response_failure(None, COMMAND, &error.to_string(), None),
        }
    }

    fn handle_get_session_stats(&self) -> DaemonResponse {
        if let Err(response) = self.require_created("get_session_stats") {
            return response;
        }
        let core = self.core.lock().unwrap();
        let Some(store) = core.store.as_ref() else {
            return response_failure(
                None,
                "get_session_stats",
                "Session is still initializing",
                None,
            );
        };
        let stats = crate::session_stats::session_stats(store, self.engine.model_context_window());
        response_success(None, "get_session_stats", Some(stats))
    }

    fn handle_get_messages(&self, payload: &Value) -> DaemonResponse {
        if let Err(response) = self.require_created("get_messages") {
            return response;
        }
        let page = transcript_page(payload);
        let capabilities = payload
            .get("capabilities")
            .and_then(Value::as_array)
            .map(|capabilities| {
                capabilities
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let core = self.core.lock().unwrap();
        let Some(page) = page else {
            let mut messages = core
                .store
                .as_ref()
                .map(SessionFile::messages)
                .unwrap_or_default();
            if crate::snapshot_stream::wants_image_elision(&capabilities) {
                crate::snapshot_stream::elide_snapshot_image_payloads(&mut messages);
            }
            return response_success(None, "get_messages", Some(json!({ "messages": messages })));
        };
        let Some(store) = core.store.as_ref() else {
            return response_failure(None, "get_messages", "History branch changed", None);
        };
        // Validate ancestry atomically with the page read. Appends keep the
        // attached leaf on this branch; navigation to another branch does not.
        let history_leaf = match payload.get("historyLeafId").map(Value::as_str) {
            Some(Some(leaf)) if store.branch_contains(leaf) => leaf.to_string(),
            Some(_) => {
                return response_failure(None, "get_messages", "History branch changed", None);
            }
            None => store.leaf_id().unwrap_or_default().to_string(),
        };
        let Some(mut rows) = store.transcript_page(&page) else {
            return response_failure(None, "get_messages", "History branch changed", None);
        };
        if crate::snapshot_stream::wants_image_elision(&capabilities) {
            crate::snapshot_stream::elide_snapshot_image_payloads(&mut rows.messages);
        }
        response_success(
            None,
            "get_messages",
            Some(json!({
                "messages": rows.messages,
                "firstEntryId": rows.first_entry_id,
                "lastEntryId": rows.last_entry_id,
                "more": rows.more,
                "historyLeafId": history_leaf,
            })),
        )
    }

    /// `get_session_messages`: one slice of the session's display transcript, filtered by
    /// role globs; a slice into a windowed store's unloaded prefix hydrates the store first.
    async fn handle_get_session_messages(&self, payload: &Value) -> DaemonResponse {
        const NAME: &str = "get_session_messages";
        if let Err(response) = self.require_created(NAME) {
            return response;
        }
        let roles: Vec<String> = payload
            .get("roles")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        let query = match SessionMessagesQuery::new(
            payload.get("start").and_then(Value::as_i64).unwrap_or(0),
            payload.get("limit").and_then(Value::as_u64).unwrap_or(0),
            &roles,
        ) {
            Ok(query) => query,
            Err(error) => return response_failure(None, NAME, &error.to_string(), None),
        };
        let read = || {
            let mut core = self.core.lock().unwrap();
            let core = &mut *core;
            let slice = core
                .store
                .as_mut()
                .and_then(|store| query.read(store, &core.running_tool_calls));
            slice
        };
        let mut slice = read();
        if slice.is_none() {
            if let Err(error) = self.hydrate_history().await {
                return response_failure(None, NAME, &error.to_string(), None);
            }
            slice = read();
        }
        let Some(slice) = slice else {
            return response_failure(None, NAME, "Session history is unavailable", None);
        };
        response_success(None, NAME, Some(slice.into_reply()))
    }

    /// Load a windowed store's full history in place; a full store stays as it is.
    async fn hydrate_history(&self) -> Result<()> {
        let Some(path) = self
            .core
            .lock()
            .unwrap()
            .store
            .as_ref()
            .filter(|store| store.window.is_some())
            .map(|store| store.path.clone())
        else {
            return Ok(());
        };
        let load_path = path.clone();
        let full = tokio::task::spawn_blocking(move || SessionFile::open(&load_path)).await??;
        if let Some(store) = self
            .core
            .lock()
            .unwrap()
            .store
            .as_mut()
            .filter(|store| store.path == path)
        {
            store.install_full_history(full);
        }
        // The hydrated full file was a transient whole-file copy on
        // top of the installed history: release its freed heap.
        bh_types::memory_release::trim_freed_heap();
        Ok(())
    }

    fn handle_get_queue(&self) -> DaemonResponse {
        if let Err(response) = self.require_created("get_queue") {
            return response;
        }
        let core = self.core.lock().unwrap();
        // The labeled preview when the delivery carries one, else the
        // message text.
        response_success(
            None,
            "get_queue",
            Some(json!({
                "steering": core.steering.iter().map(|item| item.preview.clone().unwrap_or_else(|| item.message.clone())).collect::<Vec<_>>(),
                "followUp": core.follow_up.iter().map(|item| item.preview.clone().unwrap_or_else(|| item.message.clone())).collect::<Vec<_>>(),
            })),
        )
    }

    fn handle_clear_queue(&self) -> DaemonResponse {
        if let Err(response) = self.require_created("clear_queue") {
            return response;
        }
        let mut core = self.core.lock().unwrap();
        // The dropped agent-message rows bounce to their senders: a queued
        // row's sender otherwise never learns its message will not deliver.
        let mut dropped_agent_rows = Vec::new();
        let drain_lane =
            |lane: &mut VecDeque<QueuedItem>, dropped_agent_rows: &mut Vec<Value>| -> Vec<String> {
                lane.drain(..)
                    .map(|item| {
                        if let Some(id) = item.admission_id.as_deref() {
                            self.prompt_admissions.clear(id);
                        }
                        if item.agent_message.is_some() {
                            dropped_agent_rows.extend(item.custom_message);
                        }
                        item.message
                    })
                    .collect()
            };
        let steering: Vec<String> = drain_lane(&mut core.steering, &mut dropped_agent_rows);
        let follow_up: Vec<String> = drain_lane(&mut core.follow_up, &mut dropped_agent_rows);
        clear_withdrawn_progress_note(&core, self.engine.as_ref());
        let snapshot = Self::snapshot_locked(&core);
        drop(core);
        // The verdict refresh rides the same checkpoint as the snapshot: a stale
        // busy=true from the cleared items' admission must not revive an empty session.
        self.checkpoint_queue(QueueCheckpoint::Settle {
            operation: "queue_cleared",
        });
        let _ = self.emit_action_update(&snapshot);
        for row in &dropped_agent_rows {
            self.bounce_cleared_agent_message(row);
        }
        response_success(
            None,
            "clear_queue",
            Some(json!({ "steering": steering, "followUp": follow_up })),
        )
    }

    /// One dropped agent-message row's sender notice: the row's own
    /// `details` name the sender and the body, and the bounce rides the
    /// supervisor `send_message` route from this session. Spawned off the
    /// clear's response path — a failed bounce is logged, never retried.
    fn bounce_cleared_agent_message(&self, row: &Value) {
        let Some(details) = row.get("details") else {
            return;
        };
        let sender = details
            .get("from")
            .and_then(|from| from.get("activeSessionId"))
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty());
        let Some(sender) = sender else {
            // CLI-origin senders carry no session to bounce to.
            return;
        };
        let message = details
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        // The cleared queue is this worker's own; its identity — not the
        // row's delivery-time copy — names the receiver.
        let receiver = {
            let core = self.core.lock().unwrap();
            core.store
                .as_ref()
                .and_then(|store| store.session_name().map(str::to_string))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| self.config.active_session_id.clone())
        };
        let command = json!({
            "type": "send_message",
            "targetActiveSessionId": sender,
            "message": cleared_agent_message_bounce_text(&receiver, message),
            "fromActiveSessionId": self.config.active_session_id,
            "agentOrigin": true,
        });
        let link = std::sync::Arc::clone(&self.roster_link);
        let sender = sender.to_string();
        let timeout = std::time::Duration::from_millis(crate::messaging::WORKER_REQUEST_TIMEOUT_MS);
        tokio::spawn(async move {
            if let Err(error) = link.request_success(command, timeout).await {
                eprintln!("bh-daemon: cleared agent-message bounce to {sender} failed: {error:#}");
            }
        });
    }

    fn handle_abort_and_clear_queue(&self) -> DaemonResponse {
        let cleared = self.handle_clear_queue();
        if !cleared.success {
            return cleared;
        }
        let mut core = self.core.lock().unwrap();
        core.abort_requested = true;
        core.turn_cancelled = true;
        // The same TS `requestAbort()` suspension as the bare `abort`.
        core.queued_input_suspended = true;
        drop(core);
        // And the same eager agent abort.
        self.engine.abort_in_flight_turn();
        response_success(None, "abort_and_clear_queue", cleared.data)
    }

    fn handle_get_last_assistant_text(&self) -> DaemonResponse {
        if let Err(response) = self.require_created("get_last_assistant_text") {
            return response;
        }
        let core = self.core.lock().unwrap();
        // The newest non-empty assistant text: an ended turn's final
        // assistant row may hold only the tool call, so the answer sits in
        // the last assistant row with text (the file fallback's rule).
        let text = core.store.as_ref().and_then(|store| {
            store
                .messages()
                .into_iter()
                .rev()
                .filter(|message| crate::types::message_role(message) == Some("assistant"))
                .map(|message| crate::types::message_text(&message))
                .find(|text| !text.is_empty())
        });
        response_success(
            None,
            "get_last_assistant_text",
            Some(json!({ "text": text })),
        )
    }

    async fn handle_kill(&self, payload: &Value) -> DaemonResponse {
        let reason = KillCloseReason::from_payload(payload);
        // The session is closing: the continuation mint sites and
        // their settle-hook retries bail from here on.
        if let Some(agent_engine) = &self.agent_engine {
            agent_engine.mark_session_closed();
        }
        // TS `closeSession` aborts the side questions per attached client
        // before `closeSessionOnce`'s arms run.
        self.side_questions
            .abort_all_and_settle(SIDE_QUESTION_SETTLE_TIMEOUT)
            .await;
        // `killed` cancels the session's scheduled jobs; `replaced` keeps the plain
        // cron jobs but cancels the subagent's RLM heartbeats; `shutdown` keeps them
        // all. The store cancel is durable, so the stopped session's own heartbeats
        // can never revive it.
        let cancel_result = match reason {
            KillCloseReason::Killed => self.cancel_session_scheduled_jobs().await,
            KillCloseReason::Replaced => self.cancel_session_rlm_heartbeats().await,
            KillCloseReason::Shutdown => Ok(()),
        };
        if let Err(error) = cancel_result {
            return response_failure(None, "kill", &error.to_string(), None);
        }
        // The close cascades to the resident children with the SAME reason before
        // the session's own archive and dispose; a close failure is swallowed.
        let child_reason = match reason {
            KillCloseReason::Killed => crate::rlm_children::ChildCloseReason::Killed,
            KillCloseReason::Shutdown => crate::rlm_children::ChildCloseReason::Shutdown,
            KillCloseReason::Replaced => crate::rlm_children::ChildCloseReason::Replaced,
        };
        if let Err(error) = self.close_rlm_children(child_reason).await {
            eprintln!("bh-daemon: RLM child close at kill failed: {error:#}");
        }
        // The `archived` entry lands before the turn's aborted row (the abort flag
        // below gates the aborted row), so the file order stays the TS one.
        // The guard rides a block, not an explicit drop: a `drop(core)` does not
        // end the guard's slot in an async generator (the awaits need Send).
        {
            let mut core = self.core.lock().unwrap();
            // `shutdown` keeps the resume entry, so its file stays
            // live on disk; the killed and replaced closes archive.
            if reason != KillCloseReason::Shutdown {
                if let Some(store) = core.store.as_mut() {
                    if let Err(error) = store.persist_entry(
                        "session_state",
                        json!({ "state": { "status": "archived" } }),
                    ) {
                        return response_failure(None, "kill", &error.to_string(), None);
                    }
                }
            }
            core.created = false;
        }
        // The abort funnel fires BEFORE every close step that can wait on the
        // session mutex the running turn holds across its provider wait: the later
        // awaits settle on an already-cancelled turn. The cancel sweep must land
        // before the close clears the lanes.
        {
            let mut core = self.core.lock().unwrap();
            core.abort_requested = true;
            core.turn_cancelled = true;
            core.steering.clear();
            core.follow_up.clear();
        }
        self.work_notify.notify_one();
        self.compaction.abort();
        self.tree_navigation.abort();
        self.engine.abort_in_flight_turn();
        // The awaited settle lets the aborted turn's row broadcast and persist (the
        // #247 gate's aborted-row exception); only then does the runtime dispose run
        // (the kernel teardown must not race a live run).
        self.await_session_work_settled().await;
        // The session-ended finalization runs AFTER the awaited settle: the
        // ended-run accounting includes the aborted turn, never a pending response.
        self.engine.archive_session_telemetry().await;
        // The session's kernel dies with the session: the worker keeps the engine
        // object, so the #235 engine-drop teardown cannot run yet — dispose explicitly.
        if let Some(agent_engine) = &self.agent_engine {
            agent_engine.dispose_kernel().await;
        }
        let active_session_id = self.core.lock().unwrap().active_session_id.clone();
        let _ = self.emit_session_closed(&active_session_id, reason.session_closed_reason());
        let _ = self.record_recovery(false, reason.recovery_operation());
        self.release_session_lease();
        // The session runtime ended (TS `beehive stop <agent>`): the
        // pane reporter releases its pane as the last write on the wire —
        // no report may reclaim it afterwards. The slot is taken out
        // first (swapped to the disabled no-op) so a later attach can
        // adopt the pane again; the taken handle's release is the
        // release of the session this kill stopped.
        let reporter = std::mem::take(&mut *self.herdr.lock().unwrap());
        reporter.release().await;
        response_success(None, "kill", None)
    }

    /// `mark_anthropic_warning_shown` (Rust-native, operator directive
    /// 2026-09-29): the interactive client reports that it just drew the
    /// Anthropic subscription ban-risk warning; the worker persists the
    /// once-per-session-lifecycle marker row (the gate a reattach, a resume,
    /// or a worker replacement reads). Idempotent — a session already marked
    /// (or an in-memory session with no file) answers success without a
    /// second row.
    fn handle_mark_anthropic_warning_shown(&self) -> DaemonResponse {
        const NAME: &str = "mark_anthropic_warning_shown";
        if let Err(response) = self.require_created(NAME) {
            return response;
        }
        let mut core = self.core.lock().unwrap();
        let Some(store) = core.store.as_mut() else {
            return response_failure(None, NAME, "Session is still initializing", None);
        };
        if store.anthropic_warning_shown() {
            return response_success(None, NAME, None);
        }
        match store.mark_anthropic_warning_shown() {
            Ok(()) => response_success(None, NAME, None),
            Err(error) => response_failure(None, NAME, &error.to_string(), None),
        }
    }

    pub(crate) fn handle_rename(&self, command: &str, payload: &Value) -> DaemonResponse {
        if let Err(response) = self.require_created(command) {
            return response;
        }
        let name = payload.get("name").and_then(Value::as_str).unwrap_or("");
        if name.trim().is_empty() {
            return response_failure(None, command, "Session name cannot be empty", None);
        }
        let (mut core, inputs) = self.summary_inputs();
        let previous = core
            .store
            .as_ref()
            .and_then(|store| store.session_name().map(str::to_string));
        if let Some(store) = core.store.as_mut() {
            if let Err(error) = store.persist_entry("session_info", json!({ "name": name })) {
                return response_failure(None, command, &error.to_string(), None);
            }
        }
        let summary = self.summary_locked(&core, inputs);
        // TS #2529 `applyStateSessionName`: a rename that changed an
        // existing name leaves the renamed session a displayed transcript
        // notice (" by parent" when the rename arrived from the parent
        // session); a first name leaves none. The notice's durable row
        // lands under the SAME core lock as the name write — worker
        // commands run concurrently, so a second lock scope here could
        // interleave another command's writes between the name and its
        // notice, ordering the notice against a contradicted name history.
        // The frames share that lock so a compaction cannot include the
        // notice before its broadcast; `session_info_changed` still comes first.
        let renamed_notice = previous
            .as_deref()
            .filter(|previous| *previous != name)
            .map(|previous| {
                let content = if payload.get("renamedBy").and_then(Value::as_str)
                    == Some(
                        bh_core::session_engine::agent_messaging::AgentFamilyRelationship::Parent
                            .as_str(),
                    ) {
                    format!("Session renamed `{previous}` -> `{name}` by parent")
                } else {
                    format!("Session renamed `{previous}` -> `{name}`")
                };
                json!({
                    "customType": bh_core::session_engine::messages::SESSION_RENAMED_CUSTOM_TYPE,
                    "content": content,
                    "display": true,
                })
            });
        if let Some(notice) = renamed_notice.as_ref() {
            persist_custom_row(&mut core, notice);
        }
        // `session_info_changed` makes every attached client re-read the name.
        super::emit_worker_event_locked(
            &mut core,
            &self.events,
            serde_json::json!({ "type": "session_info_changed", "name": name }),
        );
        if let Some(notice) = renamed_notice.as_ref() {
            super::broadcast_custom_row_locked(&mut core, &self.events, notice);
        }
        drop(core);
        // The sender identity follows the live name.
        if let Ok(summary_value) = serde_json::to_value(&summary) {
            self.engine.set_session_summary(summary_value);
        }
        response_success(
            None,
            command,
            Some(serde_json::to_value(&summary).unwrap_or(Value::Null)),
        )
    }
}

/// The bounce body: the receiver label, the fact, and the dropped body —
/// truncated so the whole notice stays inside the delivery size limit the
/// receiving `worker_deliver_message` enforces.
fn cleared_agent_message_bounce_text(receiver: &str, message: &str) -> String {
    let limit = bh_core::session_engine::agent_messaging::DEFAULT_AGENT_MESSAGE_MAX_CHARS;
    let opening = "Your agent message was cleared from ";
    let ending = "'s queue before delivery: ";
    // Session names are unbounded. Reserve the notice itself before fitting
    // the receiver label, then spend the remaining characters on the body.
    let receiver_budget = limit.saturating_sub(opening.chars().count() + ending.chars().count());
    let receiver: String = receiver.chars().take(receiver_budget).collect();
    let prefix = format!("{opening}{receiver}{ending}");
    let budget = limit.saturating_sub(prefix.chars().count());
    let body: String = message.chars().take(budget).collect();
    format!("{prefix}{body}")
}

/// Durable compaction fields that make the reviewed summary the next model context.
fn workflow_context_compaction(summary: &str, name: &str) -> Value {
    json!({
        "summary": summary,
        "firstKeptEntryId": "",
        "tokensBefore": 0,
        "details": {
            "readFiles": [],
            "modifiedFiles": [],
        },
        "fromHook": false,
        "customInstructions": format!("Context rewritten by workflow {name}"),
    })
}

#[cfg(test)]
mod workflow_context_tests {
    use super::workflow_context_compaction;

    #[test]
    fn workflow_compaction_replaces_prior_model_context_without_deleting_history() {
        let mut compaction = serde_json::json!({
            "type": "compaction",
            "id": "c1",
            "parentId": "u1",
            "timestamp": "2026-11-22T00:00:01.000Z",
        });
        compaction
            .as_object_mut()
            .unwrap()
            .extend(
                workflow_context_compaction("clean context", "repair")
                    .as_object()
                    .unwrap()
                    .clone(),
            );
        let file = format!(
            "{}\n{}\n{compaction}\n",
            serde_json::json!({
                "type": "session",
                "version": 3,
                "id": "s1",
                "timestamp": "2026-11-22T00:00:00.000Z",
                "cwd": "/tmp",
            }),
            serde_json::json!({
                "type": "message",
                "id": "u1",
                "parentId": null,
                "timestamp": "2026-11-22T00:00:00.500Z",
                "message": {
                    "role": "user",
                    "content": [{"type": "text", "text": "contaminated context"}],
                    "timestamp": 1,
                },
            }),
        );
        let entries = bh_core::session::parse_session_entries(&file);
        let context = bh_core::session::build_session_context(&entries, None);
        assert_eq!(context.messages.len(), 1);
        assert!(matches!(
            &context.messages[0],
            bh_types::session::AgentMessage::CompactionSummary(summary)
                if summary.summary == "clean context"
        ));
        assert_eq!(entries.len(), 3, "the original rows remain durable");
    }
}

/// The display-transcript page a `get_messages` payload asks for, if any.
fn transcript_page(payload: &Value) -> Option<bh_types::daemon::TranscriptPage> {
    serde_json::from_value(payload.get("page")?.clone()).ok()
}

#[cfg(test)]
mod bounce_text_tests {
    use super::cleared_agent_message_bounce_text;
    use bh_core::session_engine::agent_messaging::DEFAULT_AGENT_MESSAGE_MAX_CHARS;

    #[test]
    fn bounce_text_limits_receiver_and_body_without_losing_the_notice() {
        for (receiver, message) in [
            (
                "target".to_string(),
                "界".repeat(DEFAULT_AGENT_MESSAGE_MAX_CHARS),
            ),
            (
                "界".repeat(DEFAULT_AGENT_MESSAGE_MAX_CHARS + 1),
                "body".to_string(),
            ),
            (
                "界".repeat(DEFAULT_AGENT_MESSAGE_MAX_CHARS + 1),
                String::new(),
            ),
        ] {
            let text = cleared_agent_message_bounce_text(&receiver, &message);
            assert_eq!(text.chars().count(), DEFAULT_AGENT_MESSAGE_MAX_CHARS);
            assert!(text.starts_with("Your agent message was cleared from "));
            assert!(text.contains("'s queue before delivery: "));
        }
    }
}
