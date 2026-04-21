//! WezTerm detection and focus handler.

use anyhow::Result;
use std::collections::BTreeMap;
use std::process::Command;

use crate::workspace::process;
use crate::workspace::types::{TerminalEmulator, TerminalTab, TerminalWindow};

pub fn detect() -> Result<Option<TerminalEmulator>> {
    let pids = process::find_pids_by_name("WezTerm");
    if pids.is_empty() {
        let pids2 = process::find_pids_by_name("wezterm-gui");
        if pids2.is_empty() {
            return Ok(None);
        }
    }

    let output = Command::new("wezterm")
        .args(["cli", "list", "--format", "json"])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => {
            return Ok(Some(TerminalEmulator {
                app: "WezTerm".into(),
                pid: pids.first().copied(),
                windows: vec![],
            }));
        }
    };

    let entries: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout)?;

    let mut windows_map: BTreeMap<String, Vec<TerminalTab>> = BTreeMap::new();

    for entry in &entries {
        let window_id = entry
            .get("window_id")
            .and_then(|v| v.as_u64())
            .map(|v| v.to_string())
            .unwrap_or_default();

        let title = entry
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let cwd = entry.get("cwd").and_then(|v| v.as_str()).map(|s| {
            s.strip_prefix("file://localhost")
                .or_else(|| s.strip_prefix("file://"))
                .unwrap_or(s)
                .to_string()
        });

        let (cols, rows) = entry
            .get("size")
            .map(|s| {
                let cols = s.get("cols").and_then(|v| v.as_u64()).map(|v| v as u32);
                let rows = s.get("rows").and_then(|v| v.as_u64()).map(|v| v as u32);
                (cols, rows)
            })
            .unwrap_or((None, None));

        let tab = TerminalTab {
            title,
            uri: None,
            tty: None,
            shell_pid: entry
                .get("pane_id")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32),
            shell: None,
            cwd,
            columns: cols,
            rows,
        };

        windows_map.entry(window_id).or_default().push(tab);
    }

    let windows: Vec<TerminalWindow> = windows_map
        .into_iter()
        .map(|(id, tabs)| TerminalWindow { id, tabs })
        .collect();

    Ok(Some(TerminalEmulator {
        app: "WezTerm".into(),
        pid: pids.first().copied(),
        windows,
    }))
}

/// Focus a WezTerm tab.
pub async fn focus(tab_id: Option<&str>) -> Result<()> {
    tokio::task::spawn_blocking({
        let tab_id = tab_id.map(String::from);
        move || focus_sync(tab_id.as_deref())
    })
    .await??;
    Ok(())
}

fn focus_sync(tab_id: Option<&str>) -> Result<()> {
    if let Some(tab_id) = tab_id {
        let wezterm = find_wezterm();
        if let Some(wezterm) = wezterm {
            let _ = Command::new(&wezterm)
                .args(["cli", "activate-tab", "--tab-id", tab_id])
                .output();
        }
    }

    #[cfg(target_os = "macos")]
    {
        crate::workspace::uri::activate_app_sync("WezTerm");
    }

    Ok(())
}

fn find_wezterm() -> Option<String> {
    let paths = ["/opt/homebrew/bin/wezterm", "/usr/local/bin/wezterm"];
    for p in &paths {
        if std::path::Path::new(p).exists() {
            return Some(p.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_focus_no_panic() {
        let result = focus(None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_focus_with_tab() {
        let result = focus(Some("123")).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_find_wezterm() {
        let _ = find_wezterm();
    }
}
