//! WezTerm focus handler.
//!
//! Uses the `wezterm cli activate-tab` command to switch tabs.

use anyhow::Result;

/// Focus a WezTerm tab.
pub async fn focus(tab_id: Option<&str>) -> Result<()> {
    tokio::task::spawn_blocking({
        let tab_id = tab_id.map(String::from);
        move || focus_sync(tab_id.as_deref())
    })
    .await??;
    Ok(())
}

fn focus_sync(tab_id: Option<&str>) -> Result<()> {
    if let Some(tab_id) = tab_id {
        let wezterm = find_wezterm();
        if let Some(wezterm) = wezterm {
            let _ = std::process::Command::new(&wezterm)
                .args(["cli", "activate-tab", "--tab-id", tab_id])
                .output();
        }
    }

    #[cfg(target_os = "macos")]
    {
        super::activate_app_sync("WezTerm");
    }

    Ok(())
}

fn find_wezterm() -> Option<String> {
    let paths = ["/opt/homebrew/bin/wezterm", "/usr/local/bin/wezterm"];
    for p in &paths {
        if std::path::Path::new(p).exists() {
            return Some(p.to_string());
        }
    }
    None
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
    async fn test_focus_with_tab() {
        let result = focus(Some("123")).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_find_wezterm() {
        // Should not panic
        let _ = find_wezterm();
    }
}
