//! Ghostty detection (no IPC API available yet).

use anyhow::Result;

use crate::workspace::process;
use crate::workspace::types::TerminalEmulator;

pub fn detect() -> Result<Option<TerminalEmulator>> {
    let pids = process::find_pids_by_name("Ghostty");
    if pids.is_empty() {
        return Ok(None);
    }

    Ok(Some(TerminalEmulator {
        app: "Ghostty".into(),
        pid: pids.first().copied(),
        windows: vec![],
    }))
}
