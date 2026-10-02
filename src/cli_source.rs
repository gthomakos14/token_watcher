use crate::models::{ModelQuota, PlanInfo, TokenReport, UserInfo};
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

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

pub fn find_agy_binary() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(&home).join(".local/bin/agy");
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let p = dir.join("agy");
            if p.exists() {
                return Some(p);
            }
        }
    }
    let p = PathBuf::from("/usr/local/bin/agy");
    if p.exists() {
        return Some(p);
    }
    let p = PathBuf::from("/usr/bin/agy");
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

pub fn cache_path() -> Result<PathBuf, String> {
    let dir = if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        PathBuf::from(xdg).join("antigravity-token-watcher")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".cache").join("antigravity-token-watcher")
    } else {
        return Err("Neither XDG_CACHE_HOME nor HOME is set".to_string());
    };
    Ok(dir.join("cache.json"))
}

pub fn read_cached_report(max_age_secs: u64) -> Option<TokenReport> {
    let path = cache_path().ok()?;
    let data = std::fs::read_to_string(&path).ok()?;
    let cached: CachedReport = serde_json::from_str(&data).ok()?;
    let now = Utc::now().timestamp();
    if (now - cached.timestamp).abs() <= max_age_secs as i64 {
        Some(cached.report)
    } else {
        None
    }
}

pub fn write_cached_report(report: &TokenReport) -> Result<(), String> {
    let path = cache_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let cached = CachedReport {
        timestamp: Utc::now().timestamp(),
        report: report.clone(),
    };
    let json = serde_json::to_string_pretty(&cached).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn fetch_token_report_from_cli() -> Result<TokenReport, String> {
    let agy_bin = find_agy_binary().ok_or_else(|| "agy CLI binary not found in PATH or ~/.local/bin/agy".to_string())?;

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
        user,
        plan,
        selected_model_id,
        selected_model_name: active_model_label,
        models,
        fetched_at: Utc::now().timestamp(),
    })
}
