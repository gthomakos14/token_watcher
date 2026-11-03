use crate::models::{ModelQuota, PlanInfo, Provider, TokenReport, UserInfo};
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Serialize, Deserialize)]
struct CachedReport {
    timestamp: i64,
    report: TokenReport,
}

#[derive(Debug, Deserialize)]
struct AgyUsageResponse {
    command: Option<AgyUsageCommand>,
}

#[derive(Debug, Deserialize)]
struct AgyUsageCommand {
    #[allow(dead_code)]
    name: Option<String>,
    data: Option<AgyUsageData>,
}

#[derive(Debug, Deserialize)]
struct AgyUsageData {
    groups: Option<Vec<AgyGroup>>,
}

#[derive(Debug, Deserialize)]
struct AgyGroup {
    name: String,
    #[allow(dead_code)]
    description: Option<String>,
    buckets: Option<Vec<AgyBucket>>,
}

#[derive(Debug, Deserialize)]
struct AgyBucket {
    #[allow(dead_code)]
    id: Option<String>,
    #[allow(dead_code)]
    name: Option<String>,
    #[allow(dead_code)]
    description: Option<String>,
    #[allow(dead_code)]
    window: Option<String>,
    remaining_fraction: Option<f32>,
    reset_time: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgyModelResponse {
    command: Option<AgyModelCommand>,
}

#[derive(Debug, Deserialize)]
struct AgyModelCommand {
    #[allow(dead_code)]
    name: Option<String>,
    data: Option<AgyModelData>,
}

#[derive(Debug, Deserialize)]
struct AgyModelData {
    id: Option<String>,
    label: Option<String>,
    #[allow(dead_code)]
    effort: Option<String>,
    #[allow(dead_code)]
    is_default: Option<bool>,
}

pub fn find_binary(name: &str) -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(&home).join(".local/bin").join(name);
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let p = dir.join(name);
            if p.exists() {
                return Some(p);
            }
        }
    }
    let p = PathBuf::from("/usr/local/bin").join(name);
    if p.exists() {
        return Some(p);
    }
    let p = PathBuf::from("/usr/bin").join(name);
    if p.exists() {
        return Some(p);
    }
    None
}

fn run_agy_json(agy_path: &Path, slash_cmd: &str) -> Result<String, String> {
    let output = Command::new(agy_path)
        .arg("-p")
        .arg(slash_cmd)
        .arg("--output-format")
        .arg("json")
        .output()
        .map_err(|e| format!("Failed to execute agy {}: {}", slash_cmd, e))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("agy {} exited with error: {}", slash_cmd, err));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Resolves the cache directory from `XDG_CACHE_HOME` and `HOME`. Per the XDG spec, an
/// empty `XDG_CACHE_HOME` counts as unset.
fn cache_dir(xdg_cache_home: Option<String>, home: Option<String>) -> Result<PathBuf, String> {
    if let Some(xdg) = xdg_cache_home.filter(|x| !x.is_empty()) {
        Ok(PathBuf::from(xdg).join("antigravity-token-watcher"))
    } else if let Some(home) = home {
        Ok(PathBuf::from(home).join(".cache").join("antigravity-token-watcher"))
    } else {
        Err("Neither XDG_CACHE_HOME nor HOME is set".to_string())
    }
}

fn cache_file_name(provider: Provider) -> &'static str {
    match provider {
        Provider::Antigravity => "cache.json",
        Provider::Claude => "claude-cache.json",
    }
}

pub fn cache_path(provider: Provider) -> Result<PathBuf, String> {
    let dir = cache_dir(std::env::var("XDG_CACHE_HOME").ok(), std::env::var("HOME").ok())?;
    Ok(dir.join(cache_file_name(provider)))
}

pub fn read_cached_report(provider: Provider, max_age_secs: u64) -> Option<TokenReport> {
    read_cached_report_at(&cache_path(provider).ok()?, max_age_secs, Utc::now().timestamp())
}

fn read_cached_report_at(path: &Path, max_age_secs: u64, now: i64) -> Option<TokenReport> {
    let data = std::fs::read_to_string(path).ok()?;
    let cached: CachedReport = serde_json::from_str(&data).ok()?;
    if (now - cached.timestamp).abs() <= max_age_secs as i64 {
        Some(cached.report)
    } else {
        None
    }
}

pub fn write_cached_report(report: &TokenReport) -> Result<(), String> {
    write_cached_report_at(&cache_path(report.provider)?, report, Utc::now().timestamp())
}

fn write_cached_report_at(path: &Path, report: &TokenReport, now: i64) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let cached = CachedReport {
        timestamp: now,
        report: report.clone(),
    };
    let json = serde_json::to_string_pretty(&cached).map_err(|e| e.to_string())?;
    // Write to a private temp file and rename it into place, so a concurrent run never
    // reads a half-written cache. Rename is atomic within the same directory.
    static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("json.{}.{}.tmp", std::process::id(), seq));
    if let Err(e) = std::fs::write(&tmp, json).and_then(|()| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    Ok(())
}

pub fn fetch_token_report_from_cli() -> Result<TokenReport, String> {
    let agy_bin = find_binary("agy").ok_or_else(|| "agy CLI binary not found in PATH or ~/.local/bin/agy".to_string())?;

    // Concurrently fetch /usage and /model to minimize latency
    let bin1 = agy_bin.clone();
    let usage_handle = std::thread::spawn(move || run_agy_json(&bin1, "/usage"));

    let bin2 = agy_bin;
    let model_handle = std::thread::spawn(move || run_agy_json(&bin2, "/model"));

    let usage_raw = usage_handle
        .join()
        .map_err(|_| "Usage worker thread panicked".to_string())??;
    let model_raw = model_handle
        .join()
        .map_err(|_| "Model worker thread panicked".to_string())??;

    let usage_resp: AgyUsageResponse = serde_json::from_str(&usage_raw)
        .map_err(|e| format!("Failed to parse agy /usage JSON: {}", e))?;
    let model_resp: AgyModelResponse = serde_json::from_str(&model_raw)
        .map_err(|e| format!("Failed to parse agy /model JSON: {}", e))?;

    let active_model_label = model_resp
        .command
        .as_ref()
        .and_then(|c| c.data.as_ref())
        .and_then(|d| d.label.clone());

    let active_model_id_str = model_resp
        .command
        .as_ref()
        .and_then(|c| c.data.as_ref())
        .and_then(|d| d.id.clone())
        .unwrap_or_default()
        .to_lowercase();

    let groups = usage_resp
        .command
        .and_then(|c| c.data)
        .and_then(|d| d.groups)
        .unwrap_or_default();

    let mut models = Vec::new();
    let mut selected_model_id = None;

    for (idx, g) in groups.into_iter().enumerate() {
        let bucket = g.buckets.as_ref().and_then(|b| b.first());
        let fraction = bucket.and_then(|b| b.remaining_fraction).unwrap_or(1.0);
        let remaining_pct = ((fraction * 100.0) * 10.0).round() / 10.0;
        let used_pct = ((100.0 - remaining_pct) * 10.0).round() / 10.0;

        let mut reset_timestamp = None;
        let mut reset_time_str = None;
        let mut time_until_reset = None;

        if let Some(b) = bucket {
            if let Some(rt) = &b.reset_time {
                if let Ok(dt) = DateTime::parse_from_rfc3339(rt) {
                    let ts = dt.timestamp();
                    reset_timestamp = Some(ts);
                    let local_dt: DateTime<Local> = DateTime::from(dt);
                    reset_time_str = Some(local_dt.format("%Y-%m-%d %H:%M:%S").to_string());
                    time_until_reset = Some(crate::parser::format_duration_until(ts));
                }
            }
        }

        let is_gemini = g.name.to_lowercase().contains("gemini");
        let is_3p = g.name.to_lowercase().contains("claude") || g.name.to_lowercase().contains("gpt");

        let is_selected = if is_gemini {
            active_model_id_str.contains("gemini")
                || active_model_id_str.contains("flash")
                || active_model_id_str.contains("pro")
        } else if is_3p {
            active_model_id_str.contains("claude")
                || active_model_id_str.contains("opus")
                || active_model_id_str.contains("sonnet")
                || active_model_id_str.contains("gpt")
        } else {
            false
        };

        let model_id = (idx + 1) as u32;
        if is_selected {
            selected_model_id = Some(model_id);
        }

        let display_name = if is_gemini {
            "Gemini (Flash & Pro)".to_string()
        } else if is_3p {
            "Claude & GPT (Sonnet, Opus, GPT-OSS)".to_string()
        } else {
            g.name.clone()
        };

        let badge = if is_gemini {
            Some("Fast".to_string())
        } else {
            Some("Thinking".to_string())
        };

        models.push(ModelQuota {
            name: display_name,
            id: model_id,
            remaining_percentage: remaining_pct,
            used_percentage: used_pct,
            reset_timestamp,
            reset_time: reset_time_str,
            time_until_reset,
            badge,
            is_selected,
        });
    }

    // Try reading user info and plan info from state.vscdb or fall back to system defaults
    let (mut user, mut plan) = if let Ok(default_path) = crate::db::default_db_path() {
        if let Ok(db_report) = crate::db::read_token_report(&default_path) {
            (db_report.user, db_report.plan)
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    if user.is_none() {
        let name = std::env::var("USER").unwrap_or_else(|_| "Antigravity User".to_string());
        user = Some(UserInfo {
            name,
            email: String::new(),
            profile_url: None,
        });
    }

    if plan.is_none() {
        plan = Some(PlanInfo {
            id: "starter".to_string(),
            name: "Antigravity Quota".to_string(),
            description: None,
        });
    }

    Ok(TokenReport {
        provider: Provider::Antigravity,
        user,
        plan,
        selected_model_id,
        selected_model_name: active_model_label,
        models,
        fetched_at: Utc::now().timestamp(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(provider: Provider, name: &str) -> TokenReport {
        TokenReport {
            provider,
            user: None,
            plan: None,
            selected_model_id: None,
            selected_model_name: None,
            models: vec![ModelQuota {
                name: name.to_string(),
                id: 1,
                remaining_percentage: 40.0,
                used_percentage: 60.0,
                reset_timestamp: None,
                reset_time: None,
                time_until_reset: None,
                badge: None,
                is_selected: false,
            }],
            fetched_at: 0,
        }
    }

    fn s(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    #[test]
    fn cache_dir_prefers_xdg_then_home() {
        assert_eq!(cache_dir(s("/x"), s("/h")), Ok(PathBuf::from("/x/antigravity-token-watcher")));
        assert_eq!(cache_dir(None, s("/h")), Ok(PathBuf::from("/h/.cache/antigravity-token-watcher")));
        assert!(cache_dir(None, None).is_err());
    }

    #[test]
    fn empty_xdg_cache_home_counts_as_unset() {
        assert_eq!(cache_dir(s(""), s("/h")), Ok(PathBuf::from("/h/.cache/antigravity-token-watcher")));
        assert!(cache_dir(s(""), None).is_err());
    }

    #[test]
    fn providers_use_separate_cache_files() {
        assert_ne!(cache_file_name(Provider::Antigravity), cache_file_name(Provider::Claude));
    }

    #[test]
    fn written_report_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/claude-cache.json");
        write_cached_report_at(&path, &report(Provider::Claude, "Session"), 1000).unwrap();

        let back = read_cached_report_at(&path, 60, 1000).unwrap();
        assert_eq!(back.provider, Provider::Claude);
        assert_eq!(back.models[0].name, "Session");
    }

    #[test]
    fn ttl_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.json");
        write_cached_report_at(&path, &report(Provider::Antigravity, "Flash"), 1000).unwrap();

        assert!(read_cached_report_at(&path, 60, 1060).is_some(), "exactly at the TTL is still fresh");
        assert!(read_cached_report_at(&path, 60, 1061).is_none(), "one second past the TTL is stale");
    }

    #[test]
    fn stale_cache_is_still_usable_as_a_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.json");
        let week = 86400 * 7;
        write_cached_report_at(&path, &report(Provider::Antigravity, "Flash"), 0).unwrap();

        assert!(read_cached_report_at(&path, 60, 3600).is_none());
        assert!(read_cached_report_at(&path, week, 3600).is_some());
        assert!(read_cached_report_at(&path, week, week as i64 + 1).is_none());
    }

    #[test]
    fn missing_or_corrupt_cache_reads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.json");
        assert!(read_cached_report_at(&path, 60, 0).is_none());

        std::fs::write(&path, "{\"timestamp\": 0, \"report\": {").unwrap();
        assert!(read_cached_report_at(&path, 60, 0).is_none());
    }

    #[test]
    fn cache_written_before_claude_support_loads_as_antigravity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.json");
        let old = r#"{"timestamp": 100, "report": {"user": null, "plan": null,
            "selected_model_id": null, "selected_model_name": null, "models": [], "fetched_at": 100}}"#;
        std::fs::write(&path, old).unwrap();

        let back = read_cached_report_at(&path, 60, 100).unwrap();
        assert_eq!(back.provider, Provider::Antigravity);
    }

    #[test]
    fn write_replaces_the_old_report_and_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.json");
        write_cached_report_at(&path, &report(Provider::Antigravity, "Old"), 0).unwrap();
        write_cached_report_at(&path, &report(Provider::Antigravity, "New"), 0).unwrap();

        assert_eq!(read_cached_report_at(&path, 60, 0).unwrap().models[0].name, "New");
        let files: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(files, ["cache.json"]);
    }

    #[test]
    fn readers_never_see_a_partial_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.json");
        write_cached_report_at(&path, &report(Provider::Antigravity, "Start"), 0).unwrap();

        std::thread::scope(|scope| {
            for w in 0..4 {
                let path = &path;
                scope.spawn(move || {
                    for i in 0..100 {
                        let name = format!("writer {w} {}", "x".repeat(i * 50));
                        write_cached_report_at(path, &report(Provider::Antigravity, &name), 0).unwrap();
                    }
                });
            }
            for _ in 0..4 {
                let path = &path;
                scope.spawn(move || {
                    for _ in 0..500 {
                        assert!(read_cached_report_at(path, 60, 0).is_some(), "read a partial cache file");
                    }
                });
            }
        });
    }
}
