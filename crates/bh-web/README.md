# bh-web

HTTP/WebSocket backend of `beehive web`: serves the embedded web UI (`web/`, built into the committed `assets/` bundle by `make web`) and bridges each browser
WebSocket to one daemon socket connection, forwarding the `beehive.daemon` JSONL frames verbatim. It is a
daemon-wire client like `bh-tui`.

- **Scope:** per-launch token auth (cookie, `Host`/`Origin` checks), static asset serving, the daemon
  bridge, and a few REST helpers for what a browser cannot do itself (settings file, path completion,
  uploads, export downloads).
- **Non-goals:** it never links the session engine (`bh-core`), never starts the daemon (the CLI does),
  and does not interpret session payloads.
- **Public API:** `WebOptions`, `router`, `serve`.
- **Dependencies:** `bh-types` only (workspace).
