//! `zestful inspect` — Windows Terminal detection and focus (Windows only).
//!
//! Detection enumerates `PseudoConsoleWindow` top-level windows whose Win32
//! owner (`GetParent`) is the WT frame HWND (`CASCADIA_HOSTING_WINDOW_CLASS`).
//! Each such window is owned by the shell process for that tab
//! (`GetWindowThreadProcessId`).  Sorting those shell PIDs by process creation
//! time (+ ProcessId as a stable tiebreaker) reproduces the left-to-right tab
//! order shown in the UI.
//!
//! This approach works for both ordinary tabs (shell is a direct child of the
//! WT process) and "default terminal" (defterm) tabs (shell was spawned by
//! explorer or another process and captured by WT) — in both cases WT creates
//! a `PseudoConsoleWindow` owned by its frame HWND, and the owning process is
//! always the actual interactive shell, never an intermediate like
//! `OpenConsole.exe`.
//!
//! Focus re-derives the tab index at call time by repeating the same
//! PseudoConsoleWindow enumeration and sort, then activates the matching
//! `TabItem` via UI Automation `SelectionItemPattern` (falling back to
//! `InvokePattern`).  Using the shell PID as the stable tab identity means
//! focus is robust to the user reordering tabs by drag.
//! `AttachThreadInput` + `SetForegroundWindow` is used to raise the window,
//! which is required on Windows 11 where `SetForegroundWindow` alone is
//! blocked for background processes.

use anyhow::Result;
use std::process::Command;

use crate::workspace::types::{TerminalEmulator, TerminalTab, TerminalWindow};

pub fn detect() -> Result<Option<TerminalEmulator>> {
    let script = r#"
try { Add-Type -AssemblyName UIAutomationClient; Add-Type -AssemblyName UIAutomationTypes } catch {}
try { Add-Type -TypeDefinition '
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public class ZestfulWTDetect {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lp);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, StringBuilder sb, int max);
    [DllImport("user32.dll")] public static extern int GetClassName(IntPtr hWnd, StringBuilder sb, int max);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr hWnd);

    // Returns "hwnd|wtPid|title" for each visible CASCADIA_HOSTING_WINDOW_CLASS window.
    public static List<string> FindFrames(uint[] wtPids) {
        var pidSet = new HashSet<uint>(wtPids);
        var results = new List<string>();
        EnumWindows((hWnd, lp) => {
            if (!IsWindowVisible(hWnd)) return true;
            uint pid; GetWindowThreadProcessId(hWnd, out pid);
            if (!pidSet.Contains(pid)) return true;
            var cls = new StringBuilder(64);
            GetClassName(hWnd, cls, cls.Capacity);
            if (cls.ToString() != "CASCADIA_HOSTING_WINDOW_CLASS") return true;
            var title = new StringBuilder(256);
            GetWindowText(hWnd, title, title.Capacity);
            results.Add((long)hWnd + "|" + pid + "|" + title);
            return true;
        }, IntPtr.Zero);
        return results;
    }

    // Returns "hwnd|shellPid" for each PseudoConsoleWindow owned by frameHwnd.
    public static List<string> FindPseudoConsoleWindows(long frameHwnd) {
        var results = new List<string>();
        EnumWindows((hWnd, lp) => {
            if ((long)GetParent(hWnd) != frameHwnd) return true;
            var cls = new StringBuilder(64);
            GetClassName(hWnd, cls, cls.Capacity);
            if (cls.ToString() != "PseudoConsoleWindow") return true;
            uint pid; GetWindowThreadProcessId(hWnd, out pid);
            results.Add((long)hWnd + "|" + pid);
            return true;
        }, IntPtr.Zero);
        return results;
    }
}' } catch {}

$wtPids = [uint32[]](Get-Process -Name WindowsTerminal -ErrorAction SilentlyContinue |
    Select-Object -ExpandProperty Id)
if ($wtPids.Count -eq 0) { exit }

$allProcs = Get-CimInstance Win32_Process -ErrorAction SilentlyContinue

$tabCond = New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
    [System.Windows.Automation.ControlType]::TabItem)

# Output format: frameHwnd|shellPid|tabTitle  (one line per tab)
[ZestfulWTDetect]::FindFrames($wtPids) | ForEach-Object {
    $parts     = $_ -split '\|', 3
    $frameHwnd = [long]$parts[0]

    # Enumerate PseudoConsoleWindows and sort by shell process creation time.
    # ProcessId is a stable tiebreaker when multiple tabs open simultaneously.
    $sorted = [ZestfulWTDetect]::FindPseudoConsoleWindows($frameHwnd) | ForEach-Object {
        $f        = $_ -split '\|', 2
        $shellPid = [uint32]$f[1]
        $proc     = $allProcs | Where-Object { $_.ProcessId -eq $shellPid } | Select-Object -First 1
        [PSCustomObject]@{ ShellPid = $shellPid; Created = $proc.CreationDate }
    } | Sort-Object Created, ShellPid

    # Pair each sorted shell with the matching UI Automation TabItem for its title.
    try {
        $ae   = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$frameHwnd)
        # Sort by RuntimeId (last component) — assigned at tab creation time, not visual position.
        # This makes the index stable across user drag-reorders.
        $tabs = @($ae.FindAll([System.Windows.Automation.TreeScope]::Descendants, $tabCond) |
            Sort-Object { ($_.GetRuntimeId() | Select-Object -Last 1) })
        for ($i = 0; $i -lt $sorted.Count; $i++) {
            $title = if ($i -lt $tabs.Count) { $tabs[$i].Current.Name } else { "" }
            "$frameHwnd|$($sorted[$i].ShellPid)|$title"
        }
    } catch {
        foreach ($s in $sorted) { "$frameHwnd|$($s.ShellPid)|" }
    }
}
"#;

    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stderr.trim().lines() {
        let line = line.trim();
        if !line.is_empty() {
            crate::log::log("wt-detect", line);
        }
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut windows: std::collections::BTreeMap<String, Vec<TerminalTab>> =
        std::collections::BTreeMap::new();

    for line in stdout.trim().lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Format: frameHwnd|shellPid|tabTitle
        let parts: Vec<&str> = line.splitn(3, '|').collect();
        if parts.len() < 3 {
            continue;
        }

        let hwnd = parts[0].to_string();
        let shell_pid: Option<u32> = parts[1].trim().parse().ok().filter(|&p| p != 0);
        let title = parts[2].trim().to_string();

        crate::log::log(
            "wt-detect",
            &format!(
                "window={} tab={} shell_pid={:?} title=\"{}\"",
                hwnd,
                windows.get(&hwnd).map_or(0, |t| t.len()) + 1,
                shell_pid,
                title,
            ),
        );

        windows.entry(hwnd).or_default().push(TerminalTab {
            title,
            uri: None,
            tty: None,
            shell_pid,
            shell: None,
            cwd: None,
            columns: None,
            rows: None,
        });
    }

    if windows.is_empty() {
        return Ok(None);
    }

    let terminal_windows: Vec<TerminalWindow> = windows
        .into_iter()
        .map(|(id, tabs)| TerminalWindow { id, tabs })
        .collect();

    Ok(Some(TerminalEmulator {
        app: "Windows Terminal".into(),
        pid: None,
        windows: terminal_windows,
    }))
}

/// Focus a specific Windows Terminal tab by window handle and shell PID.
pub async fn focus(window_id: &str, tab_id: Option<&str>) -> Result<()> {
    let window_id = window_id.to_string();
    let tab_id = tab_id.map(String::from);
    tokio::task::spawn_blocking(move || focus_sync(&window_id, tab_id.as_deref())).await??;
    Ok(())
}

fn focus_sync(window_id: &str, tab_id: Option<&str>) -> Result<()> {
    let hwnd: i64 = match window_id.parse() {
        Ok(h) => h,
        Err(_) => return Ok(()),
    };

    // tab_id is the shell PID stored during detection.  At focus time we
    // re-enumerate PseudoConsoleWindows owned by the frame and re-sort by
    // shell process creation time to find the current tab index for that PID.
    // This is robust to the user reordering tabs by drag.
    let shell_pid: u32 = tab_id.and_then(|t| t.parse().ok()).unwrap_or(0);
    crate::log::log(
        "wt-focus",
        &format!("hwnd={} shell_pid={}", hwnd, shell_pid),
    );

    let script = format!(
        r#"
try {{ Add-Type -AssemblyName UIAutomationClient; Add-Type -AssemblyName UIAutomationTypes }} catch {{}}
try {{ Add-Type -TypeDefinition '
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public class ZestfulWTFocus {{
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lp);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll")] public static extern int GetClassName(IntPtr hWnd, StringBuilder sb, int max);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int n);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool f);

    public static List<string> FindPseudoConsoleWindows(long frameHwnd) {{
        var results = new List<string>();
        EnumWindows((hWnd, lp) => {{
            if ((long)GetParent(hWnd) != frameHwnd) return true;
            var cls = new StringBuilder(64);
            GetClassName(hWnd, cls, cls.Capacity);
            if (cls.ToString() != "PseudoConsoleWindow") return true;
            uint pid; GetWindowThreadProcessId(hWnd, out pid);
            results.Add((long)hWnd + "|" + pid);
            return true;
        }}, IntPtr.Zero);
        return results;
    }}
}}' }} catch {{}}

$hwnd = [IntPtr]{hwnd}

# Raise the window.  AttachThreadInput is required on Windows 11 where
# SetForegroundWindow alone is blocked for background processes.
[ZestfulWTFocus]::ShowWindow($hwnd, 9)
$fgThread  = [ZestfulWTFocus]::GetWindowThreadProcessId([ZestfulWTFocus]::GetForegroundWindow(), [ref]0)
$tgtThread = [ZestfulWTFocus]::GetWindowThreadProcessId($hwnd, [ref]0)
[ZestfulWTFocus]::AttachThreadInput($fgThread, $tgtThread, $true)
[ZestfulWTFocus]::SetForegroundWindow($hwnd)
[ZestfulWTFocus]::BringWindowToTop($hwnd)
[ZestfulWTFocus]::AttachThreadInput($fgThread, $tgtThread, $false)

try {{
    $tabCond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::TabItem)
    $ae   = [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
    # Sort by RuntimeId (last component) — assigned at tab creation time, stable across drag-reorders.
    $tabs = @($ae.FindAll([System.Windows.Automation.TreeScope]::Descendants, $tabCond) |
        Sort-Object {{ ($_.GetRuntimeId() | Select-Object -Last 1) }})

    $targetPid = [uint32]{shell_pid}
    $tabIdx    = -1

    if ($targetPid -gt 0) {{
        $allProcs = Get-CimInstance Win32_Process -ErrorAction SilentlyContinue
        $sorted = [ZestfulWTFocus]::FindPseudoConsoleWindows({hwnd}) | ForEach-Object {{
            $f        = $_ -split '\|', 2
            $shellPid = [uint32]$f[1]
            $proc     = $allProcs | Where-Object {{ $_.ProcessId -eq $shellPid }} | Select-Object -First 1
            [PSCustomObject]@{{ ShellPid = $shellPid; Created = $proc.CreationDate }}
        }} | Sort-Object Created, ShellPid

        for ($i = 0; $i -lt $sorted.Count; $i++) {{
            if ($sorted[$i].ShellPid -eq $targetPid) {{ $tabIdx = $i; break }}
        }}
    }}

    [System.Console]::Error.WriteLine("pcw_count=$($sorted.Count) tabs=$($tabs.Count) tabIdx=$tabIdx")

    if ($tabIdx -ge 0 -and $tabIdx -lt $tabs.Count) {{
        $tab = $tabs[$tabIdx]
        try {{
            $pat = $tab.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern)
            $pat.Select()
        }} catch {{
            try {{
                $pat = $tab.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
                $pat.Invoke()
            }} catch {{}}
        }}
    }}
}} catch {{}}
"#,
        hwnd = hwnd,
        shell_pid = shell_pid
    );

    if let Ok(output) = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
    {
        let stderr = String::from_utf8_lossy(&output.stderr);
        for line in stderr.trim().lines() {
            let line = line.trim();
            if !line.is_empty() {
                crate::log::log("wt-focus", line);
            }
        }
    }

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
        let result = focus("99999", None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_focus_with_shell_pid_no_panic() {
        let result = focus("99999", Some("1234")).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_focus_non_numeric_hwnd() {
        let result = focus("not-a-hwnd", None).await;
        assert!(result.is_ok());
    }
}
