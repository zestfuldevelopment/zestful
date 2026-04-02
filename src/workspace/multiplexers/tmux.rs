//! tmux session detection.

use anyhow::Result;
use std::process::Command;

use crate::workspace::types::{TmuxPane, TmuxSession, TmuxWindow};

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
        .args(["list-sessions", "-F", "#{session_name}\t#{session_id}\t#{session_attached}"])
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
