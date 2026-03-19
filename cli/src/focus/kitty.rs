use anyhow::Result;
use std::fs;

/// Focus a kitty terminal tab or window.
pub async fn focus(window_id: Option<&str>, tab_id: Option<&str>) -> Result<()> {
    tokio::task::spawn_blocking({
        let window_id = window_id.map(String::from);
        let tab_id = tab_id.map(String::from);
        move || focus_sync(window_id.as_deref(), tab_id.as_deref())
    })
    .await??;
    Ok(())
}

fn focus_sync(window_id: Option<&str>, tab_id: Option<&str>) -> Result<()> {
    let kitten = find_kitten();
    let socket = find_kitty_socket();

    if let (Some(kitten), Some(socket)) = (&kitten, &socket) {
        if let Some(tab_id) = tab_id {
            let _ = std::process::Command::new(kitten)
                .args(["@", "--to", &format!("unix:{}", socket), "focus-tab", "--match", &format!("id:{}", tab_id)])
                .output();
        } else if let Some(window_id) = window_id {
            let _ = std::process::Command::new(kitten)
                .args(["@", "--to", &format!("unix:{}", socket), "focus-window", "--match", &format!("id:{}", window_id)])
                .output();
        }
    }

    // Always bring kitty to front
    #[cfg(target_os = "macos")]
    {
        super::activate_app_sync("kitty");
    }

    Ok(())
}

fn find_kitten() -> Option<String> {
    let paths = ["/opt/homebrew/bin/kitten", "/usr/local/bin/kitten"];
    for p in &paths {
        if std::path::Path::new(p).exists() {
            return Some(p.to_string());
        }
    }
    // Try PATH
    which("kitten")
}

fn find_kitty_socket() -> Option<String> {
    if let Ok(entries) = fs::read_dir("/tmp") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("kitty-sock") {
                return Some(format!("/tmp/{}", name));
            }
        }
    }
    None
}

fn which(name: &str) -> Option<String> {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_kitty_socket_no_panic() {
        // Should not panic even if /tmp has no kitty socket
        let _ = find_kitty_socket();
    }

    #[test]
    fn test_find_kitten_no_panic() {
        let _ = find_kitten();
    }

    #[tokio::test]
    async fn test_focus_no_panic() {
        // Should gracefully handle missing kitty
        let result = focus(None, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_focus_with_tab_id() {
        // Should not panic with a tab_id when kitty isn't running
        let result = focus(None, Some("123")).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_focus_with_window_id() {
        let result = focus(Some("42"), None).await;
        assert!(result.is_ok());
    }
}
