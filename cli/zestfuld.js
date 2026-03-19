#!/usr/bin/env node
/**
 * zestfuld — Focus daemon for Zestful. Runs outside the app sandbox.
 * Listens on localhost:21548 for focus commands from the sandboxed app.
 */

const http = require("http");
const { execSync, spawn } = require("child_process");
const fs = require("fs");
const path = require("path");
const os = require("os");

const PORT = 21548;
const CONFIG_DIR = path.join(os.homedir(), ".config", "zestful");
const PID_FILE = path.join(CONFIG_DIR, "zestfuld.pid");

// --- Focus handlers ---

function focusITerm2(tabId) {
  // Use the python iterm2 module for tab switching
  if (tabId) {
    const python3 = fs.existsSync("/opt/homebrew/bin/python3")
      ? "/opt/homebrew/bin/python3"
      : "python3";

    const scriptFile = path.join(CONFIG_DIR, ".iterm2-focus-tmp.py");
    const script = [
      "import iterm2",
      "",
      "async def main(connection):",
      `    target = '${tabId}'`,
      "    app = await iterm2.async_get_app(connection)",
      "    for window in app.windows:",
      "        for tab in window.tabs:",
      "            title_override = await tab.async_get_variable('titleOverride')",
      "            session_name = await tab.current_session.async_get_variable('name')",
      "            if target == title_override or target == session_name:",
      "                await tab.async_select()",
      "                await window.async_activate()",
      "                return",
      "",
      "iterm2.run_until_complete(main)",
    ].join("\n");

    fs.writeFileSync(scriptFile, script);
    try {
      execSync(`${python3} ${scriptFile}`, {
        timeout: 5000,
        stdio: "pipe",
      });
    } catch (e) {
      console.error(`[zestfuld] iTerm2 python error: ${e.stderr?.toString().trim() || e.message}`);
    }
  }
  // Always bring iTerm2 to front
  try {
    execSync('osascript -e \'tell application "iTerm2" to activate\'', { stdio: "pipe" });
  } catch {}
}

function focusKitty(windowId, tabId) {
  const kitten = fs.existsSync("/opt/homebrew/bin/kitten")
    ? "/opt/homebrew/bin/kitten"
    : "/usr/local/bin/kitten";

  // Find kitty socket
  let socket = null;
  try {
    const files = fs.readdirSync("/tmp");
    const sock = files.find((f) => f.startsWith("kitty-sock"));
    if (sock) socket = `/tmp/${sock}`;
  } catch {}

  if (socket) {
    try {
      if (tabId) {
        execSync(`${kitten} @ --to unix:${socket} focus-tab --match id:${tabId}`, { stdio: "pipe" });
      } else if (windowId) {
        execSync(`${kitten} @ --to unix:${socket} focus-window --match id:${windowId}`, { stdio: "pipe" });
      }
    } catch (e) {
      console.error(`[zestfuld] Kitty focus error: ${e.message}`);
    }
  }
  try {
    execSync('osascript -e \'tell application "kitty" to activate\'', { stdio: "pipe" });
  } catch {}
}

function focusWezTerm(tabId) {
  const wezterm = fs.existsSync("/opt/homebrew/bin/wezterm")
    ? "/opt/homebrew/bin/wezterm"
    : "/usr/local/bin/wezterm";
  if (tabId) {
    try {
      execSync(`${wezterm} cli activate-tab --tab-id ${tabId}`, { stdio: "pipe" });
    } catch {}
  }
  try {
    execSync('osascript -e \'tell application "WezTerm" to activate\'', { stdio: "pipe" });
  } catch {}
}

function focusTerminal(tabId) {
  const script = tabId
    ? `tell application "Terminal"
         activate
         set target_tab to "${tabId}"
         repeat with w in windows
           repeat with t in tabs of w
             if tty of t contains target_tab then
               set selected tab of w to t
               set index of w to 1
               return
             end if
           end repeat
         end repeat
       end tell`
    : 'tell application "Terminal" to activate';
  try {
    execSync(`osascript -e '${script.replace(/'/g, "'\\''")}'`, { stdio: "pipe" });
  } catch {}
}

function focusGeneric(app) {
  try {
    execSync(`osascript -e 'tell application "${app}" to activate'`, { stdio: "pipe" });
  } catch {}
}

function handleFocus(app, windowId, tabId) {
  const lower = app.toLowerCase();
  if (lower.includes("kitty")) {
    focusKitty(windowId, tabId);
  } else if (lower.includes("iterm")) {
    focusITerm2(tabId);
  } else if (lower.includes("wezterm")) {
    focusWezTerm(tabId);
  } else if (lower.includes("terminal")) {
    focusTerminal(tabId);
  } else {
    focusGeneric(app);
  }
}

// --- Server ---

const server = http.createServer((req, res) => {
  if (req.method === "GET" && req.url === "/health") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end('{"status":"ok"}');
    return;
  }

  if (req.method === "POST" && req.url === "/focus") {
    let body = "";
    req.on("data", (chunk) => (body += chunk));
    req.on("end", () => {
      try {
        const data = JSON.parse(body);
        const { app, window_id, tab_id } = data;
        if (!app) {
          res.writeHead(400, { "Content-Type": "application/json" });
          res.end('{"error":"app is required"}');
          return;
        }
        console.log(`[zestfuld] Focus: app=${app} window_id=${window_id || ""} tab_id=${tab_id || ""}`);
        handleFocus(app, window_id, tab_id);
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end('{"status":"ok"}');
      } catch (e) {
        res.writeHead(400, { "Content-Type": "application/json" });
        res.end('{"error":"invalid json"}');
      }
    });
    return;
  }

  res.writeHead(404);
  res.end();
});

// Write PID file
fs.mkdirSync(CONFIG_DIR, { recursive: true });
fs.writeFileSync(PID_FILE, String(process.pid));

process.on("SIGTERM", () => {
  try { fs.unlinkSync(PID_FILE); } catch {}
  process.exit(0);
});
process.on("SIGINT", () => {
  try { fs.unlinkSync(PID_FILE); } catch {}
  process.exit(0);
});

server.listen(PORT, "127.0.0.1", () => {
  console.log(`[zestfuld] Listening on localhost:${PORT}`);
});

