mod claude_source;
mod cli_source;
mod db;
mod models;
mod parser;
mod recency;

use models::{Provider, TokenReport};
use std::env;
use std::path::PathBuf;

const STALE_CACHE_SECS: u64 = 86400 * 7;

fn print_human_report(report: &TokenReport) {
    let title = match report.provider {
        Provider::Antigravity => "ANTIGRAVITY TOKEN & QUOTA WATCHER",
        Provider::Claude => "CLAUDE USAGE WATCHER",
    };
    println!("==========================================================");
    println!("{:^58}", title);
    println!("==========================================================");

    if let Some(user) = &report.user {
        if !user.email.is_empty() {
            println!("User:         {} <{}>", user.name, user.email);
        } else {
            println!("User:         {}", user.name);
        }
    }
    if let Some(plan) = &report.plan {
        println!("Plan:         {}", plan.name);
    }
    if let Some(sel) = &report.selected_model_name {
        println!("Active Model: {}", sel);
    }
    println!("----------------------------------------------------------");
    println!("{:<36} {:>8}  {:>14}  {:<10}", "MODEL / GROUP", "REMAIN", "RESETS IN", "STATUS");
    println!("----------------------------------------------------------");

    for m in &report.models {
        let prefix = if m.is_selected { "*" } else { " " };
        let bar_len: usize = 10;
        let filled = ((m.remaining_percentage / 100.0) * bar_len as f32).round() as usize;
        let bar: String = "█".repeat(filled) + &"░".repeat(bar_len.saturating_sub(filled));

        let reset_str = m.time_until_reset.as_deref().unwrap_or("N/A");
        let badge_str = m.badge.as_deref().unwrap_or("");

        println!(
            "{}{:<35} {:>5.1}% [{}] {:>10}  {:<8}",
            prefix,
            m.name,
            m.remaining_percentage,
            bar,
            reset_str,
            badge_str
        );
    }
    println!("==========================================================");
}

fn print_summary_line(report: &TokenReport) {
    if report.provider == Provider::Claude {
        let parts: Vec<String> = report
            .models
            .iter()
            .map(|m| format!("{}: {:.0}%", m.name, m.remaining_percentage))
            .collect();
        println!("Claude {}", parts.join(" | "));
        return;
    }

    let mut parts = Vec::new();
    for m in &report.models {
        if m.name.contains("Flash") || m.name.contains("Gemini") {
            parts.push(format!("Gemini: {:.0}%", m.remaining_percentage));
        } else if m.name.contains("Claude") || m.name.contains("Sonnet") || m.name.contains("GPT") {
            parts.push(format!("Claude: {:.0}%", m.remaining_percentage));
        }
    }
    if parts.is_empty() {
        if let Some(first) = report.models.first() {
            println!("{}: {:.1}%", first.name, first.remaining_percentage);
            return;
        }
    }
    println!("{}", parts.join(" | "));
}

/// Prefers the agy CLI (with caching), then falls back to stale cache and SQLite.
fn load_antigravity_report(force_refresh: bool, ttl_secs: u64) -> Result<TokenReport, String> {
    let provider = Provider::Antigravity;
    let mut report = None;

    if !force_refresh {
        report = cli_source::read_cached_report(provider, ttl_secs);
    }

    if report.is_none() {
        if cli_source::find_binary("agy").is_some() {
            match cli_source::fetch_token_report_from_cli() {
                Ok(rep) => {
                    let _ = cli_source::write_cached_report(&rep);
                    report = Some(rep);
                }
                Err(e) => {
                    // If CLI fetch fails, check if we have any stale cache before failing
                    if let Some(stale) = cli_source::read_cached_report(provider, STALE_CACHE_SECS) {
                        report = Some(stale);
                    } else {
                        // Fallback to SQLite if available
                        if let Ok(p) = db::default_db_path() {
                            if p.exists() {
                                if let Ok(db_rep) = db::read_token_report(&p) {
                                    report = Some(db_rep);
                                }
                            }
                        }
                        if report.is_none() {
                            eprintln!("Warning: Failed to fetch from agy CLI ({e})");
                        }
                    }
                }
            }
        } else {
            // agy CLI not found; fallback to SQLite
            if let Ok(p) = db::default_db_path() {
                report = db::read_token_report(&p).ok();
            }
        }
    }

    report.ok_or_else(|| "Failed to fetch token data from agy CLI or local SQLite database".to_string())
}

fn load_claude_report(force_refresh: bool, ttl_secs: u64) -> Result<TokenReport, String> {
    let provider = Provider::Claude;
    if !force_refresh {
        if let Some(cached) = cli_source::read_cached_report(provider, ttl_secs) {
            return Ok(cached);
        }
    }

    match claude_source::fetch_token_report() {
        Ok(report) => {
            let _ = cli_source::write_cached_report(&report);
            Ok(report)
        }
        Err(e) => cli_source::read_cached_report(provider, STALE_CACHE_SECS).ok_or(e),
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut json_mode = false;
    let mut summary_mode = false;
    let mut force_refresh = false;
    let mut ttl_secs: u64 = 60;
    let mut custom_db: Option<PathBuf> = None;
    let mut forced_provider: Option<Provider> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => json_mode = true,
            "--summary" => summary_mode = true,
            "--refresh" | "--force" => force_refresh = true,
            "--ttl" => {
                if i + 1 < args.len() {
                    if let Ok(v) = args[i + 1].parse::<u64>() {
                        ttl_secs = v;
                    }
                    i += 1;
                }
            }
            "--db" => {
                if i + 1 < args.len() {
                    custom_db = Some(PathBuf::from(&args[i + 1]));
                    i += 1;
                }
            }
            "--provider" => {
                if i + 1 < args.len() {
                    forced_provider = match args[i + 1].as_str() {
                        "claude" => Some(Provider::Claude),
                        "antigravity" => Some(Provider::Antigravity),
                        _ => None,
                    };
                    i += 1;
                }
            }
            "--help" | "-h" => {
                println!("Antigravity & Claude Token Watcher");
                println!("Usage: token-watcher [OPTIONS]");
                println!();
                println!("Options:");
                println!("  --json             Output report as JSON");
                println!("  --summary          Output single-line summary for status bars");
                println!("  --refresh, --force Bypass cache and fetch fresh usage from the CLI");
                println!("  --ttl <SECONDS>    Cache TTL in seconds (default: 60)");
                println!("  --db <PATH>        Override path to state.vscdb (forces SQLite mode)");
                println!("  --provider <NAME>  Use claude or antigravity instead of the most recently prompted");
                println!("  --help, -h         Show this help message");
                return;
            }
            _ => {}
        }
        i += 1;
    }

    let report_result = if let Some(db_path) = custom_db {
        // Explicit SQLite mode
        db::read_token_report(&db_path)
    } else {
        match forced_provider.unwrap_or_else(recency::most_recent_provider) {
            Provider::Antigravity => load_antigravity_report(force_refresh, ttl_secs),
            Provider::Claude => load_claude_report(force_refresh, ttl_secs),
        }
    };

    match report_result {
        Ok(report) => {
            if json_mode {
                match serde_json::to_string(&report) {
                    Ok(json) => println!("{}", json),
                    Err(e) => {
                        eprintln!("{}", serde_json::json!({ "error": format!("JSON serialization error: {e}") }));
                        std::process::exit(1);
                    }
                }
            } else if summary_mode {
                print_summary_line(&report);
            } else {
                print_human_report(&report);
            }
        }
        Err(e) => {
            if json_mode {
                println!("{}", serde_json::json!({ "error": e }));
            } else {
                eprintln!("Error: {}", e);
            }
            std::process::exit(1);
        }
    }
}
