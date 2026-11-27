# Web UI parity checklist

Status: `[ ]` todo, `[x]` done, `[~]` partial, `[-]` not applicable / not possible in a browser.
Owner: **L** = lead (backend, shell, wire), **A** = chat/session view (`web/src/chat/**`), **B** = agents + panels (`web/src/agents/**`, `web/src/panels/**`).

## Backend / shell (L)
- [ ] `beehive web [--port N] [--open]`, 127.0.0.1 default, per-launch token in URL, cookie + Origin/Host checks
- [ ] Daemon bridge: one WebSocket <-> one daemon socket connection (verbatim JSONL frames)
- [ ] Ensure daemon running (same as TUI start)
- [ ] Embedded static assets, SPA fallback
- [ ] Settings read/write endpoint (`settings.json`), keybinding/theme info endpoint
- [ ] Adoption telemetry event `web_started`
- [ ] Connection status + automatic reconnect + session re-attach
- [ ] Command palette (Ctrl/Cmd+K), global shortcuts, responsive layout

## Session list / picker (B)
- [ ] Saved sessions list with streaming progress (`list_saved_sessions`), search, content search (`search_session_content`)
- [ ] Active sessions list (`list`), status badges
- [ ] New session (cwd, name, initial prompt, model) (`create`/`new_session`)
- [ ] Resume/attach saved session, rename, delete (`rename_saved_session`, `delete_saved_session`)
- [ ] Session tree navigation / branches (`/tree`, `get_session_tree`, `navigate_tree`)
- [ ] Fork from a user message, clone (`/fork`, `/clone`, `get_user_messages_for_forking`, `fork`)
- [ ] Labels on session entries (`set_session_entry_label`)
- [ ] Import JSONL (`/import`)

## Chat (A)
- [ ] Attach (snapshot begin/chunk/end), resume cursor, replay, reattach after reconnect
- [ ] Transcript: user, assistant (markdown, code highlighting, tables, links), custom messages, skill blocks
- [ ] Streaming assistant text, smooth append, auto-scroll with "jump to latest"
- [ ] Thinking blocks (collapsible, level colour)
- [ ] Tool calls + results: bash, edit (diffs), python_repl (cells/output), generic; expand/collapse all
- [ ] Images in transcript and attachments (paste, drop, file picker)
- [ ] Composer: multi-line, submit, steer, follow-up, queue view (edit/reorder/clear/restore), prompt history, stash
- [ ] Autocomplete: slash commands, `@path` tokens, skills
- [ ] Abort, abort-and-send-queued, retry/abort-retry
- [ ] Bash mode (`!cmd`, `!!cmd`), `execute_bash`, `abort_bash`
- [ ] Model picker + cycle, thinking level + cycle, service tier, scoped models, transport, steering/follow-up modes
- [ ] Status bar: model, thinking, context usage %, cost, tokens, goal, queue length
- [ ] Compaction row/progress, `/compact [instructions]`, abort compaction, auto-compaction toggle
- [ ] Side question `/btw` pane
- [ ] Copy last message (`/copy`), `/session`, `/cost [reset]`, `/context` usage tree, `/system-prompt`
- [ ] Goal surface: `/goal`, pause/resume/clear, budget
- [ ] Incident / error notices, retry banners

## Agents view + panels (B)
- [ ] Agents view: roster tree (parent/child), groups, status, activity, search, delete, rename
- [ ] Live roster updates (`roster_subscribe`, `roster_update`)
- [ ] Agent messages: inbox, send/reply (`send_message`, `agent_messages_*`), pause/resume/clear
- [ ] Cancel / delete rlm child (`cancel_rlm_child`, `delete_rlm_subagent`)
- [ ] Persistent shells + kernel bash list/tail/kill
- [ ] Factory activity / workflows (`/run`, `list_workflows`, `get_workflow_run`, factory view)
- [ ] Heartbeats: `/heartbeat`, `/heartbeats` manager, cron list/add/cancel
- [ ] Settings menu (tabs: model policy, prompt segments, compaction, retry, etc.)
- [ ] MCP connections (`/mcp`, `/plugins`), static tokens, remove
- [ ] Harness browser (`/harness`), `/refine`
- [ ] Auth: `/login`, `/logout` (API-key entry; OAuth flows that need a local browser reported as limited)
- [ ] `/hotkeys`, `/changelog`, `/logs`, `/reload`, `/name`, `/export` (HTML/JSONL download), `/share`
- [ ] Onboarding view when no provider is configured
- [ ] Keyboard shortcuts mirroring TUI app bindings where the browser permits

## Not applicable in a browser
- [-] Alt-screen, mouse tracking, OSC52/OSC133, terminal image protocols, suspend (Ctrl+Z), external editor, terminal selection, `/quit`
