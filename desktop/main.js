// Beehive desktop: a native window around the web UI the daemon serves (`webServer` setting,
// 127.0.0.1:7878 by default). It reads the login token from <agent-dir>/web-token, so no browser
// and no token copying is needed.
const { app, BrowserWindow, shell, Menu } = require("electron");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const agentDir = process.env.BEEHIVE_CODING_AGENT_DIR || path.join(os.homedir(), ".beehive");

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch {
    return {};
  }
}

function webUrl() {
  const port = readJson(path.join(agentDir, "settings.json")).webServer?.port ?? 7878;
  let token = "";
  try {
    token = fs.readFileSync(path.join(agentDir, "web-token"), "utf8").trim();
  } catch {
    /* the daemon writes it on first start */
  }
  return { origin: `http://127.0.0.1:${port}`, url: `http://127.0.0.1:${port}/?token=${token}` };
}

const boundsFile = path.join(app.getPath("userData"), "window.json");

function waitingPage(origin) {
  const html = `<body style="margin:0;height:100vh;display:grid;place-items:center;background:#0a0a0a;color:#a1a1aa;font:14px system-ui">
    <div style="text-align:center"><div style="color:#f5c518;font-size:28px">&#x2B22;</div>
    <p>Waiting for the Beehive daemon at ${origin}&hellip;</p>
    <p style="font-size:12px;color:#71717a">Start it with <code>beehive</code> or <code>systemctl --user start beehive-daemon</code>.</p></div></body>`;
  return `data:text/html;charset=utf-8,${encodeURIComponent(html)}`;
}

function createWindow() {
  const saved = readJson(boundsFile);
  const win = new BrowserWindow({
    width: saved.width ?? 1400,
    height: saved.height ?? 900,
    x: saved.x,
    y: saved.y,
    minWidth: 480,
    minHeight: 400,
    backgroundColor: "#0a0a0a",
    title: "Beehive",
    icon: path.join(__dirname, "icon.png"),
    autoHideMenuBar: true,
    webPreferences: { contextIsolation: true, sandbox: true },
  });
  if (saved.maximized) win.maximize();

  const { origin } = webUrl();
  // Links that leave the local UI (docs, OAuth pages, gists) open in the system browser.
  win.webContents.setWindowOpenHandler(({ url }) => {
    if (!url.startsWith(origin)) void shell.openExternal(url);
    return { action: "deny" };
  });
  win.webContents.on("will-navigate", (event, url) => {
    if (!url.startsWith(origin) && !url.startsWith("data:")) {
      event.preventDefault();
      void shell.openExternal(url);
    }
  });

  // Retry until the daemon's web UI answers (the daemon may still be starting).
  let retry = null;
  const load = () => {
    const target = webUrl();
    win.loadURL(target.url).catch(() => {
      win.loadURL(waitingPage(target.origin));
      retry = setTimeout(load, 2000);
    });
  };
  load();

  win.on("close", () => {
    clearTimeout(retry);
    const bounds = win.getNormalBounds();
    fs.mkdirSync(path.dirname(boundsFile), { recursive: true });
    fs.writeFileSync(boundsFile, JSON.stringify({ ...bounds, maximized: win.isMaximized() }));
  });
}

Menu.setApplicationMenu(
  Menu.buildFromTemplate([
    { label: "Beehive", submenu: [{ role: "reload" }, { role: "forceReload" }, { role: "toggleDevTools" }, { type: "separator" }, { role: "quit" }] },
    { role: "editMenu" },
    { label: "View", submenu: [{ role: "resetZoom" }, { role: "zoomIn" }, { role: "zoomOut" }, { type: "separator" }, { role: "togglefullscreen" }] },
  ]),
);

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on("second-instance", () => {
    const [win] = BrowserWindow.getAllWindows();
    if (win) {
      if (win.isMinimized()) win.restore();
      win.focus();
    }
  });
  app.whenReady().then(createWindow);
  app.on("window-all-closed", () => app.quit());
}
