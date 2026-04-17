//! IDE detection (Xcode, VS Code family, etc.)

#[cfg(target_os = "macos")]
mod xcode;
#[cfg(target_os = "macos")]
mod vscode_family;

use anyhow::Result;
use crate::workspace::types::IdeInstance;

pub fn detect_all() -> Result<Vec<IdeInstance>> {
    let mut ides = Vec::new();

    #[cfg(target_os = "macos")]
    {
        if let Ok(Some(instance)) = xcode::detect() {
            ides.push(instance);
        }
        if let Ok(more) = vscode_family::detect_all() {
            ides.extend(more.into_iter().filter(|i| !i.projects.is_empty()));
        }
    }

    Ok(ides)
}
