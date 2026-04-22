//! PowerShell detection and focus handler (Windows only).

use anyhow::Result;

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
    let mut entries: Vec<(u32, String, &str)> = Vec::new();

    for &(exe_name, shell_name) in &[("powershell.exe", "powershell"), ("pwsh.exe", "pwsh")] {
        for (pid, title) in process::query_tasklist(exe_name) {
            entries.push((pid, title, shell_name));
        }
    }

    if entries.is_empty() {
        return Vec::new();
    }

    let pids: Vec<u32> = entries.iter().map(|(pid, _, _)| *pid).collect();
    let cwds = process::get_cwds_batch(&pids);

    entries
        .into_iter()
        .map(|(pid, title, shell_name)| TerminalWindow {
            id: pid.to_string(),
            tabs: vec![TerminalTab {
                title: if title.is_empty() {
                    shell_name.to_string()
                } else {
                    title
                },
                uri: None,
                tty: None,
                shell_pid: Some(pid),
                shell: Some(shell_name.to_string()),
                cwd: cwds.get(&pid).cloned(),
                columns: None,
                rows: None,
            }],
        })
        .collect()
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
    let pid: u32 = match window_id {
        Some(s) if s.chars().all(|c| c.is_ascii_digit()) => s.parse().unwrap_or(0),
        _ => crate::workspace::win32::find_pids_by_exe("powershell.exe")
            .into_iter()
            .chain(crate::workspace::win32::find_pids_by_exe("pwsh.exe"))
            .next()
            .unwrap_or(0),
    };
    if pid != 0 {
        crate::workspace::win32::focus_by_pid(pid);
    }
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
