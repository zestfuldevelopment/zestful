//! tmux session detection and focus.

use anyhow::Result;
use std::process::Command;

use crate::workspace::types::{TmuxPane, TmuxSession, TmuxWindow};
use crate::workspace::uri::{self, TmuxInfo};

pub fn detect() -> Result<Vec<TmuxSession>> {
    let has_tmux = Command::new("which")
        .arg("tmux")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !has_tmux {
        return Ok(vec![]);
    }

    let output = Command::new("tmux")
        .args([
            "list-sessions",
            "-F",
            "#{session_name}\t#{session_id}\t#{session_attached}",
        ])
        .output()?;

    if !output.status.success() {
        return Ok(vec![]);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut sessions = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }

        let session_name = parts[0].to_string();
        let session_id = parts[1].to_string();
        let attached = parts[2] != "0";

        let windows = list_windows(&session_name)?;

        sessions.push(TmuxSession {
            name: session_name,
            id: session_id,
            attached,
            windows,
        });
    }

    Ok(sessions)
}

fn list_windows(session: &str) -> Result<Vec<TmuxWindow>> {
    let output = Command::new("tmux")
        .args([
            "list-windows",
            "-t",
            session,
            "-F",
            "#{window_index}\t#{window_name}\t#{window_active}",
        ])
        .output()?;

    if !output.status.success() {
        return Ok(vec![]);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut windows = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }

        let index: u32 = parts[0].parse().unwrap_or(0);
        let name = parts[1].to_string();
        let active = parts[2] == "1";

        let target = format!("{}:{}", session, index);
        let panes = list_panes(&target)?;

        windows.push(TmuxWindow {
            index,
            name,
            active,
            panes,
        });
    }

    Ok(windows)
}

fn list_panes(target: &str) -> Result<Vec<TmuxPane>> {
    let output = Command::new("tmux")
        .args([
            "list-panes",
            "-t",
            target,
            "-F",
            "#{pane_index}\t#{pane_pid}\t#{pane_current_command}\t#{pane_current_path}\t#{pane_width}\t#{pane_height}\t#{pane_active}",
        ])
        .output()?;

    if !output.status.success() {
        return Ok(vec![]);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut panes = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 7 {
            continue;
        }

        panes.push(TmuxPane {
            uri: None,
            index: parts[0].parse().unwrap_or(0),
            pid: parts[1].parse().unwrap_or(0),
            command: parts[2].to_string(),
            cwd: parts[3].to_string(),
            width: parts[4].parse().unwrap_or(0),
            height: parts[5].parse().unwrap_or(0),
            active: parts[6] == "1",
        });
    }

    Ok(panes)
}

/// Focus a tmux window and/or pane by session name.
pub async fn focus(info: &TmuxInfo) -> Result<()> {
    uri::validate_focus_id(&info.session, "tmux session")?;
    if let Some(ref w) = info.window {
        uri::validate_focus_id(w, "tmux window")?;
    }
    if let Some(ref p) = info.pane {
        uri::validate_focus_id(p, "tmux pane")?;
    }

    let session = info.session.clone();
    let window = info.window.clone();
    let pane = info.pane.clone();

    tokio::task::spawn_blocking(move || focus_sync(&session, window.as_deref(), pane.as_deref()))
        .await??;

    Ok(())
}

fn focus_sync(session: &str, window: Option<&str>, pane: Option<&str>) -> Result<()> {
    if let Some(win) = window {
        let target = format!("{}:{}", session, win);
        let output = Command::new("tmux")
            .args(["select-window", "-t", &target])
            .output();
        if let Ok(ref o) = output {
            if !o.status.success() {
                crate::log::log(
                    "tmux",
                    &format!(
                        "select-window -t {} failed: {}",
                        target,
                        String::from_utf8_lossy(&o.stderr).trim()
                    ),
                );
            }
        }

        if let Some(pane_idx) = pane {
            let pane_target = format!("{}.{}", target, pane_idx);
            let output = Command::new("tmux")
                .args(["select-pane", "-t", &pane_target])
                .output();
            if let Ok(ref o) = output {
                if !o.status.success() {
                    crate::log::log(
                        "tmux",
                        &format!(
                            "select-pane -t {} failed: {}",
                            pane_target,
                            String::from_utf8_lossy(&o.stderr).trim()
                        ),
                    );
                }
            }
        }
    }

    crate::log::log(
        "tmux",
        &format!(
            "focus session={} window={} pane={}",
            session,
            window.unwrap_or(""),
            pane.unwrap_or("")
        ),
    );

    Ok(())
}

#[cfg(test)]
mod focus_tests {
    use super::*;

    #[tokio::test]
    async fn test_focus_no_window() {
        // No window means nothing to select — should succeed silently
        let info = TmuxInfo {
            session: "main".into(),
            window: None,
            pane: None,
        };
        let result = focus(&info).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_focus_rejects_invalid_session() {
        let info = TmuxInfo {
            session: "bad\"session".into(),
            window: Some("0".into()),
            pane: None,
        };
        let result = focus(&info).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_focus_rejects_invalid_window() {
        let info = TmuxInfo {
            session: "main".into(),
            window: Some("$(whoami)".into()),
            pane: None,
        };
        let result = focus(&info).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_focus_with_window_and_pane() {
        // tmux may not be running — should not panic
        let info = TmuxInfo {
            session: "main".into(),
            window: Some("0".into()),
            pane: Some("0".into()),
        };
        let result = focus(&info).await;
        assert!(result.is_ok());
    }
}
