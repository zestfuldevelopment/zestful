//! Zellij session detection.

use anyhow::Result;
use std::process::Command;

use crate::workspace::types::{ZellijPane, ZellijSession, ZellijTab};

pub fn detect() -> Result<Vec<ZellijSession>> {
    let has_zellij = Command::new("which")
        .arg("zellij")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !has_zellij {
        return Ok(vec![]);
    }

    let output = Command::new("zellij")
        .args(["list-sessions", "--short", "--no-formatting"])
        .output()?;

    if !output.status.success() {
        return Ok(vec![]);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut sessions = Vec::new();

    for line in stdout.lines() {
        let session_name = line.trim().to_string();
        if session_name.is_empty() {
            continue;
        }

        let tabs = list_tabs(&session_name)?;
        let panes = list_panes(&session_name)?;

        let mut tab_list: Vec<ZellijTab> = tabs;
        for tab in &mut tab_list {
            tab.panes = panes
                .iter()
                .filter(|p| p.tab_id == tab.id)
                .cloned()
                .collect();
        }

        sessions.push(ZellijSession {
            name: session_name,
            tabs: tab_list,
        });
    }

    Ok(sessions)
}

fn list_tabs(session: &str) -> Result<Vec<ZellijTab>> {
    let output = Command::new("zellij")
        .args(["--session", session, "action", "list-tabs", "--json"])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return Ok(vec![]),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);

    let raw: Vec<serde_json::Value> = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => return Ok(vec![]),
    };

    let tabs = raw
        .iter()
        .map(|t| {
            let id = t
                .get("TAB_ID")
                .or_else(|| t.get("tab_id"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let position = t
                .get("POSITION")
                .or_else(|| t.get("position"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let name = t
                .get("NAME")
                .or_else(|| t.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let active = t
                .get("ACTIVE")
                .or_else(|| t.get("active"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            ZellijTab {
                id,
                position,
                name,
                active,
                panes: vec![],
            }
        })
        .collect();

    Ok(tabs)
}

fn list_panes(session: &str) -> Result<Vec<ZellijPane>> {
    let output = Command::new("zellij")
        .args([
            "--session",
            session,
            "action",
            "list-panes",
            "--json",
            "--all",
        ])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return Ok(vec![]),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);

    let raw: Vec<serde_json::Value> = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => return Ok(vec![]),
    };

    let panes = raw
        .iter()
        .map(|p| {
            let tab_id = p
                .get("TAB_ID")
                .or_else(|| p.get("tab_id"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let pane_id = p
                .get("PANE_ID")
                .or_else(|| p.get("pane_id"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let title = p
                .get("TITLE")
                .or_else(|| p.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let command = p
                .get("COMMAND")
                .or_else(|| p.get("command"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let cwd = p
                .get("CWD")
                .or_else(|| p.get("cwd"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let cols = p
                .get("COLS")
                .or_else(|| p.get("cols"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let rows = p
                .get("ROWS")
                .or_else(|| p.get("rows"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let focused = p
                .get("FOCUSED")
                .or_else(|| p.get("focused"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            ZellijPane {
                tab_id,
                pane_id,
                uri: None,
                title,
                command,
                cwd,
                columns: cols,
                rows,
                focused,
            }
        })
        .collect();

    Ok(panes)
}
