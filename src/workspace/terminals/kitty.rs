//! Kitty terminal detection and focus handler.
//!
//! Uses `kitty @ ls` via the remote control socket to enumerate OS windows,
//! tabs, and windows (splits). Each kitty "window" (split/pane) maps to one
//! entry with kitty's internal window ID. Focus uses
//! `kitty @ focus-window --match id:{id}` which handles OS window, tab, and
//! split switching in a single command.

use anyhow::Result;
use std::fs;
use std::process::Command;

use crate::workspace::process;
use crate::workspace::types::{TerminalEmulator, TerminalTab, TerminalWindow};

pub fn detect() -> Result<Option<TerminalEmulator>> {
    let pids = process::find_pids_by_name("kitty");
    if pids.is_empty() {
        return Ok(None);
    }

    let mut windows = Vec::new();
    let mut connected = false;

    for socket in find_kitty_sockets(&pids) {
        let output = Command::new("kitty")
            .args(["@", "--to", &format!("unix:{}", socket), "ls"])
            .output();

        let output = match output {
            Ok(o) if o.status.success() => o,
            _ => continue,
        };

        let json: serde_json::Value = match serde_json::from_slice(&output.stdout) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let os_windows = match json.as_array() {
            Some(a) if !a.is_empty() => a,
            _ => continue,
        };

        connected = true;

        for os_win in os_windows {
            if let Some(kitty_tabs) = os_win.get("tabs").and_then(|v| v.as_array()) {
                for kt in kitty_tabs {
                    if let Some(kitty_windows) = kt.get("windows").and_then(|v| v.as_array()) {
                        for kw in kitty_windows {
                            let win_id = kw
                                .get("id")
                                .and_then(|v| v.as_u64())
                                .map(|v| v.to_string())
                                .unwrap_or_default();

                            let title = kw
                                .get("title")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();

                            let fg_pid = kw
                                .get("foreground_processes")
                                .and_then(|v| v.as_array())
                                .and_then(|a| a.first())
                                .and_then(|p| p.get("pid"))
                                .and_then(|v| v.as_u64())
                                .map(|v| v as u32);

                            let cwd = kw
                                .get("cwd")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                                .or_else(|| fg_pid.and_then(process::get_cwd));

                            let cols = kw
                                .get("columns")
                                .and_then(|v| v.as_u64())
                                .map(|v| v as u32);
                            let rows = kw
                                .get("lines")
                                .and_then(|v| v.as_u64())
                                .map(|v| v as u32);

                            let shell = kw
                                .get("foreground_processes")
                                .and_then(|v| v.as_array())
                                .and_then(|a| a.first())
                                .and_then(|p| p.get("cmdline"))
                                .and_then(|v| v.as_array())
                                .and_then(|a| a.first())
                                .and_then(|v| v.as_str())
                                .map(|s| {
                                    let name = s.rsplit('/').next().unwrap_or(s);
                                    name.strip_prefix('-')
                                        .unwrap_or(name)
                                        .to_string()
                                });

                            let tab = TerminalTab {
                                title,
                                uri: None,
                                tty: None,
                                shell_pid: fg_pid,
                                shell,
                                cwd,
                                columns: cols,
                                rows,
                            };

                            // Each kitty window (split) gets its own TerminalWindow
                            // so the URI becomes workspace://kitty/window:{kitty_id}/tab:1
                            // and focus can use the kitty window ID directly.
                            windows.push(TerminalWindow {
                                id: win_id,
                                tabs: vec![tab],
                            });
                        }
                    }
                }
            }
        }
        break;
    }

    if !connected {
        return Ok(Some(TerminalEmulator {
            app: "kitty".into(),
            pid: pids.first().copied(),
            windows: vec![],
        }));
    }

    Ok(Some(TerminalEmulator {
        app: "kitty".into(),
        pid: pids.first().copied(),
        windows,
    }))
}

/// Focus a kitty window (split) by its internal ID.
/// This single command handles OS window, tab, and split switching.
pub async fn focus(window_id: Option<&str>, _tab_id: Option<&str>) -> Result<()> {
    tokio::task::spawn_blocking({
        let window_id = window_id.map(String::from);
        move || focus_sync(window_id.as_deref())
    })
    .await??;
    Ok(())
}

fn focus_sync(window_id: Option<&str>) -> Result<()> {
    if let Some(win_id) = window_id {
        let socket = find_any_kitty_socket();
        if let Some(socket) = socket {
            let output = Command::new("kitty")
                .args([
                    "@", "--to", &format!("unix:{}", socket),
                    "focus-window", "--match", &format!("id:{}", win_id),
                ])
                .output();

            if let Ok(ref o) = output {
                if !o.status.success() {
                    crate::log::log("kitty", &format!(
                        "focus-window --match id:{} failed: {}",
                        win_id,
                        String::from_utf8_lossy(&o.stderr).trim()
                    ));
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        crate::workspace::uri::activate_app_sync("kitty");
    }

    Ok(())
}

/// Find all possible kitty socket paths.
fn find_kitty_sockets(pids: &[u32]) -> Vec<String> {
    let mut sockets = Vec::new();

    // Check KITTY_LISTEN_ON env var first
    if let Ok(listen_on) = std::env::var("KITTY_LISTEN_ON") {
        let path = listen_on.strip_prefix("unix:").unwrap_or(&listen_on);
        if std::path::Path::new(path).exists() {
            sockets.push(path.to_string());
        }
    }

    // Try pid-suffixed sockets
    for pid in pids {
        let path = format!("/tmp/kitty-sock-{}", pid);
        if std::path::Path::new(&path).exists() {
            sockets.push(path);
        }
    }

    // Scan /tmp for kitty sockets
    if let Ok(entries) = fs::read_dir("/tmp") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("kitty-sock") || name.starts_with("kitty-") {
                let path = entry.path();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::FileTypeExt;
                    if let Ok(meta) = fs::symlink_metadata(&path) {
                        if meta.file_type().is_symlink() {
                            continue;
                        }
                    }
                    if let Ok(meta) = fs::metadata(&path) {
                        if !meta.file_type().is_socket() {
                            continue;
                        }
                    }
                }
                let p = path.to_string_lossy().to_string();
                if !sockets.contains(&p) {
                    sockets.push(p);
                }
            }
        }
    }

    sockets
}

/// Find any available kitty socket for focus commands.
fn find_any_kitty_socket() -> Option<String> {
    if let Ok(listen_on) = std::env::var("KITTY_LISTEN_ON") {
        let path = listen_on.strip_prefix("unix:").unwrap_or(&listen_on);
        if std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
    }

    if let Ok(entries) = fs::read_dir("/tmp") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("kitty-sock") || name.starts_with("kitty-") {
                let path = entry.path();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::FileTypeExt;
                    if let Ok(meta) = fs::symlink_metadata(&path) {
                        if meta.file_type().is_symlink() {
                            continue;
                        }
                    }
                    if let Ok(meta) = fs::metadata(&path) {
                        if !meta.file_type().is_socket() {
                            continue;
                        }
                    }
                }
                return Some(path.to_string_lossy().to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_kitty_sockets_no_panic() {
        let _ = find_kitty_sockets(&[]);
    }

    #[test]
    fn test_find_any_kitty_socket_no_panic() {
        let _ = find_any_kitty_socket();
    }

    #[tokio::test]
    async fn test_focus_no_panic() {
        let result = focus(None, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_focus_with_window_id() {
        let result = focus(Some("42"), None).await;
        assert!(result.is_ok());
    }
}
