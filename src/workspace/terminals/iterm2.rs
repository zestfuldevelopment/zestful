//! iTerm2 detection and focus handler (macOS only).
//!
//! Each iTerm2 session (pane/split) maps to one entry with its TTY as the
//! unique identifier. Focus uses AppleScript to find the session by TTY,
//! select it, raise its window, and activate the app.

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
    let mut windows = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 6 {
            continue;
        }

        let _window_id = parts[0];
        let _tab_idx = parts[1];
        let title = parts[2].to_string();
        let tty = parts[3].to_string();
        let cols: Option<u32> = parts[4].parse().ok();
        let rows: Option<u32> = parts[5].parse().ok();

        let (shell_pid, shell) = process::get_shell_for_tty(&tty)
            .map(|(p, s)| (Some(p), Some(s)))
            .unwrap_or((None, None));

        let cwd = shell_pid.and_then(process::get_cwd);

        // Strip /dev/ prefix for cleaner URIs
        let tty_id = tty.strip_prefix("/dev/").unwrap_or(&tty).to_string();

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

        // Each session (pane/split) gets its own TerminalWindow with the TTY as ID
        windows.push(TerminalWindow {
            id: tty_id,
            tabs: vec![tab],
        });
    }

    Ok(Some(TerminalEmulator {
        app: "iTerm2".into(),
        pid: pids.first().copied(),
        windows,
    }))
}

/// Focus an iTerm2 session (pane) by its TTY.
/// Finds the session by TTY, selects it (which switches tab and pane),
/// raises the containing window, and activates the app.
pub async fn focus(window_id: Option<&str>, _tab_id: Option<&str>) -> Result<()> {
    let window_id = window_id.map(String::from);

    tokio::task::spawn_blocking(move || {
        focus_applescript(window_id.as_deref())
    })
    .await??;

    Ok(())
}

fn focus_applescript(tty_name: Option<&str>) -> Result<()> {
    let script = match tty_name {
        Some(tty) if !tty.is_empty() => {
            let tty_path = if tty.starts_with("/dev/") {
                tty.to_string()
            } else {
                format!("/dev/{}", tty)
            };
            let escaped = crate::workspace::uri::escape_applescript(&tty_path);
            format!(
                r#"tell application "iTerm2"
  repeat with w in windows
    set tabList to tabs of w
    repeat with tabIdx from 1 to count of tabList
      set t to item tabIdx of tabList
      repeat with s in sessions of t
        try
          if tty of s is equal to "{}" then
            select tab tabIdx of w
            select s
            set index of w to 1
            activate
            return
          end if
        end try
      end repeat
    end repeat
  end repeat
  activate
end tell"#,
                escaped
            )
        }
        _ => r#"tell application "iTerm2" to activate"#.to_string(),
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
    async fn test_focus_no_ids() {
        let result = focus(None, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_focus_with_tty() {
        let result = focus(Some("ttys999"), None).await;
        assert!(result.is_ok());
    }
}
