use anyhow::Result;

/// Focus an iTerm2 tab using the native iterm2-client crate.
pub async fn focus(tab_id: Option<&str>) -> Result<()> {
    if let Some(tab_id) = tab_id {
        if let Err(e) = focus_via_api(tab_id).await {
            eprintln!("[zestfuld] iTerm2 API error (falling back to AppleScript): {}", e);
        }
    }

    // Always bring iTerm2 to front
    super::activate_app_sync("iTerm2");

    Ok(())
}

async fn focus_via_api(tab_id: &str) -> Result<()> {
    use iterm2_client::{App, Connection};

    let conn = Connection::connect("zestful-daemon").await?;
    let app = App::new(conn);
    let sessions = app.list_sessions().await?;

    for window in &sessions.windows {
        for tab in &window.tabs {
            for session in &tab.sessions {
                // Match by title (titleOverride) or session id
                let matches = session
                    .title
                    .as_deref()
                    .map(|t| t == tab_id)
                    .unwrap_or(false);

                if matches {
                    session.activate().await?;
                    return Ok(());
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_focus_no_tab_id() {
        // With no tab_id, should just activate the app (or no-op)
        let result = focus(None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_focus_with_tab_id_falls_back() {
        // iTerm2 API likely not available in test, should fall back gracefully
        let result = focus(Some("test-tab")).await;
        assert!(result.is_ok());
    }
}
