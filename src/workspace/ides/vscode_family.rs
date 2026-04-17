//! Detection for VS Code and its forks (Cursor, Windsurf, etc.).
//!
//! Strategy: each open workspace window keeps a `state.vscdb` file open under
//! `~/Library/Application Support/<App>/User/workspaceStorage/<hash>/`. The
//! sibling `workspace.json` records the folder URI. We use `lsof` against
//! the running app's PID to find which workspace storage dirs are *currently*
//! open, then read each `workspace.json` to extract the project path.
//!
//! This avoids needing Accessibility / System Events permission.

use anyhow::Result;
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::workspace::types::{IdeInstance, IdeProject};

struct AppSpec {
    process_name: &'static str,
    support_dir: &'static str,  // relative to ~/Library/Application Support/
    display: &'static str,
}

const APPS: &[AppSpec] = &[
    AppSpec { process_name: "Code",     support_dir: "Code",     display: "Visual Studio Code" },
    AppSpec { process_name: "Cursor",   support_dir: "Cursor",   display: "Cursor" },
    AppSpec { process_name: "Windsurf", support_dir: "Windsurf", display: "Windsurf" },
];

pub fn detect_all() -> Result<Vec<IdeInstance>> {
    let mut out = Vec::new();
    for spec in APPS {
        if let Some(instance) = detect_one(spec) {
            out.push(instance);
        }
    }
    Ok(out)
}

fn detect_one(spec: &AppSpec) -> Option<IdeInstance> {
    let pid = pgrep_first(spec.process_name)?;
    let storage_root = home_dir()?
        .join("Library/Application Support")
        .join(spec.support_dir)
        .join("User/workspaceStorage");

    let open_dirs = lsof_workspace_dirs(pid, &storage_root);
    let projects: Vec<IdeProject> = open_dirs
        .iter()
        .filter_map(|dir| read_workspace_project(dir))
        .collect();

    Some(IdeInstance {
        app: spec.display.to_string(),
        pid: Some(pid),
        projects,
    })
}

/// Use `lsof` to find every workspace storage directory currently held open
/// by the given app PID.
fn lsof_workspace_dirs(pid: u32, storage_root: &PathBuf) -> Vec<PathBuf> {
    let output = Command::new("lsof").args(["-p", &pid.to_string()]).output();
    let stdout = match output {
        Ok(o) if o.status.success() || !o.stdout.is_empty() => {
            String::from_utf8_lossy(&o.stdout).into_owned()
        }
        _ => return vec![],
    };

    let prefix = storage_root.to_string_lossy().to_string() + "/";
    let mut seen = HashSet::new();
    let mut dirs = Vec::new();
    for line in stdout.lines() {
        if let Some(idx) = line.find(&prefix) {
            let rest = &line[idx + prefix.len()..];
            // Hash directory is the segment up to the next "/"
            let hash = rest.split('/').next().unwrap_or("");
            if !hash.is_empty() && seen.insert(hash.to_string()) {
                dirs.push(storage_root.join(hash));
            }
        }
    }
    dirs
}

#[derive(Deserialize)]
struct WorkspaceFile {
    folder: Option<String>,
    workspace: Option<String>,
}

/// Read `<dir>/workspace.json` and return an IdeProject for its folder/workspace path.
fn read_workspace_project(dir: &PathBuf) -> Option<IdeProject> {
    let path = dir.join("workspace.json");
    let contents = fs::read_to_string(&path).ok()?;
    let parsed: WorkspaceFile = serde_json::from_str(&contents).ok()?;
    let uri = parsed.folder.or(parsed.workspace)?;
    let local = uri.strip_prefix("file://").unwrap_or(&uri);
    let decoded = urlencoding_decode(local);
    let name = PathBuf::from(&decoded)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    if name.is_empty() {
        return None;
    }
    Some(IdeProject {
        name,
        uri: None,
        path: decoded,
        active: false,  // can't tell without UI scripting
    })
}

fn pgrep_first(name: &str) -> Option<u32> {
    let output = Command::new("pgrep").args(["-x", name]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .lines()
        .next()?
        .parse()
        .ok()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Minimal percent-decode for file:// URIs (just spaces and common chars).
fn urlencoding_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::urlencoding_decode;

    #[test]
    fn decodes_spaces() {
        assert_eq!(urlencoding_decode("/foo%20bar"), "/foo bar");
    }

    #[test]
    fn passes_through_plain() {
        assert_eq!(urlencoding_decode("/foo/bar"), "/foo/bar");
    }
}
