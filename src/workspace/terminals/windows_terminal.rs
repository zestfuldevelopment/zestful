//! Windows Terminal detection and focus (Windows only).
//!
//! Detection uses EnumWindows (C# P/Invoke) to find Windows Terminal top-level windows,
//! then Windows UI Automation to enumerate the individual tabs within each window.
//! Each tab's shell PID is determined by matching the tab's position (in UI Automation
//! order) to the WT process's direct child processes sorted by creation time.
//!
//! Focus uses the shell PID to find the correct tab at focus time: it re-queries the
//! child processes of the WT process sorted by creation time, finds the index of the
//! target PID, and uses that index to activate the corresponding tab via
//! UI Automation SelectionItemPattern (falling back to InvokePattern).
//! This is robust to tab reordering by drag because the PID is stable whereas the
//! positional index changes.
//! AttachThreadInput + SetForegroundWindow is used to raise the window, which is
//! required on Windows 11 where SetForegroundWindow alone is blocked for background
//! processes.

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
public class ZestfulWTEnum {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lp);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, StringBuilder sb, int max);
    public static List<string> FindWindows(uint[] pids) {
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
            results.Add(((long)hWnd).ToString() + "|" + pid.ToString() + "|" + sb.ToString());
            return true;
        }, IntPtr.Zero);
        return results;
    }
}' } catch {}

$wtPids = [uint32[]](Get-Process -Name WindowsTerminal -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
if ($wtPids.Count -eq 0) { exit }

# Build a map of wtPid -> child shell PIDs sorted by process creation time.
# The creation order approximates the original tab creation order.
$allProcs = Get-CimInstance Win32_Process -ErrorAction SilentlyContinue
$childrenByWtPid = @{}
foreach ($wtPid in $wtPids) {
    $kids = @($allProcs | Where-Object { $_.ParentProcessId -eq $wtPid } | Sort-Object CreationDate, ProcessId | Select-Object -ExpandProperty ProcessId)
    $childrenByWtPid[[uint32]$wtPid] = $kids
}

$tabCond = New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
    [System.Windows.Automation.ControlType]::TabItem
)

# Output format: hwnd|shellPid|tabTitle
[ZestfulWTEnum]::FindWindows($wtPids) | ForEach-Object {
    $parts = $_ -split '\|', 3
    $hwnd = [long]$parts[0]
    $wtPid = [uint32]$parts[1]
    $kids = $childrenByWtPid[$wtPid]
    try {
        $ae = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$hwnd)
        $tabs = $ae.FindAll([System.Windows.Automation.TreeScope]::Descendants, $tabCond)
        if ($tabs.Count -gt 0) {
            for ($i = 0; $i -lt $tabs.Count; $i++) {
                $shellPid = if ($kids -and $i -lt $kids.Count) { $kids[$i] } else { 0 }
                "$hwnd|$shellPid|$($tabs[$i].Current.Name)"
            }
        } else {
            $shellPid = if ($kids -and $kids.Count -gt 0) { $kids[0] } else { 0 }
            "$hwnd|$shellPid|"
        }
    } catch {
        "$hwnd|0|"
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

        // Format: hwnd|shellPid|tabTitle
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

    // tab_id is the shell PID stored during detection. At focus time we re-query the
    // WT process's direct children sorted by creation time to find the current tab
    // index for that PID. This is robust to the user reordering tabs by drag.
    let shell_pid: u32 = tab_id.and_then(|t| t.parse().ok()).unwrap_or(0);
    crate::log::log("wt-focus", &format!("hwnd={} shell_pid={}", hwnd, shell_pid));

    let script = format!(
        r#"
try {{ Add-Type -AssemblyName UIAutomationClient; Add-Type -AssemblyName UIAutomationTypes }} catch {{}}
try {{ Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public class ZestfulWT {{ [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int n); [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h); [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h); [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow(); [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p); [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool f); }}' }} catch {{}}

$hwnd = [IntPtr]{hwnd}
[ZestfulWT]::ShowWindow($hwnd, 9)
$d = [uint32]0
$fg = [ZestfulWT]::GetWindowThreadProcessId([ZestfulWT]::GetForegroundWindow(), [ref]$d)
$tgt = [ZestfulWT]::GetWindowThreadProcessId($hwnd, [ref]$d)
[ZestfulWT]::AttachThreadInput($fg, $tgt, $true)
[ZestfulWT]::SetForegroundWindow($hwnd)
[ZestfulWT]::BringWindowToTop($hwnd)
[ZestfulWT]::AttachThreadInput($fg, $tgt, $false)

try {{
    $ae = [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
    $tabCond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::TabItem
    )
    $tabs = $ae.FindAll([System.Windows.Automation.TreeScope]::Descendants, $tabCond)

    # Resolve shell PID to a tab index.
    #
    # Primary strategy: sort WT's direct children by CreationDate (+ ProcessId as a
    # stable tiebreaker for processes opened simultaneously) and use the positional
    # index. The ProcessId tiebreaker is critical: when several tabs are opened at
    # once their CreationDate timestamps are identical, making Sort-Object unstable
    # across separate PowerShell invocations and causing detect/focus to disagree.
    #
    # Fallback when child count != tab count: Windows 11 "default terminal" (defterm)
    # adds UI tabs whose shells are NOT direct WT children (their parent is explorer or
    # another process), so $children.Count < $tabs.Count and the positional index lands
    # on the wrong tab. WT helper processes (e.g. for elevation) go the other way.
    # In either case we fall back to title-based matching: find which rank the target
    # PID has among same-process-name WT children, then pick the N-th UI tab whose
    # title suggests it runs that process.
    $targetPid = [uint32]{shell_pid}
    $tabIdx = -1
    if ($targetPid -gt 0) {{
        $wtProcId = [uint32]0
        [ZestfulWT]::GetWindowThreadProcessId($hwnd, [ref]$wtProcId) | Out-Null
        $children = @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$wtProcId" `
            -ErrorAction SilentlyContinue | Sort-Object CreationDate, ProcessId)
        $positionalIdx = -1
        for ($ci = 0; $ci -lt $children.Count; $ci++) {{
            if ($children[$ci].ProcessId -eq $targetPid) {{
                $positionalIdx = $ci
                break
            }}
        }}
        if ($positionalIdx -ge 0) {{
            $tabIdx = $positionalIdx
            # When child and tab counts differ, try title-based correction.
            if ($children.Count -ne $tabs.Count) {{
                $targetName = ($children[$positionalIdx].Name -replace '\.exe$','').ToLower()
                # Rank of target among all WT children with the same process name.
                $sameProcKids = @($children | Where-Object {{ $_.Name -ieq $children[$positionalIdx].Name }})
                $rank = -1
                for ($si = 0; $si -lt $sameProcKids.Count; $si++) {{
                    if ($sameProcKids[$si].ProcessId -eq $targetPid) {{ $rank = $si; break }}
                }}
                # Find the N-th UI tab (rank-th) whose title contains the process name.
                if ($rank -ge 0) {{
                    $matchCount = 0
                    $correctedIdx = -1
                    for ($ti = 0; $ti -lt $tabs.Count; $ti++) {{
                        if ($tabs[$ti].Current.Name.ToLower() -like "*$targetName*") {{
                            if ($matchCount -eq $rank) {{ $correctedIdx = $ti; break }}
                            $matchCount++
                        }}
                    }}
                    if ($correctedIdx -ge 0) {{ $tabIdx = $correctedIdx }}
                }}
            }}
        }}
    }}

    $dbgChildren = @($children).Count
    [System.Console]::Error.WriteLine("children=$dbgChildren tabs=$($tabs.Count) positionalIdx=$positionalIdx tabIdx=$tabIdx")
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
