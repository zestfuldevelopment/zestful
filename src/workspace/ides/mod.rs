//! IDE detection (Xcode, etc.)

#[cfg(target_os = "macos")]
mod xcode;

use anyhow::Result;
use crate::workspace::types::IdeInstance;

pub fn detect_all() -> Result<Vec<IdeInstance>> {
    let mut ides = Vec::new();

    #[cfg(target_os = "macos")]
    {
        if let Ok(xcode) = xcode::detect() {
            if let Some(instance) = xcode {
                ides.push(instance);
            }
        }
    }

    Ok(ides)
}
