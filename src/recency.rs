use crate::models::Provider;
use serde::Deserialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Only the end of a history file matters; both tools append to them forever.
const HISTORY_TAIL_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Deserialize)]
struct HistoryEntry {
    #[serde(default)]
    display: String,
    timestamp: i64,
}

fn read_tail(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(HISTORY_TAIL_BYTES))).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).to_string())
}

/// Finds the newest prompt timestamp (ms) in JSONL history text. Slash commands such as
/// `/model` are skipped because they are not prompts.
fn last_prompt_in_history(text: &str) -> Option<i64> {
    text.lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<HistoryEntry>(line).ok())
        .find(|entry| !entry.display.trim_start().starts_with('/'))
        .map(|entry| entry.timestamp)
}

/// Antigravity IDE prompts never reach history.jsonl, but they rewrite a conversation file.
fn newest_mtime_ms(dir: &Path) -> Option<i64> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.metadata().ok()?.modified().ok())
        .filter_map(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|age| age.as_millis() as i64)
        .max()
}

fn choose(claude_last: Option<i64>, antigravity_last: Option<i64>) -> Provider {
    match (claude_last, antigravity_last) {
        (Some(claude), Some(antigravity)) if claude > antigravity => Provider::Claude,
        (Some(_), None) => Provider::Claude,
        _ => Provider::Antigravity,
    }
}

/// Picks whichever tool received a prompt most recently.
pub fn most_recent_provider() -> Provider {
    if crate::cli_source::find_binary("claude").is_none() {
        return Provider::Antigravity;
    }
    let Ok(home) = std::env::var("HOME").map(PathBuf::from) else {
        return Provider::Antigravity;
    };

    let claude_last = read_tail(&home.join(".claude/history.jsonl"))
        .as_deref()
        .and_then(last_prompt_in_history);
    let antigravity_cli_last = read_tail(&home.join(".gemini/antigravity-cli/history.jsonl"))
        .as_deref()
        .and_then(last_prompt_in_history);
    let antigravity_ide_last = newest_mtime_ms(&home.join(".gemini/antigravity/conversations"));

    choose(claude_last, antigravity_cli_last.max(antigravity_ide_last))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_prompt_wins() {
        assert_eq!(choose(Some(200), Some(100)), Provider::Claude);
        assert_eq!(choose(Some(100), Some(200)), Provider::Antigravity);
    }

    #[test]
    fn missing_history_loses_to_any_prompt() {
        assert_eq!(choose(Some(1), None), Provider::Claude);
        assert_eq!(choose(None, Some(1)), Provider::Antigravity);
        assert_eq!(choose(None, None), Provider::Antigravity);
    }

    #[test]
    fn slash_commands_do_not_count_as_prompts() {
        let history = "{\"display\":\"fix the bug\",\"timestamp\":100}\n\
            {\"display\":\"/model\",\"timestamp\":200}\n";
        assert_eq!(last_prompt_in_history(history), Some(100));
    }

    #[test]
    fn a_truncated_first_line_is_skipped() {
        let history = "nt\",\"timestamp\":50}\n{\"display\":\"hello\",\"timestamp\":100}\n";
        assert_eq!(last_prompt_in_history(history), Some(100));
        assert_eq!(last_prompt_in_history("nt\",\"timestamp\":50}\n"), None);
    }
}
