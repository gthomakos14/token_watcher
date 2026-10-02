mod cli_source;
mod db;
mod models;
mod parser;

use std::env;
use std::path::PathBuf;

fn print_human_report(report: &models::TokenReport) {
    println!("==========================================================");
    println!("             ANTIGRAVITY TOKEN & QUOTA WATCHER            ");
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

fn print_summary_line(report: &models::TokenReport) {
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

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut json_mode = false;
    let mut summary_mode = false;
    let mut force_refresh = false;
    let mut ttl_secs: u64 = 60;
    let mut custom_db: Option<PathBuf> = None;

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
            "--help" | "-h" => {
                println!("Antigravity Token Watcher");
                println!("Usage: token-watcher [OPTIONS]");
                println!();
                println!("Options:");
                println!("  --json             Output report as JSON");
                println!("  --summary          Output single-line summary for status bars");
                println!("  --refresh, --force Bypass cache and fetch fresh quota from agy CLI");
                println!("  --ttl <SECONDS>    Cache TTL in seconds (default: 60)");
                println!("  --db <PATH>        Override path to state.vscdb (forces SQLite mode)");
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
        // Default mode: Prefer CLI with caching
        let mut report = None;

        if !force_refresh {
            if let Some(cached) = cli_source::read_cached_report(ttl_secs) {
                report = Some(cached);
            }
        }

        if report.is_none() {
            if cli_source::find_agy_binary().is_some() {
                match cli_source::fetch_token_report_from_cli() {
                    Ok(rep) => {
                        let _ = cli_source::write_cached_report(&rep);
                        report = Some(rep);
                    }
                    Err(e) => {
                        // If CLI fetch fails, check if we have any stale cache before failing
                        if let Some(stale) = cli_source::read_cached_report(86400 * 7) {
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
    };

    match report_result {
        Ok(report) => {
            if json_mode {
                match serde_json::to_string(&report) {
                    Ok(json) => println!("{}", json),
                    Err(e) => {
                        eprintln!("{{\"error\": \"JSON serialization error: {}\"}}", e);
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
                println!("{{\"error\": \"{}\"}}", e);
            } else {
                eprintln!("Error: {}", e);
            }
            std::process::exit(1);
        }
    }
}
