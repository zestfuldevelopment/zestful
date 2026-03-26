//! Shelldon focus handler.
//!
//! Reads the shelldon discovery file to get the MCP server port and auth token,
//! then sends a `focus_tab` JSON-RPC call over HTTP.

use super::ShelldonInfo;
use anyhow::{bail, Result};
use std::io::{Read, Write};
use std::net::TcpStream;

/// Focus a shelldon tab by calling the MCP server's `focus_tab` tool.
pub async fn focus(info: &ShelldonInfo) -> Result<()> {
    let tab_id = match &info.tab_id {
        Some(id) => id.clone(),
        None => return Ok(()), // No tab to focus
    };

    let session_id = info.session_id.clone();
    tokio::task::spawn_blocking(move || focus_sync(&session_id, &tab_id))
        .await??;

    Ok(())
}

fn focus_sync(session_id: &str, tab_id: &str) -> Result<()> {
    // Extract PID from session_id (format: "shelldon-{PID}-{PORT}")
    let parts: Vec<&str> = session_id.split('-').collect();
    if parts.len() < 2 {
        bail!("invalid shelldon session_id: {}", session_id);
    }
    let pid_str = parts[1];

    // Read discovery file
    let discovery_path = format!("/tmp/shelldon-{}.json", pid_str);
    let contents = std::fs::read_to_string(&discovery_path)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {}", discovery_path, e))?;

    let info: serde_json::Value = serde_json::from_str(&contents)?;
    let port = info
        .get("port")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("no port in discovery file"))? as u16;
    let token = info
        .get("auth_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("no auth_token in discovery file"))?;

    // Send focus_tab via MCP JSON-RPC
    let body = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"focus_tab","arguments":{{"tab_id":"{}"}}}}}}"#,
        tab_id
    );

    let request = format!(
        "POST /mcp HTTP/1.1\r\n\
         Host: localhost:{}\r\n\
         Content-Type: application/json\r\n\
         Authorization: Bearer {}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {}",
        port,
        token,
        body.len(),
        body
    );

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(3)))?;
    stream.write_all(request.as_bytes())?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;

    eprintln!(
        "[zestfuld] Shelldon focus_tab({}) on port {} — ok",
        tab_id, port
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_focus_no_tab_id() {
        let info = ShelldonInfo {
            session_id: "shelldon-12345-56789".into(),
            tab_id: None,
        };
        let result = focus(&info).await;
        assert!(result.is_ok());
    }
}
