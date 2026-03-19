#[cfg(target_os = "macos")]
pub mod iterm2;
pub mod kitty;
pub mod terminal;
pub mod wezterm;

use anyhow::Result;

/// Dispatch focus to the appropriate terminal handler.
pub async fn handle_focus(app: &str, window_id: Option<&str>, tab_id: Option<&str>) -> Result<()> {
    let lower = app.to_lowercase();

    if lower.contains("kitty") {
        kitty::focus(window_id, tab_id).await
    } else if lower.contains("iterm") {
        #[cfg(target_os = "macos")]
        {
            iterm2::focus(tab_id).await
        }
        #[cfg(not(target_os = "macos"))]
        {
            activate_generic(app).await
        }
    } else if lower.contains("wezterm") {
        wezterm::focus(tab_id).await
    } else if lower.contains("terminal") {
        terminal::focus(tab_id).await
    } else {
        activate_generic(app).await
    }
}

/// Generic app activation via osascript (macOS) or no-op.
pub async fn activate_generic(app: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let app = app.to_string();
        tokio::task::spawn_blocking(move || {
            let _ = std::process::Command::new("osascript")
                .args(["-e", &format!("tell application \"{}\" to activate", app)])
                .output();
        })
        .await?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
    }
    Ok(())
}

/// Activate an app by name via osascript. Used by focus handlers.
#[cfg(target_os = "macos")]
pub fn activate_app_sync(app: &str) {
    let _ = std::process::Command::new("osascript")
        .args(["-e", &format!("tell application \"{}\" to activate", app)])
        .output();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_handle_focus_dispatches_kitty() {
        // Should not panic even though kitty isn't running
        let result = handle_focus("kitty", None, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_handle_focus_dispatches_wezterm() {
        let result = handle_focus("WezTerm", None, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_handle_focus_dispatches_terminal() {
        let result = handle_focus("Terminal", None, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_handle_focus_dispatches_generic() {
        let result = handle_focus("SomeRandomApp", None, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_handle_focus_case_insensitive() {
        // "KITTY" should route to kitty handler
        let result = handle_focus("KITTY", None, None).await;
        assert!(result.is_ok());
    }
}
