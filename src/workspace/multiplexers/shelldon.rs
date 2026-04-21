//! Shelldon detection and focus handler.

use anyhow::{bail, Result};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::Command;

use crate::workspace::types::{ShelldonInstance, ShelldonPane, ShelldonTab};
use crate::workspace::uri::ShelldonInfo;

/// Discovery file written by each shelldon instance.
#[derive(serde::Deserialize)]
struct DiscoveryInfo {
    pid: u32,
    port: u16,
    auth_token: String,
    session_id: String,
}

pub fn detect() -> Result<Vec<ShelldonInstance>> {
    let entries = std::fs::read_dir("/tmp")?;
    let mut instances = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("shelldon-") || !name.ends_with(".json") {
            continue;
        }

        let contents = match std::fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let info: DiscoveryInfo = match serde_json::from_str(&contents) {
            Ok(i) => i,
            Err(_) => continue,
        };

        if !process_alive(info.pid) {
            continue;
        }

        let tty = get_tty(info.pid);
        let panes = query_panes(&info).unwrap_or_default();

        instances.push(ShelldonInstance {
            pid: info.pid,
            port: info.port,
            session_id: info.session_id,
            tty,
            panes,
        });
    }

    Ok(instances)
}

fn process_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn get_tty(pid: u32) -> Option<String> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "tty="])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let tty = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if tty.is_empty() || tty == "??" {
        None
    } else {
        Some(format!("/dev/{}", tty))
    }
}

fn query_panes(info: &DiscoveryInfo) -> Result<Vec<ShelldonPane>> {
    let panes_json = mcp_call(info, "list_panes", "{}")?;
    let tabs_json = mcp_call(info, "list_tabs", "{}")?;

    let panes_raw: Vec<serde_json::Value> = serde_json::from_str(&panes_json)?;
    let tabs_raw: Vec<serde_json::Value> = serde_json::from_str(&tabs_json)?;

    let mut panes = Vec::new();

    for p in &panes_raw {
        let pane_id = p.get("pane_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let name = p
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let is_focused = p
            .get("is_focused")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let tabs: Vec<ShelldonTab> = tabs_raw
            .iter()
            .filter(|t| {
                t.get("pane_id")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(u64::MAX)
                    == pane_id as u64
            })
            .flat_map(|t| {
                t.get("tabs")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default()
            })
            .map(|t| ShelldonTab {
                uri: None,
                tab_id: t
                    .get("tab_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                title: t
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                pane_type: t
                    .get("pane_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                is_active: t
                    .get("is_active")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            })
            .collect();

        panes.push(ShelldonPane {
            pane_id,
            name,
            is_focused,
            tabs,
        });
    }

    Ok(panes)
}

/// Make a JSON-RPC call to a shelldon MCP server over raw TCP + HTTP/1.1.
fn mcp_call(info: &DiscoveryInfo, method: &str, args: &str) -> Result<String> {
    let body = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"{}","arguments":{}}}}}"#,
        method, args
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
        info.port,
        info.auth_token,
        body.len(),
        body
    );

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", info.port))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    stream.write_all(request.as_bytes())?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;

    let response = String::from_utf8_lossy(&response);

    let body = response.split("\r\n\r\n").nth(1).unwrap_or("");

    let rpc: serde_json::Value = serde_json::from_str(body)?;
    let text = rpc
        .get("result")
        .and_then(|r| r.get("content"))
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("[]");

    Ok(text.to_string())
}

/// Focus a shelldon tab by calling the MCP server's `focus_tab` tool.
pub async fn focus(info: &ShelldonInfo) -> Result<()> {
    let tab_id = match &info.tab_id {
        Some(id) => id.clone(),
        None => return Ok(()),
    };

    let session_id = info.session_id.clone();
    tokio::task::spawn_blocking(move || focus_sync(&session_id, &tab_id)).await??;

    Ok(())
}

fn focus_sync(session_id: &str, tab_id: &str) -> Result<()> {
    let parts: Vec<&str> = session_id.split('-').collect();
    if parts.len() < 2 {
        bail!("invalid shelldon session_id: {}", session_id);
    }
    let pid_str = parts[1];

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

    crate::log::log(
        "daemon",
        &format!("shelldon focus_tab({}) on port {} — ok", tab_id, port),
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
