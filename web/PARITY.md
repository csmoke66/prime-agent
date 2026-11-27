# Web UI parity checklist

Status: `[ ]` todo, `[x]` done, `[~]` partial, `[-]` not applicable / not possible in a browser.
Owner: **L** = lead (backend, shell, wire), **A** = chat/session view (`web/src/chat/**`), **B** = agents + panels (`web/src/agents/**`, `web/src/panels/**`).

## Backend / shell (L)
- [x] `beehive web [--port N] [--open]`, 127.0.0.1 default, per-launch token in URL, cookie + Origin/Host checks
- [x] Daemon bridge: one WebSocket <-> one daemon socket connection (verbatim JSONL frames)
- [x] Ensure daemon running (same as TUI start)
- [x] Embedded static assets, SPA fallback
- [~] Settings read/write endpoint (`settings.json`), keybinding/theme info endpoint (settings + keybindings overrides; no theme endpoint, palette is the fixed Beehive theme)
- [x] Adoption telemetry event `web_started`
- [x] Connection status + automatic reconnect + session re-attach
- [x] Command palette (Ctrl/Cmd+K), global shortcuts, responsive layout

## Session list / sidebar (B)
- [x] One persistent, collapsible, resizable left sidebar replaces the Sessions and Agents views: live + saved (inactive, dimmed) agents in TUI order (custom groups, then Running/Idle/Inactive), nested subagent tree, status glyphs, goal/queue/subagent badges, filter (+ message-text search), new/import, rename, group, stop/delete, persistent shells
- [x] Saved sessions list with streaming progress (`list_saved_sessions`), search, content search (`search_session_content`)
- [x] Active sessions list (`list`), status badges
- [x] New session (cwd, name, initial prompt, model) (`create`/`new_session`)
- [x] Resume/attach saved session, rename, delete (`rename_saved_session`, `delete_saved_session`)
- [x] Session tree navigation / branches (`/tree`, `get_session_tree`, `navigate_tree`)
- [x] Fork from a user message, clone (`/fork`, `/clone`, `get_user_messages_for_forking`, `fork`)
- [x] Labels on session entries (`set_session_entry_label`)
- [x] Import JSONL (`/import`)

## Chat (A)
- [~] Attach (snapshot begin/chunk/end), resume cursor, replay, reattach after reconnect
- [x] Transcript: user, assistant (markdown, code highlighting, tables, links), custom messages, skill blocks
- [x] Streaming assistant text, smooth append, auto-scroll with "jump to latest"
- [x] Thinking blocks (collapsible, level colour)
- [x] Tool calls + results: bash, edit (diffs), python_repl (cells/output), generic; expand/collapse all
- [x] Images in transcript and attachments (paste, drop, file picker)
- [x] Composer: multi-line, submit, steer, follow-up, queue view (edit/reorder/clear/restore), prompt history, stash
- [~] Autocomplete: slash commands, `@path` tokens, skills
- [x] Abort, abort-and-send-queued, retry/abort-retry
- [x] Bash mode (`!cmd`, `!!cmd`), `execute_bash`, `abort_bash`
- [~] Model picker + cycle, thinking level + cycle, service tier, scoped models, transport, steering/follow-up modes
- [x] Status bar: model, thinking, context usage %, cost, tokens, goal, queue length
- [x] Compaction row/progress, `/compact [instructions]`, abort compaction, auto-compaction toggle
- [x] Side question `/btw` pane
- [x] Copy last message (`/copy`), `/session`, `/cost [reset]`, `/context` usage tree, `/system-prompt`
- [x] Goal surface: `/goal`, pause/resume/clear, budget
- [x] Incident / error notices, retry banners

## Agents view + panels (B)
- [x] Agents (sidebar): roster tree (parent/child), groups, status, activity, search, delete, rename
- [x] Right inspector (Ctrl+I; drawer on narrow screens): context meter, goal, queue, subagents, agent-message controls, kernel bash, reply box
- [x] Live roster updates (`roster_subscribe`, `roster_update`)
- [~] Agent messages: inbox, send/reply (`send_message`, `agent_messages_*`), pause/resume/clear
- [x] Cancel / delete rlm child (`cancel_rlm_child`, `delete_rlm_subagent`)
- [x] Persistent shells + kernel bash list/tail/kill
- [x] Factory activity / workflows (`/run`, `list_workflows`, `get_workflow_run`, factory view)
- [x] Heartbeats: `/heartbeat`, `/heartbeats` manager, cron list/add/cancel
- [x] Settings menu (tabs: model policy, prompt segments, compaction, retry, etc.)
- [x] MCP connections (`/mcp`, `/plugins`), static tokens, remove
- [x] Harness browser (`/harness`), `/refine`
- [x] Auth: `/login`, `/logout` (API key, OAuth via sign-in link + paste, through the `WebHost` seam)
- [~] `/hotkeys`, `/changelog`, `/logs`, `/reload`, `/name`, `/export` (HTML/JSONL download), `/share`
- [~] Onboarding view when no provider is configured
- [~] Keyboard shortcuts mirroring TUI app bindings where the browser permits

## Not applicable in a browser
- [-] Alt-screen, mouse tracking, OSC52/OSC133, terminal image protocols, suspend (Ctrl+Z), external editor, terminal selection, `/quit`


## Known limitations (report)
- Provider auth, MCP login, the harness browser and `/share` go through the `WebHost` trait (`bh-web`), implemented in `bh-cli/src/web_host.rs`. `/share` needs `gh` logged in on the host. OAuth callbacks that listen on localhost need the browser on the same machine as `beehive web`.
- Direct worker transport (TUI fast path) is not used; everything goes through the supervisor connection.
- Visual checks ran in headless Chromium at desktop and 390px widths; queue panel, retry/compaction banners, images, and the model picker modal were not screenshot-tested.