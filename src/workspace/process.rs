use std::process::Command;

/// Get the current working directory of a process by PID.
/// Uses /proc on Linux, Get-CimInstance on Windows, falls back to lsof on other platforms.
pub fn get_cwd(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        return std::fs::read_link(format!("/proc/{}/cwd", pid))
            .ok()
            .and_then(|p| p.into_os_string().into_string().ok());
    }

    #[cfg(target_os = "windows")]
    {
        return get_cwds_batch(&[pid]).remove(&pid);
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let output = Command::new("lsof")
            .args(["-p", &pid.to_string(), "-a", "-d", "cwd", "-Fn"])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Some(path) = line.strip_prefix('n') {
                return Some(path.to_string());
            }
        }
        None
    }
}

/// Batch-query the working directory for multiple processes in a single subprocess call.
/// Returns a map of PID → CWD for any processes where the CWD could be determined.
#[cfg(target_os = "windows")]
pub fn get_cwds_batch(pids: &[u32]) -> std::collections::HashMap<u32, String> {
    use std::collections::HashMap;

    if pids.is_empty() {
        return HashMap::new();
    }

    let filter = pids
        .iter()
        .map(|p| format!("ProcessId={}", p))
        .collect::<Vec<_>>()
        .join(" OR ");

    let script = format!(
        "Get-CimInstance Win32_Process -Filter '{filter}' | \
         ForEach-Object {{ \"$($_.ProcessId)|$($_.WorkingDirectory)\" }}"
    );

    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return HashMap::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut map = HashMap::new();

    for line in stdout.lines() {
        let line = line.trim();
        if let Some((pid_str, cwd)) = line.split_once('|') {
            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                let cwd = cwd.trim();
                if !cwd.is_empty() {
                    map.insert(pid, cwd.to_string());
                }
            }
        }
    }

    map
}

/// Enumerate visible top-level windows for the named executable and return
/// (pid, window_title) pairs. Processes with no visible titled window are
/// excluded — equivalent to tasklist's `WINDOWTITLE ne N/A` filter.
#[cfg(target_os = "windows")]
pub fn query_tasklist(exe_name: &str) -> Vec<(u32, String)> {
    crate::workspace::win32::query_processes(exe_name)
}

/// Given a tty name (e.g. "/dev/ttys000" or "ttys000"), find the shell process and its PID.
pub fn get_shell_for_tty(tty: &str) -> Option<(u32, String)> {
    let tty_short = tty.strip_prefix("/dev/").unwrap_or(tty);

    let output = Command::new("ps")
        .args(["-t", tty_short, "-o", "pid=,comm="])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let known_shells = [
        "zsh", "bash", "fish", "sh", "tcsh", "csh", "dash", "ksh", "nu", "nushell", "elvish",
    ];

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let pid_str = parts.next()?;
        let comm = parts.next()?.trim();
        let basename = comm.rsplit('/').next().unwrap_or(comm);
        let basename = basename.strip_prefix('-').unwrap_or(basename);

        if known_shells.contains(&basename) {
            if let Ok(pid) = pid_str.parse::<u32>() {
                return Some((pid, basename.to_string()));
            }
        }
    }
    None
}

/// Find PIDs of a process by name.
/// On macOS, tries System Events (AppleScript) first for GUI apps, then falls back to pgrep.
/// On Windows, uses the Win32 process snapshot API. On Linux, uses pgrep.
pub fn find_pids_by_name(name: &str) -> Vec<u32> {
    #[cfg(target_os = "windows")]
    {
        return crate::workspace::win32::find_pids_by_exe(name);
    }

    #[cfg(target_os = "macos")]
    {
        // Try System Events first (macOS) — reliably finds GUI apps that pgrep misses
        let script = format!(
            r#"tell application "System Events" to get the unix id of every process whose name is "{}""#,
            name
        );
        let output = Command::new("osascript").args(["-e", &script]).output();

        if let Ok(ref o) = output {
            if o.status.success() {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let pids: Vec<u32> = stdout
                    .trim()
                    .split(", ")
                    .filter_map(|s| s.trim().parse::<u32>().ok())
                    .collect();
                if !pids.is_empty() {
                    return pids;
                }
            }
        }
    }

    // Use pgrep to find processes by name (Linux and macOS fallback)
    #[cfg(not(target_os = "windows"))]
    {
        let output = Command::new("pgrep").args(["-x", name]).output();

        let output = match output {
            Ok(o) if o.status.success() => o,
            _ => return vec![],
        };

        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|l| l.trim().parse::<u32>().ok())
            .collect()
    }
}
