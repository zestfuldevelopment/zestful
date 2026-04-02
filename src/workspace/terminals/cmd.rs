//! Windows Command Prompt (cmd.exe) detection and focus handler.

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
        app: "Cmd".into(),
        pid: first_pid,
        windows,
    }))
}

fn collect_windows() -> Vec<TerminalWindow> {
    let mut windows = Vec::new();

    for (pid, title) in query_tasklist("cmd.exe") {
        let cwd = process::get_cwd(pid);
        let tab = TerminalTab {
            title: if title.is_empty() {
                "cmd".to_string()
            } else {
                title
            },
            uri: None,
            tty: None,
            shell_pid: Some(pid),
            shell: Some("cmd".to_string()),
            cwd,
            columns: None,
            rows: None,
        };
        windows.push(TerminalWindow {
            id: pid.to_string(),
            tabs: vec![tab],
        });
    }

    windows
}

/// Query tasklist for cmd.exe and return (pid, window_title) pairs.
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

/// Focus a cmd.exe window.
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
    // Inline C# P/Invoke — try/catch silences "type already exists" on repeated calls.
    let add_type = r#"try { Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public class ZestfulWin32 { [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int n); [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h); }' } catch {}"#;

    let activate = match window_id {
        Some(pid) if pid.chars().all(|c| c.is_ascii_digit()) => format!(
            "$p = Get-Process -Id {} -ErrorAction SilentlyContinue; \
             if ($p -and $p.MainWindowHandle -ne 0) {{ \
               [ZestfulWin32]::ShowWindow($p.MainWindowHandle, 9); \
               [ZestfulWin32]::SetForegroundWindow($p.MainWindowHandle) }}",
            pid
        ),
        _ => String::from(
            "$p = Get-Process -Name cmd -ErrorAction SilentlyContinue \
             | Select-Object -First 1; \
             if ($p -and $p.MainWindowHandle -ne 0) { \
               [ZestfulWin32]::ShowWindow($p.MainWindowHandle, 9); \
               [ZestfulWin32]::SetForegroundWindow($p.MainWindowHandle) }"
        ),
    };

    let script = format!("{}; {}", add_type, activate);

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
