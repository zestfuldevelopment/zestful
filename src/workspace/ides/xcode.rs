//! Xcode project detection via AppleScript.

use anyhow::Result;
use std::process::Command;

use crate::workspace::types::{IdeInstance, IdeProject};

pub fn detect() -> Result<Option<IdeInstance>> {
    let pid = get_xcode_pid();
    if pid.is_none() {
        return Ok(None);
    }

    let output = Command::new("osascript")
        .args([
            "-e",
            r#"tell application "Xcode"
  set AppleScript's text item delimiters to linefeed
  set docNames to name of every workspace document
  set docPaths to path of every workspace document
  try
    set activeDoc to path of active workspace document
  on error
    set activeDoc to ""
  end try
  return (docNames as text) & "|||" & (docPaths as text) & "|||" & activeDoc
end tell"#,
        ])
        .output()?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = stdout.trim().splitn(3, "|||").collect();
    if parts.len() < 3 {
        return Ok(None);
    }

    let names: Vec<&str> = parts[0].split('\n').collect();
    let paths: Vec<&str> = parts[1].split('\n').collect();
    let active_path = parts[2].trim();

    if names.len() != paths.len() {
        return Ok(None);
    }

    let projects: Vec<IdeProject> = names
        .iter()
        .zip(paths.iter())
        .map(|(name, path)| {
            let clean_name = name
                .trim_end_matches(".xcodeproj")
                .trim_end_matches(".xcworkspace")
                .to_string();
            IdeProject {
                name: clean_name,
                uri: None,
                path: path.to_string(),
                active: *path == active_path,
            }
        })
        .collect();

    Ok(Some(IdeInstance {
        app: "Xcode".to_string(),
        pid,
        projects,
    }))
}

fn get_xcode_pid() -> Option<u32> {
    let output = Command::new("pgrep")
        .args(["-x", "Xcode"])
        .output()
        .ok()?;

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
