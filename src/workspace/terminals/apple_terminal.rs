//! Terminal.app detection and focus handler (macOS only).

use anyhow::Result;
use std::process::Command;

use crate::workspace::process;
use crate::workspace::types::{TerminalEmulator, TerminalTab, TerminalWindow};

const APPLESCRIPT: &str = r#"
tell application "Terminal"
    set output to ""
    set winList to every window
    repeat with w in winList
        try
            set wid to id of w
            set tabList to every tab of w
            repeat with t in tabList
                try
                    set ttyVal to tty of t
                    set titleVal to custom title of t
                    set colsVal to number of columns of t
                    set rowsVal to number of rows of t
                    set output to output & wid & "\t" & titleVal & "\t" & ttyVal & "\t" & colsVal & "\t" & rowsVal & "\n"
                end try
            end repeat
        end try
    end repeat
    return output
end tell
"#;

pub fn detect() -> Result<Option<TerminalEmulator>> {
    let pids = process::find_pids_by_name("Terminal");
    if pids.is_empty() {
        return Ok(None);
    }

    let output = Command::new("osascript")
        .args(["-e", APPLESCRIPT])
        .output()?;

    if !output.status.success() {
        eprintln!(
            "warning: could not query Terminal.app via AppleScript: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return Ok(Some(TerminalEmulator {
            app: "Terminal".into(),
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
        if parts.len() < 5 {
            continue;
        }

        let window_id = parts[0].to_string();
        let title = parts[1].to_string();
        let tty = parts[2].to_string();
        let cols: Option<u32> = parts[3].parse().ok();
        let rows: Option<u32> = parts[4].parse().ok();

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

        windows_map.entry(window_id).or_default().push(tab);
    }

    let windows: Vec<TerminalWindow> = windows_map
        .into_iter()
        .map(|(id, tabs)| TerminalWindow { id, tabs })
        .collect();

    Ok(Some(TerminalEmulator {
        app: "Terminal".into(),
        pid: pids.first().copied(),
        windows,
    }))
}

/// Focus a Terminal.app tab by window ID and tab index.
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
    let script = match (window_id, tab_id) {
        (Some(win_id), Some(tab_idx)) => {
            // tab_id from the URI is a 1-based tab index; window_id is the AppleScript window id
            let tab_index: u32 = tab_idx.parse().unwrap_or(1);
            let win_id_int: i64 = win_id.parse().unwrap_or(-1);
            format!(
                r#"tell application "Terminal"
  try
    repeat with w in windows
      try
        if id of w is equal to {} then
          set selected tab of w to tab {} of w
          set index of w to 1
          activate
          return
        end if
      on error
        -- window may have closed; skip it
      end try
    end repeat
  on error
    -- windows list changed during iteration; ignore
  end try
end tell"#,
                win_id_int, tab_index
            )
        }
        _ => r#"tell application "Terminal" to activate"#.to_string(),
    };

    let _ = Command::new("osascript")
        .args(["-e", &script])
        .output();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_focus_no_panic() {
        let result = focus(None, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_focus_with_window_and_tab() {
        let result = focus(Some("12345"), Some("1")).await;
        assert!(result.is_ok());
    }
}
