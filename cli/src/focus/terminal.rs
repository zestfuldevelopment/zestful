//! Terminal.app focus handler (macOS only).
//!
//! Uses AppleScript to iterate Terminal.app windows/tabs and select the one
//! whose tty matches the given tab ID.

use anyhow::Result;

/// Focus a Terminal.app tab by tty.
pub async fn focus(tab_id: Option<&str>) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        tokio::task::spawn_blocking({
            let tab_id = tab_id.map(String::from);
            move || focus_sync(tab_id.as_deref())
        })
        .await??;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = tab_id;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn focus_sync(tab_id: Option<&str>) -> Result<()> {
    let script = if let Some(tab_id) = tab_id {
        let escaped = super::escape_applescript(tab_id);
        format!(
            r#"tell application "Terminal"
  activate
  set target_tab to "{}"
  repeat with w in windows
    repeat with t in tabs of w
      if tty of t contains target_tab then
        set selected tab of w to t
        set index of w to 1
        return
      end if
    end repeat
  end repeat
end tell"#,
            escaped
        )
    } else {
        r#"tell application "Terminal" to activate"#.to_string()
    };

    let _ = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output();

    Ok(())
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
        let result = focus(Some("/dev/ttys001")).await;
        assert!(result.is_ok());
    }
}
