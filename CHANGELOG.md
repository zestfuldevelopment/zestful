# Changelog

All notable changes to the Zestful CLI will be documented in this file.

## [0.1.1] - 2026-03-19

### Security
- Fix command injection in osascript: AppleScript string escaping and input validation on app/window_id/tab_id (reject shell metacharacters)
- Add token authentication (`X-Zestful-Token`) to daemon `/focus` endpoint
- Prevent PID file symlink attacks: refuse to write if path is a symlink
- Validate kitty socket discovery: reject symlinks, confirm file is a Unix socket
- Guard `kill(pid, 0)` against pid <= 0 to prevent signaling process group 0

### Changed
- Updated README with Rust build instructions, architecture section, and daemon docs
- Added module-level and public API doc comments across all Rust source files

### Removed
- Old bash CLI script and Node.js daemon (replaced in 0.1.0)

## [0.1.0] - 2026-03-19

### Added
- Port CLI and focus daemon from bash/Node.js to Rust — single static binary replaces both
- `zestful notify` — send notifications to the Zestful macOS app via sync HTTP (ureq)
- `zestful watch` — wrap a command and notify on completion with exit-code-based severity
- `zestful ssh` — sync config to remote host and `exec ssh -R` for port forwarding
- `zestful daemon` — axum server on localhost:21548 with `/health` and `/focus` endpoints
- Native iTerm2 tab switching via `iterm2-client` crate (replaces Python iterm2 module)
- Focus handlers for kitty, WezTerm, Terminal.app, and generic apps (osascript)
- Auto-start daemon from CLI commands via PID file check
- 68 unit tests

### Removed
- Node.js dependency (zestfuld.js daemon)
- Python dependency (iTerm2 tab switching)
