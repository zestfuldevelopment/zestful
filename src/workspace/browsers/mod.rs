//! Browser detection (Chrome, Safari, etc.)

#[cfg(target_os = "macos")]
mod chrome;

use anyhow::Result;
use crate::workspace::types::BrowserInstance;

pub fn detect_all() -> Result<Vec<BrowserInstance>> {
    let mut browsers = Vec::new();

    #[cfg(target_os = "macos")]
    {
        if let Ok(Some(instance)) = chrome::detect() {
            browsers.push(instance);
        }
    }

    Ok(browsers)
}
