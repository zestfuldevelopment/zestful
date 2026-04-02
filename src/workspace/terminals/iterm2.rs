//! iTerm2 detection and focus handler (macOS only).

use anyhow::Result;
use std::process::Command;

use crate::workspace::process;
use crate::workspace::types::{TerminalEmulator, TerminalTab, TerminalWindow};

const APPLESCRIPT: &str = r#"
tell application "iTerm2"
    set output to ""
    set winList to every window
    repeat with w in winList
        try
            set wid to id of w
            set tabIdx to 0
            set tabList to every tab of w
            repeat with t in tabList
                try
                    set tabIdx to tabIdx + 1
                    set sessionList to every session of t
                    repeat with s in sessionList
                        try
                            set ttyVal to tty of s
                            set titleVal to name of s
                            set colsVal to columns of s
                            set rowsVal to rows of s
                            set output to output & wid & "\t" & tabIdx & "\t" & titleVal & "\t" & ttyVal & "\t" & colsVal & "\t" & rowsVal & "\n"
                        end try
                    end repeat
                end try
            end repeat
        end try
    end repeat
    return output
end tell
"#;

pub fn detect() -> Result<Option<TerminalEmulator>> {
    let pids = process::find_pids_by_name("iTerm2");
    if pids.is_empty() {
        return Ok(None);
    }

    let output = Command::new("osascript")
        .args(["-e", APPLESCRIPT])
        .output()?;

    if !output.status.success() {
        eprintln!(
            "warning: could not query iTerm2 via AppleScript: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return Ok(Some(TerminalEmulator {
            app: "iTerm2".into(),
            pid: pids.first().copied(),
            windows: vec![],
        }));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut windows_map: std::collections::BTreeMap<String, Vec<TerminalTab>> =
        std::collections::BTreeMap::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 6 {
            continue;
        }

        let window_id = parts[0].to_string();
        let _tab_idx = parts[1];
        let title = parts[2].to_string();
        let tty = parts[3].to_string();
        let cols: Option<u32> = parts[4].parse().ok();
        let rows: Option<u32> = parts[5].parse().ok();

        let (shell_pid, shell) = process::get_shell_for_tty(&tty)
            .map(|(p, s)| (Some(p), Some(s)))
            .unwrap_or((None, None));

        let cwd = shell_pid.and_then(process::get_cwd);

        let tab = TerminalTab {
            title,
            uri: None,
            tty: Some(tty),
            shell_pid,
            shell,
            cwd,
            columns: cols,
            rows,
        };

        windows_map
            .entry(window_id)
            .or_default()
            .push(tab);
    }

    let windows: Vec<TerminalWindow> = windows_map
        .into_iter()
        .map(|(id, tabs)| TerminalWindow { id, tabs })
        .collect();

    Ok(Some(TerminalEmulator {
        app: "iTerm2".into(),
        pid: pids.first().copied(),
        windows,
    }))
}

/// Focus an iTerm2 window/tab using the native iterm2-client crate.
pub async fn focus(window_id: Option<&str>, tab_id: Option<&str>) -> Result<()> {
    if tab_id.is_some() {
        if let Err(e) = focus_via_api(tab_id).await {
            crate::log::log("daemon", &format!("iTerm2 API error (falling back to AppleScript): {}", e));
        }
    }

    crate::workspace::uri::activate_app_sync("iTerm2");

    let _ = window_id;
    Ok(())
}

async fn focus_via_api(tab_id: Option<&str>) -> Result<()> {
    use iterm2_client::{App, Connection};

    let conn = Connection::connect("zestful-daemon").await?;
    let app = App::new(conn);
    let sessions = app.list_sessions().await?;

    let tab_id = match tab_id {
        Some(id) => id,
        None => return Ok(()),
    };

    if let Ok(tab_idx) = tab_id.parse::<usize>() {
        let zero_idx = tab_idx.saturating_sub(1);
        for window in &sessions.windows {
            if let Some(tab_info) = window.tabs.get(zero_idx) {
                tab_info.tab.activate().await?;
                return Ok(());
            }
        }
    }

    for window in &sessions.windows {
        for tab_info in &window.tabs {
            if tab_info.tab.id == tab_id {
                tab_info.tab.activate().await?;
                return Ok(());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_focus_no_ids() {
        let result = focus(None, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_focus_with_ids_falls_back() {
        let result = focus(Some("99999"), Some("1")).await;
        assert!(result.is_ok());
    }
}
