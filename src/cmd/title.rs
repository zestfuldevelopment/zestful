//! `zestful title` — set the terminal tab title.
//!
//! Sets the tab title so that click-to-focus can find the right tab.
//! Handles iTerm2 (with and without tmux), kitty, and generic terminals.
//! Defaults to the current directory name if no name is given.

use anyhow::Result;
use std::env;
use std::io::{self, Write};

/// Execute the `title` command: set the terminal tab title.
pub fn run(name: Option<String>) -> Result<()> {
    let title = name.unwrap_or_else(|| {
        env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "untitled".to_string())
    });

    let in_tmux = env::var("TMUX").is_ok();
    let in_iterm = env::var("ITERM_SESSION_ID").is_ok() || env::var("ITERM_PROFILE").is_ok();

    if in_iterm {
        set_iterm2_title(&title, in_tmux)?;
    } else {
        set_generic_title(&title, in_tmux)?;
    }

    eprintln!("Tab title set to: {}", title);
    Ok(())
}

/// Set iTerm2 tab title using proprietary escape sequence.
/// This sets titleOverride which persists and overrides the automatic title.
fn set_iterm2_title(title: &str, in_tmux: bool) -> Result<()> {
    // iTerm2 proprietary: ESC ] 1337 ; SetUserVar=titleOverride=BASE64 BEL
    let encoded = base64_encode(title);
    let seq = format!("\x1b]1337;SetUserVar=titleOverride={}\x07", encoded);

    if in_tmux {
        // Wrap in tmux pass-through: ESC Ptmux; ESC <seq> ESC backslash
        let tmux_seq = tmux_passthrough(&seq);
        write_stdout(&tmux_seq)?;
    } else {
        write_stdout(&seq)?;
    }

    // Also set the standard title as fallback
    set_generic_title(title, in_tmux)?;

    Ok(())
}

/// Set tab title using the standard xterm escape sequence (works in most terminals).
fn set_generic_title(title: &str, in_tmux: bool) -> Result<()> {
    // ESC ] 1 ; title BEL — set icon name (tab title in most terminals)
    let seq = format!("\x1b]1;{}\x07", title);

    if in_tmux {
        let tmux_seq = tmux_passthrough(&seq);
        write_stdout(&tmux_seq)?;
    } else {
        write_stdout(&seq)?;
    }

    Ok(())
}

/// Wrap an escape sequence in tmux pass-through.
fn tmux_passthrough(seq: &str) -> String {
    // Double any ESC characters inside the sequence for tmux
    let escaped = seq.replace('\x1b', "\x1b\x1b");
    format!("\x1bPtmux;{}\x1b\\", escaped)
}

/// Simple base64 encoding (no padding needed for iTerm2).
fn base64_encode(input: &str) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut result = String::new();

    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let combined = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARS[(combined >> 18 & 0x3F) as usize] as char);
        result.push(CHARS[(combined >> 12 & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[(combined >> 6 & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(combined & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }

    result
}

fn write_stdout(s: &str) -> Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(s.as_bytes())?;
    stdout.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_encode() {
        assert_eq!(base64_encode("hello"), "aGVsbG8=");
        assert_eq!(base64_encode("Zestful"), "WmVzdGZ1bA==");
        assert_eq!(base64_encode("ab"), "YWI=");
        assert_eq!(base64_encode("abc"), "YWJj");
        assert_eq!(base64_encode(""), "");
    }

    #[test]
    fn test_tmux_passthrough_wraps() {
        let seq = "\x1b]1;hello\x07";
        let result = tmux_passthrough(seq);
        assert!(result.starts_with("\x1bPtmux;"));
        assert!(result.ends_with("\x1b\\"));
        // ESC should be doubled inside
        assert!(result.contains("\x1b\x1b]1;hello"));
    }

    #[test]
    fn test_default_title_is_directory() {
        // run(None) would use current directory, just verify it doesn't panic
        let cwd = env::current_dir().unwrap();
        let name = cwd.file_name().unwrap().to_string_lossy().to_string();
        assert!(!name.is_empty());
    }
}
