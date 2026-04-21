//! Event-name → notification policy mapping per agent.
//!
//! Given the detected agent kind and the JSON payload the agent sent, return
//! the severity / message / push flag to use for the notification. Unknown
//! events fall back to a generic "activity" notification so we never go silent.

use crate::hooks::AgentKind;

#[derive(Debug, Clone)]
pub struct Policy {
    pub severity: Severity,
    pub message: String,
    pub push: bool,
    /// If true, skip sending the notification (e.g. redundant "Stop" after a
    /// click-through). Nothing sets this today, kept for future tuning.
    pub skip: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Urgent,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Urgent => "urgent",
        }
    }
}

/// Resolve the policy for this agent + event. `payload` is the parsed JSON
/// the agent wrote to stdin; may be `Null` if parsing failed.
pub fn resolve(agent: AgentKind, payload: &serde_json::Value) -> Policy {
    let event = payload
        .get("hook_event_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match agent {
        AgentKind::ClaudeCode => claude_code(event, payload),
        AgentKind::Cursor => cursor(event, payload),
        AgentKind::CodexCli => codex(event, payload),
        _ => generic(event),
    }
}

fn codex(event: &str, payload: &serde_json::Value) -> Policy {
    match event {
        "Stop" => Policy {
            severity: Severity::Warning,
            message: "Waiting for your input".into(),
            push: true,
            skip: false,
        },
        "UserPromptSubmit" => {
            let prompt = payload
                .get("prompt")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let message = if prompt.is_empty() {
                "Working...".into()
            } else {
                let preview: String = prompt.chars().take(80).collect();
                format!("Working on: {}", preview)
            };
            Policy { severity: Severity::Info, message, push: false, skip: false }
        }
        "PreToolUse" => {
            let tool = payload
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("tool");
            Policy {
                severity: Severity::Info,
                message: format!("Using {}", tool),
                push: false,
                skip: false,
            }
        }
        // SessionStart / PostToolUse: don't surface every tick.
        "SessionStart" | "PostToolUse" => Policy {
            severity: Severity::Info,
            message: String::new(),
            push: false,
            skip: true,
        },
        _ => generic(event),
    }
}

fn cursor(event: &str, payload: &serde_json::Value) -> Policy {
    match event {
        "stop" => Policy {
            severity: Severity::Warning,
            message: "Waiting for your input".into(),
            push: true,
            skip: false,
        },
        "beforeSubmitPrompt" => {
            let prompt = payload
                .get("prompt")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let message = if prompt.is_empty() {
                "Working...".into()
            } else {
                let preview: String = prompt.chars().take(80).collect();
                format!("Working on: {}", preview)
            };
            Policy {
                severity: Severity::Info,
                message,
                push: false,
                skip: false,
            }
        }
        "beforeShellExecution" => Policy {
            severity: Severity::Info,
            message: "Running shell".into(),
            push: false,
            skip: false,
        },
        "beforeMCPExecution" => {
            let tool = payload
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("MCP tool");
            Policy {
                severity: Severity::Info,
                message: format!("Using {}", tool),
                push: false,
                skip: false,
            }
        }
        // Chatty events we don't want to surface on every tick.
        "beforeReadFile" | "afterFileEdit" => Policy {
            severity: Severity::Info,
            message: String::new(),
            push: false,
            skip: true,
        },
        _ => generic(event),
    }
}

fn claude_code(event: &str, payload: &serde_json::Value) -> Policy {
    match event {
        "Stop" | "SubagentStop" => Policy {
            severity: Severity::Warning,
            message: "Waiting for your input".into(),
            push: true,
            skip: false,
        },
        "Notification" => Policy {
            severity: Severity::Warning,
            message: "Needs attention".into(),
            push: true,
            skip: false,
        },
        "UserPromptSubmit" => {
            let prompt = payload
                .get("prompt")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let message = if prompt.is_empty() {
                "Working...".into()
            } else {
                let preview: String = prompt.chars().take(80).collect();
                format!("Working on: {}", preview)
            };
            Policy {
                severity: Severity::Info,
                message,
                push: false,
                skip: false,
            }
        }
        "PreToolUse" => {
            let tool = payload
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("tool");
            Policy {
                severity: Severity::Info,
                message: format!("Using {}", tool),
                push: false,
                skip: false,
            }
        }
        "PermissionRequest" => Policy {
            severity: Severity::Warning,
            message: "Waiting for permission".into(),
            push: true,
            skip: false,
        },
        "Elicitation" => Policy {
            severity: Severity::Warning,
            message: "Waiting for input".into(),
            push: true,
            skip: false,
        },
        _ => generic(event),
    }
}

fn generic(event: &str) -> Policy {
    // Unknown agent + unknown event: still say *something*. Treat as info
    // so we don't spam pushes.
    let message = if event.is_empty() {
        "Agent activity".into()
    } else {
        format!("Event: {}", event)
    };
    Policy {
        severity: Severity::Info,
        message,
        push: false,
        skip: false,
    }
}
