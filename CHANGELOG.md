# Changelog

All notable changes to the Zestful CLI will be documented in this file.

## [3.1.0] - 2026-04-01

### Added
- `zestful inspect` — inspect running terminals, multiplexers, IDEs, and browsers
  - Subcommands: `terminals`, `tmux`, `shelldon`, `zellij`, `ides`, `browsers`, `where`, `all`
  - `--pretty` flag for human-readable output, JSON by default
  - `zestful inspect where` prints the `workspace://` URI for the current terminal location
- `zestful focus` — focus a terminal tab directly from the CLI
  - Accepts a `workspace://` URI positional arg or `--app`/`--window-id`/`--tab-id` flags
  - Same focus logic as the daemon, no HTTP round-trip
  - Handles shelldon multiplexer tabs embedded in URIs
- Built-in workspace inspector — terminal/multiplexer/IDE/browser detection is now part of the zestful binary
  - Detects: iTerm2, kitty, WezTerm, Terminal.app, Alacritty, Ghostty, GNOME Terminal, Command Prompt, PowerShell
  - Multiplexers: tmux, zellij, shelldon
  - IDEs: Xcode
  - Browsers: Google Chrome
  - Focus handlers merged with detection — detect and focus code for each terminal lives in one module

### Changed
- Terminal detection and focus code merged into `src/workspace/` module tree
- Focus dispatch moved from `src/focus/` to `src/workspace/terminals/`
- URI parsing moved to `src/workspace/uri.rs`

### Removed
- `workspace-inspector` external crate dependency — all detection code is now built-in
- `src/focus/` directory — replaced by `src/workspace/terminals/` (merged detect+focus)

## [3.0.0] - 2026-03-19

Complete rewrite from bash/Node.js to Rust. A single static binary replaces the
bash CLI script, Node.js focus daemon, and Python iTerm2 dependency.

### Added
- `zestful notify` — send notifications to the Zestful macOS app via sync HTTP (ureq)
- `zestful watch` — wrap a command and notify on completion with exit-code-based severity
- `zestful ssh` — sync config to remote host and `exec ssh -R` for port forwarding
- `zestful daemon` — axum server on localhost:21548 with `/health` and `/focus` endpoints
- Native iTerm2 tab switching via `iterm2-client` crate (no Python dependency)
- Focus handlers for kitty, WezTerm, Terminal.app, and generic apps (osascript)
- Auto-start daemon from CLI commands via PID file check
- 68 unit tests

### Security
- Input validation on all focus IDs (app, window_id, tab_id) — rejects shell metacharacters
- AppleScript string escaping prevents command injection via osascript
- Token authentication (`X-Zestful-Token`) required on daemon `/focus` endpoint
- 16KB request body size limit on daemon prevents memory exhaustion
- `--severity` validated against `info`/`warning`/`urgent` at parse time
- PID file symlink check prevents arbitrary file overwrite
- Kitty socket validated as real Unix socket, not a symlink
- Config directory created with mode 0700
- Error messages redacted to prevent token leakage
- `kill(pid, 0)` guarded against pid <= 0

### Removed
- Node.js dependency (zestfuld.js daemon)
- Python dependency (iTerm2 tab switching)
- Bash CLI script

### Breaking Changes
- Requires Rust toolchain to build from source (or use pre-built binary)
- Binary replaces both `zestful` (bash) and `zestfuld.js` (Node.js daemon)
