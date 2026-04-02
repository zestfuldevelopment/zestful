//! Kitty terminal detection and focus handler.

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

    let mut all_windows = Vec::new();

    for pid in &pids {
        let sock = format!("/tmp/kitty-sock-{}", pid);
        let output = Command::new("kitty")
            .args(["@", "--to", &format!("unix:{}", sock), "ls"])
            .output();

        let output = match output {
            Ok(o) if o.status.success() => o,
            _ => continue,
        };

        let json: serde_json::Value =
            match serde_json::from_slice(&output.stdout) {
                Ok(v) => v,
                Err(_) => continue,
            };

        if let Some(os_windows) = json.as_array() {
            for os_win in os_windows {
                let win_id = os_win
                    .get("id")
                    .and_then(|v| v.as_u64())
                    .map(|v| v.to_string())
                    .unwrap_or_default();

                let mut tabs = Vec::new();
                if let Some(kitty_tabs) = os_win.get("tabs").and_then(|v| v.as_array()) {
                    for kt in kitty_tabs {
                        let title = kt
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        if let Some(kitty_windows) =
                            kt.get("windows").and_then(|v| v.as_array())
                        {
                            for kw in kitty_windows {
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

                                let cmd = kw
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

                                tabs.push(TerminalTab {
                                    title: title.clone(),
                                    uri: None,
                                    tty: None,
                                    shell_pid: fg_pid,
                                    shell: cmd,
                                    cwd,
                                    columns: cols,
                                    rows,
                                });
                            }
                        }
                    }
                }

                all_windows.push(TerminalWindow {
                    id: win_id,
                    tabs,
                });
            }
        }
    }

    Ok(Some(TerminalEmulator {
        app: "kitty".into(),
        pid: pids.first().copied(),
        windows: all_windows,
    }))
}

/// Focus a kitty terminal tab or window.
pub async fn focus(window_id: Option<&str>, tab_id: Option<&str>) -> Result<()> {
    tokio::task::spawn_blocking({
        let window_id = window_id.map(String::from);
        let tab_id = tab_id.map(String::from);
        move || focus_sync(window_id.as_deref(), tab_id.as_deref())
    })
    .await??;
    Ok(())
}

fn focus_sync(window_id: Option<&str>, tab_id: Option<&str>) -> Result<()> {
    let kitten = find_kitten();
    let socket = find_kitty_socket();

    if let (Some(kitten), Some(socket)) = (&kitten, &socket) {
        if let Some(tab_id) = tab_id {
            let _ = Command::new(kitten)
                .args(["@", "--to", &format!("unix:{}", socket), "focus-tab", "--match", &format!("id:{}", tab_id)])
                .output();
        } else if let Some(window_id) = window_id {
            let _ = Command::new(kitten)
                .args(["@", "--to", &format!("unix:{}", socket), "focus-window", "--match", &format!("id:{}", window_id)])
                .output();
        }
    }

    #[cfg(target_os = "macos")]
    {
        crate::workspace::uri::activate_app_sync("kitty");
    }

    Ok(())
}

fn find_kitten() -> Option<String> {
    let paths = ["/opt/homebrew/bin/kitten", "/usr/local/bin/kitten"];
    for p in &paths {
        if std::path::Path::new(p).exists() {
            return Some(p.to_string());
        }
    }
    which("kitten")
}

fn find_kitty_socket() -> Option<String> {
    if let Ok(entries) = fs::read_dir("/tmp") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("kitty-sock") {
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

fn which(name: &str) -> Option<String> {
    Command::new("which")
        .arg(name)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_kitty_socket_no_panic() {
        let _ = find_kitty_socket();
    }

    #[test]
    fn test_find_kitten_no_panic() {
        let _ = find_kitten();
    }

    #[tokio::test]
    async fn test_focus_no_panic() {
        let result = focus(None, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_focus_with_tab_id() {
        let result = focus(None, Some("123")).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_focus_with_window_id() {
        let result = focus(Some("42"), None).await;
        assert!(result.is_ok());
    }
}
