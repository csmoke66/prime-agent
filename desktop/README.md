# Beehive desktop

A native window around the web UI that the Beehive daemon serves (the `webServer`
setting; `127.0.0.1:7878` by default). It reads the login token from
`~/.beehive/web-token`, retries until the daemon answers, remembers its window size,
and opens external links in the system browser.

Install for the current user (needs node and npm):

```sh
make desktop
```

This installs `beehive-desktop` in `~/.local/bin` and a "Beehive" entry in the
application menu.
