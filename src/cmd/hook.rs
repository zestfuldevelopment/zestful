//! `zestful hook` — the universal agent hook entry point.
//!
//! Reads a JSON payload on stdin (in whatever schema the invoking agent
//! provides), detects which agent kind is calling us, maps the event to a
//! severity / message / push policy, and sends the notification via the
//! same path as `zestful notify`.

use anyhow::Result;
use std::io::Read;
use std::path::Path;

/// Execute the `hook` subcommand.
pub fn run(agent_override: Option<String>) -> Result<()> {
    // Read all of stdin so we can log and parse.
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw)?;

    let preview: String = raw.chars().take(500).collect();
    crate::log::log("hook", &format!("stdin ({} bytes): {}", raw.len(), preview));

    if let Some(agent) = agent_override.as_deref() {
        crate::log::log("hook", &format!("--agent override: {}", agent));
    }

    let payload: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|e| {
            crate::log::log("hook", &format!("JSON parse error: {}", e));
            serde_json::Value::Null
        });

    let agent_kind = crate::hooks::detect_agent(agent_override.as_deref(), &payload);
    let policy = crate::hooks::resolve_policy(agent_kind, &payload);
    crate::log::log("hook", &format!(
        "resolved: agent={:?} event={} → severity={} msg={:?} push={} skip={}",
        agent_kind,
        payload.get("hook_event_name").and_then(|v| v.as_str()).unwrap_or(""),
        policy.severity.as_str(),
        policy.message,
        policy.push,
        policy.skip,
    ));

    if policy.skip {
        return Ok(());
    }

    // Compose the agent identifier: `<slug>:<project>` where project is the
    // basename of the payload's cwd. Cursor sends `workspace_roots[0]` instead
    // of `cwd`; fall back to that, then to our own PWD.
    let project_from_path = |p: &str| -> Option<String> {
        Path::new(p)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty())
    };
    let project = payload
        .get("cwd")
        .and_then(|v| v.as_str())
        .and_then(project_from_path)
        .or_else(|| {
            payload
                .get("workspace_roots")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
                .and_then(project_from_path)
        })
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
        })
        .unwrap_or_default();
    let agent_name = if project.is_empty() {
        agent_kind.slug().to_string()
    } else {
        format!("{}:{}", agent_kind.slug(), project)
    };

    // Locate where we are (terminal/IDE/browser). If `locate()` can't match
    // (common when the agent's hook subprocess isn't a child of the editor
    // process we know about — e.g. Cursor's AI agent), fall back to a
    // project-level URI synthesized from the payload for IDE-family agents.
    let terminal_uri = crate::workspace::locate().ok().or_else(|| {
        let editor_slug = match agent_kind {
            crate::hooks::AgentKind::Cursor => Some("cursor"),
            _ => None,
        };
        match (editor_slug, project.as_str()) {
            (Some(slug), p) if !p.is_empty() => Some(format!("workspace://{}/project:{}", slug, p)),
            _ => None,
        }
    });

    let token = crate::config::read_token().ok_or_else(|| {
        anyhow::anyhow!("Zestful app not running or not configured. Token not found.")
    })?;
    let port = crate::config::read_port();

    crate::log::log("hook", &format!(
        "notify: agent={} message={:?} severity={} uri={} push={}",
        agent_name,
        policy.message,
        policy.severity.as_str(),
        terminal_uri.as_deref().unwrap_or("none"),
        policy.push,
    ));

    crate::cmd::notify::send(
        &token,
        port,
        &agent_name,
        &policy.message,
        policy.severity.as_str(),
        terminal_uri,
        !policy.push,
    )?;

    Ok(())
}
