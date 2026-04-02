//! Alacritty detection (no focus API available).

use anyhow::Result;

use crate::workspace::process;
use crate::workspace::types::TerminalEmulator;

pub fn detect() -> Result<Option<TerminalEmulator>> {
    let pids = process::find_pids_by_name("Alacritty");
    if pids.is_empty() {
        let pids2 = process::find_pids_by_name("alacritty");
        if pids2.is_empty() {
            return Ok(None);
        }
    }

    Ok(Some(TerminalEmulator {
        app: "Alacritty".into(),
        pid: pids.first().copied(),
        windows: vec![],
    }))
}
