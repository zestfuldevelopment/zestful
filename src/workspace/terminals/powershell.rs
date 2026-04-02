//! PowerShell detection and focus handler (Windows only).

use anyhow::Result;
use std::process::Command;

use crate::workspace::process;
use crate::workspace::types::{TerminalEmulator, TerminalTab, TerminalWindow};

pub fn detect() -> Result<Option<TerminalEmulator>> {
    let windows = collect_windows();
    if windows.is_empty() {
        return Ok(None);
    }

    let first_pid = windows
        .first()
        .and_then(|w| w.tabs.first())
        .and_then(|t| t.shell_pid);

    Ok(Some(TerminalEmulator {
        app: "PowerShell".into(),
        pid: first_pid,
        windows,
    }))
}

fn collect_windows() -> Vec<TerminalWindow> {
    let mut windows = Vec::new();

    for &exe_name in &["powershell.exe", "pwsh.exe"] {
        let entries = query_tasklist(exe_name);
        for (pid, title) in entries {
            let cwd = process::get_cwd(pid);
            let shell = if exe_name == "pwsh.exe" {
                "pwsh"
            } else {
                "powershell"
            };
            let tab = TerminalTab {
                title: if title.is_empty() {
                    shell.to_string()
                } else {
                    title
                },
                uri: None,
                tty: None,
                shell_pid: Some(pid),
                shell: Some(shell.to_string()),
                cwd,
                columns: None,
                rows: None,
            };
            windows.push(TerminalWindow {
                id: pid.to_string(),
                tabs: vec![tab],
            });
        }
    }

    windows
}

/// Query tasklist for a specific executable and return (pid, window_title) pairs.
fn query_tasklist(exe_name: &str) -> Vec<(u32, String)> {
    let output = Command::new("tasklist")
        .args([
            "/fi",
            &format!("imagename eq {}", exe_name),
            "/fo",
            "csv",
            "/v",
            "/nh",
        ])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        if !line.starts_with('"') {
            continue;
        }

        let stripped = line
            .strip_prefix('"')
            .unwrap_or(line)
            .trim_end_matches('"');
        let fields: Vec<&str> = stripped.split("\",\"").collect();

        if fields.len() < 9 {
            continue;
        }

        let pid: u32 = match fields[1].parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        let title = fields[8].trim();
        let title = if title == "N/A" || title == "OleMainThreadWndName" {
            String::new()
        } else {
            title.to_string()
        };

        results.push((pid, title));
    }

    results
}

/// Focus a PowerShell window.
pub async fn focus(window_id: Option<&str>) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        tokio::task::spawn_blocking({
            let window_id = window_id.map(String::from);
            move || focus_sync(window_id.as_deref())
        })
        .await??;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window_id;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn focus_sync(window_id: Option<&str>) -> Result<()> {
    let target = match window_id {
        Some(pid) if pid.chars().all(|c| c.is_ascii_digit()) => pid.to_string(),
        _ => "\"PowerShell\"".to_string(),
    };

    let script = format!(
        "Add-Type -AssemblyName Microsoft.VisualBasic; \
         [Microsoft.VisualBasic.Interaction]::AppActivate({})",
        target
    );

    let _ = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output();

    Ok(())
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
    async fn test_focus_with_pid() {
        let result = focus(Some("1234")).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_focus_with_non_numeric_id() {
        let result = focus(Some("some-id")).await;
        assert!(result.is_ok());
    }
}
