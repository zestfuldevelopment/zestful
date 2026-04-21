//! Detect which AI agent invoked `zestful hook`.
//!
//! Detection priority:
//! 1. Explicit `--agent` override
//! 2. JSON schema sniff — unique field names the agent wrote into its payload
//! 3. Well-known env vars set by each agent (e.g. `CLAUDE_PROJECT_DIR`)
//! 4. Parent-process walk — look for a known agent binary in our ancestry
//! 5. Fallback: `AgentKind::Generic`
//!
//! Schema sniff comes before parent-process walk because the JSON payload is
//! authored by the invoking agent itself. Parent-process matching is fuzzy —
//! e.g. Cursor ships helper binaries whose basenames happen to match `claude`.

use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    ClaudeCode,
    CodexCli,
    CopilotCli,
    Cline,
    Aider,
    Cursor,
    GeminiCli,
    Generic,
}

impl AgentKind {
    /// Canonical slug used in the `agent:` field of a Zestful notification,
    /// e.g. "claude-code:myproject".
    pub fn slug(self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude-code",
            AgentKind::CodexCli => "codex",
            AgentKind::CopilotCli => "copilot",
            AgentKind::Cline => "cline",
            AgentKind::Aider => "aider",
            AgentKind::Cursor => "cursor",
            AgentKind::GeminiCli => "gemini-cli",
            AgentKind::Generic => "agent",
        }
    }
}

pub fn detect_agent(override_kind: Option<&str>, payload: &serde_json::Value) -> AgentKind {
    if let Some(s) = override_kind {
        if let Some(kind) = from_slug(s) {
            return kind;
        }
    }
    if let Some(kind) = detect_by_schema(payload) {
        return kind;
    }
    if let Some(kind) = detect_by_env() {
        return kind;
    }
    if let Some(kind) = detect_by_parent_process() {
        return kind;
    }
    AgentKind::Generic
}

fn from_slug(slug: &str) -> Option<AgentKind> {
    match slug.to_ascii_lowercase().as_str() {
        "claude-code" | "claudecode" | "claude" => Some(AgentKind::ClaudeCode),
        "codex" | "codex-cli" | "codexcli" => Some(AgentKind::CodexCli),
        "copilot" | "copilot-cli" => Some(AgentKind::CopilotCli),
        "cline" => Some(AgentKind::Cline),
        "aider" => Some(AgentKind::Aider),
        "cursor" => Some(AgentKind::Cursor),
        "gemini" | "gemini-cli" => Some(AgentKind::GeminiCli),
        _ => None,
    }
}

/// Known env vars each agent sets before firing a hook.
fn detect_by_env() -> Option<AgentKind> {
    if std::env::var_os("CLAUDE_PROJECT_DIR").is_some()
        || std::env::var_os("CLAUDE_CODE_SESSION_ID").is_some()
    {
        return Some(AgentKind::ClaudeCode);
    }
    if std::env::var_os("CODEX_SESSION_ID").is_some()
        || std::env::var_os("OPENAI_CODEX_SESSION").is_some()
    {
        return Some(AgentKind::CodexCli);
    }
    if std::env::var_os("CURSOR_AGENT_SESSION").is_some() {
        return Some(AgentKind::Cursor);
    }
    if std::env::var_os("AIDER_SESSION").is_some() {
        return Some(AgentKind::Aider);
    }
    None
}

/// Walk ancestor processes and match their `comm` against known binaries.
/// Returns the first hit; returns None if we hit init / loop limit with no match.
fn detect_by_parent_process() -> Option<AgentKind> {
    let mut current = std::process::id();
    for _ in 0..30 {
        let output = Command::new("ps")
            .args(["-p", &current.to_string(), "-o", "ppid=,comm="])
            .output()
            .ok()?;
        let line = String::from_utf8_lossy(&output.stdout);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        // ps output: `<ppid> <comm>`; comm may itself contain path/args.
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let ppid_str = parts.next()?;
        let comm = parts.next().unwrap_or("").trim();
        if let Some(kind) = match_process_name(comm) {
            return Some(kind);
        }
        let ppid: u32 = ppid_str.parse().ok()?;
        if ppid == 0 || ppid == 1 || ppid == current {
            return None;
        }
        current = ppid;
    }
    None
}

fn match_process_name(comm: &str) -> Option<AgentKind> {
    // comm can be a path like "/usr/local/bin/claude" — grab the basename.
    let basename = comm.rsplit('/').next().unwrap_or(comm).to_ascii_lowercase();
    match basename.as_str() {
        "claude" => Some(AgentKind::ClaudeCode),
        "codex" | "codex-cli" => Some(AgentKind::CodexCli),
        "copilot" | "gh-copilot" => Some(AgentKind::CopilotCli),
        "cline" => Some(AgentKind::Cline),
        "aider" => Some(AgentKind::Aider),
        "cursor" | "cursor-cli" | "cursor-agent" => Some(AgentKind::Cursor),
        "gemini" => Some(AgentKind::GeminiCli),
        _ => None,
    }
}

/// Last-resort: look for unique keys in the JSON payload.
///
/// All three of Cursor / Codex / Claude Code emit `transcript_path` +
/// `hook_event_name`, so we need stronger discriminators before the fallback:
///
/// - Cursor: `cursor_version`, `composer_mode`, `workspace_roots`.
/// - Codex CLI: top-level `model` string + `turn_id` on turn-scoped events.
///   `transcript_path` (when present) lives under `~/.codex/`.
/// - Claude Code: `permission_mode`, `tool_use_id`, or `transcript_path`
///   under `~/.claude/`.
fn detect_by_schema(payload: &serde_json::Value) -> Option<AgentKind> {
    let obj = payload.as_object()?;
    if obj.contains_key("cursor_version")
        || obj.contains_key("composer_mode")
        || obj.contains_key("workspace_roots")
    {
        return Some(AgentKind::Cursor);
    }
    // `transcript_path` prefix is the strongest discriminator between Codex
    // and Claude Code — both payloads carry `permission_mode` and
    // `hook_event_name`, so neither is on its own a reliable signal.
    if let Some(path) = obj.get("transcript_path").and_then(|v| v.as_str()) {
        if path.contains("/.codex/") {
            return Some(AgentKind::CodexCli);
        }
        if path.contains("/.claude/") {
            return Some(AgentKind::ClaudeCode);
        }
    }
    // Codex tags every turn-scoped event with `turn_id`; Claude Code uses
    // `tool_use_id` on tool events instead.
    if obj.contains_key("turn_id") {
        return Some(AgentKind::CodexCli);
    }
    if obj.contains_key("tool_use_id") {
        return Some(AgentKind::ClaudeCode);
    }
    // Codex always sets top-level `model`; Claude Code does not.
    if obj.contains_key("model") && obj.contains_key("hook_event_name") {
        return Some(AgentKind::CodexCli);
    }
    if obj.contains_key("transcript_path") && obj.contains_key("hook_event_name") {
        return Some(AgentKind::ClaudeCode);
    }
    None
}
