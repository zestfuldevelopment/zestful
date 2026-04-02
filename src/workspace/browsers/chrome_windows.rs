//! Google Chrome detection and focus for Windows.
//!
//! Uses PowerShell to find chrome.exe processes with visible windows and
//! enumerate their titles. Chrome's tab API is not accessible without the
//! remote debugging protocol, so each visible top-level Chrome window is
//! reported as a single tab using the window title.

use anyhow::Result;
use std::process::Command;

use crate::workspace::types::{BrowserInstance, BrowserTab, BrowserWindow};

pub fn detect() -> Result<Option<BrowserInstance>> {
    // Chrome uses a single browser process for all windows, so MainWindowHandle
    // only gives one entry. Use EnumWindows via C# to find all top-level visible
    // Chrome windows and their titles.
    let script = r#"
try { Add-Type -TypeDefinition '
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public class ZestfulEnum {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lp);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, StringBuilder sb, int max);
    [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr hWnd);
    public static List<string> FindChromeWindows(uint[] pids) {
        var pidSet = new HashSet<uint>(pids);
        var results = new List<string>();
        EnumWindows((hWnd, lp) => {
            if (!IsWindowVisible(hWnd)) return true;
            uint pid; GetWindowThreadProcessId(hWnd, out pid);
            if (!pidSet.Contains(pid)) return true;
            int len = GetWindowTextLength(hWnd);
            if (len == 0) return true;
            var sb = new StringBuilder(len + 1);
            GetWindowText(hWnd, sb, sb.Capacity);
            string title = sb.ToString();
            if (title.EndsWith(" - Google Chrome"))
                results.Add(pid.ToString() + "|" + ((long)hWnd).ToString() + "|" + title);
            return true;
        }, IntPtr.Zero);
        return results;
    }
}' } catch {}
$chromePids = [uint32[]](Get-Process -Name chrome -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
if ($chromePids.Count -eq 0) { exit }
[ZestfulEnum]::FindChromeWindows($chromePids) | ForEach-Object { $_ }
"#;

    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut windows: Vec<BrowserWindow> = Vec::new();
    let mut first_pid: Option<u32> = None;

    for line in stdout.trim().lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Format: pid|hwnd|title
        let parts: Vec<&str> = line.splitn(3, '|').collect();
        if parts.len() < 3 {
            continue;
        }

        let pid: u32 = match parts[0].parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let hwnd = parts[1].to_string();

        // Strip the " - Google Chrome" suffix Chrome appends to all window titles
        let title = parts[2]
            .trim_end_matches(" - Google Chrome")
            .trim()
            .to_string();

        if first_pid.is_none() {
            first_pid = Some(pid);
        }

        windows.push(BrowserWindow {
            id: hwnd,
            tabs: vec![BrowserTab {
                index: 1,
                uri: None,
                title,
                active: false,
            }],
        });
    }

    if windows.is_empty() {
        return Ok(None);
    }

    Ok(Some(BrowserInstance {
        app: "Google Chrome".to_string(),
        pid: first_pid,
        windows,
    }))
}

/// Focus a Chrome window by process ID using Win32 via PowerShell.
pub async fn focus(window_id: &str) -> Result<()> {
    let window_id = window_id.to_string();
    tokio::task::spawn_blocking(move || focus_sync(&window_id)).await??;
    Ok(())
}

fn focus_sync(window_id: &str) -> Result<()> {
    // window_id is the HWND as a decimal string (from EnumWindows in detect())
    let hwnd: i64 = match window_id.parse() {
        Ok(h) => h,
        Err(_) => return Ok(()),
    };

    let add_type = r#"try { Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public class ZestfulWin32 { [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int n); [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h); }' } catch {}"#;

    let activate = format!(
        "$hwnd = [IntPtr]{}; \
         [ZestfulWin32]::ShowWindow($hwnd, 9); \
         [ZestfulWin32]::SetForegroundWindow($hwnd)",
        hwnd
    );

    let script = format!("{}; {}", add_type, activate);

    let _ = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_no_panic() {
        let _ = detect();
    }

    #[tokio::test]
    async fn test_focus_no_panic() {
        let result = focus("99999").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_focus_non_numeric_no_panic() {
        let result = focus("not-a-pid").await;
        assert!(result.is_ok());
    }
}
