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



## Re-audit: every TUI slash command (`bh-types` registry)
Verified = opened/run in headless Chromium against an isolated daemon (`BEEHIVE_DAEMON_SOCKET`, `BEEHIVE_CODING_AGENT_DIR`); other rows were checked in code and unit tests only.

| Command | Web | Status |
| --- | --- | --- |
| `/settings` | Settings modal | verified |
| `/export` | daemon writes into export dir, browser downloads HTML/JSONL | verified (JSONL download) |
| `/import` | upload + `import_jsonl` | verified (panel opens) |
| `/share` | export HTML, `WebHost::share_gist` (needs `gh`) | verified (panel opens); gist creation needs host `gh` |
| `/copy`, `/btw`, `/session`, `/cost`, `/system-prompt`, `/context`, `/logs` | chat view | code + unit tests; `/logs` verified |
| `/name`, `/rename` | inline rename dialog / direct rename | verified |
| `/changelog`, `/hotkeys` | info panels | verified |
| `/fork`, `/clone`, `/tree` | fork picker, clone, tree | verified (open; empty session) |
| `/login`, `/logout` | provider list + flow runner (API key / OAuth link + paste) | verified (list); OAuth needs a real provider |
| `/mcp`, `/plugins` | services panel with login/token flows | verified |
| `/new`, `/clear` | new-session dialog | verified (created a session) |
| `/edit_context` | JSON editor over `get/set_session_context` (added) | verified (opens, loads) |
| `/compact`, `/refine`, `/goal` | sent as session commands | code |
| `/harness` | browser + delete via `WebHost` | verified (opens) |
| `/run` | workflows panel | verified |
| `/heartbeat`, `/heartbeats` | heartbeat panel | verified |
| `/resume` | go to sessions / resume by id or path | code |
| `/reload` | `reload` command + toast | code |
| `/quit` | [-] closing the tab; the daemon keeps running |  |

## Re-audit: keybinding actions (`definitions.rs`)
| Action | Web |
| --- | --- |
| `app.interrupt`, `app.input.clear` (Esc) | done |
| `app.clear` (Ctrl+C), `app.exit` (Ctrl+D) | [-] browser copy/close shortcuts; abort is Esc / Stop button |
| `app.suspend`, `app.editor.external` | [-] no terminal job control or `$EDITOR` in a browser |
| `app.model.select`, `cycleForward/Backward` | done (Ctrl+L, Alt+M, Shift+Alt+M) |
| `app.model.toggleScope` (Alt+S) | done (scoped checkbox + Alt+S in the model picker) |
| `app.tools.expand` (Ctrl+O), `app.subagents.focus` (Alt+A), `app.prompt.stash` (Ctrl+S) | done |
| `app.message.followUp/navigate*/move*` | done |
| `app.clipboard.pasteImage` | done (paste event); `copyLoginUrl`: link is a clickable anchor with copy button |
| `app.session.new/tree/fork/resume` | done via palette and sidebar (no default keys) |
| `app.agents.*` back/open/reply/new/delete/rename/group/searchContent | done in the sidebar (see Hotkeys panel) |
| `app.agents.program` | done: per-subagent "Spawn program" disclosure in the inspector (added) |
| `app.agents.expand` | done: sidebar caret |
| `app.modal.back/open` | Esc closes; click opens |
| `app.tree.*` fold/filters/label | done (tabs, fold carets, Label button); label timestamps toggle added |
| `tui.editor.*`, `tui.input.*`, `tui.select.*`, `tui.viewport.*` | native `<textarea>`, list focus and page scroll |

## Panels / overlays
Model picker, thinking menu, session options, tree, fork, settings, MCP/plugins, auth flow runner, harness, heartbeats, workflows, export/import/share, hotkeys, changelog, logs, context editor, rename, new session, onboarding: all present. The `app.agents` program view, scope toggle and context editor were added in this pass.

## Known limitations (report)
- Provider auth, MCP login, the harness browser and `/share` go through the `WebHost` trait (`bh-web`), implemented in `bh-cli/src/web_host.rs`. `/share` needs `gh` logged in on the host. OAuth callbacks that listen on localhost need the browser on the same machine as `beehive web`.
- Direct worker transport (TUI fast path) is not used; everything goes through the supervisor connection.
- Visual checks ran in headless Chromium at desktop and 390px widths; queue panel, retry/compaction banners, images, and the model picker modal were not screenshot-tested.