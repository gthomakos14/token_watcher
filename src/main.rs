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
        println!("User:  {} <{}>", user.name, user.email);
    }
    if let Some(plan) = &report.plan {
        println!("Plan:  {}", plan.name);
    }
    if let Some(sel) = &report.selected_model_name {
        println!("Active Model: {}", sel);
    }
    println!("----------------------------------------------------------");
    println!("{:<30} {:>8}  {:>14}  {:<10}", "MODEL", "REMAIN", "RESETS IN", "STATUS");
    println!("----------------------------------------------------------");

    for m in &report.models {
        let prefix = if m.is_selected { "*" } else { " " };
        let bar_len: usize = 10;
        let filled = ((m.remaining_percentage / 100.0) * bar_len as f32).round() as usize;
        let bar: String = "█".repeat(filled) + &"░".repeat(bar_len.saturating_sub(filled));

        let reset_str = m.time_until_reset.as_deref().unwrap_or("N/A");
        let badge_str = m.badge.as_deref().unwrap_or("");

        println!(
            "{}{:<29} {:>5.1}% [{}] {:>10}  {:<8}",
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
    // Pick the selected model or first model or flash/claude
    let mut parts = Vec::new();
    for m in &report.models {
        if m.name.contains("Flash") && m.name.contains("High") {
            parts.push(format!("Flash: {:.0}%", m.remaining_percentage));
        } else if m.name.contains("Sonnet") {
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
    let mut custom_db: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => json_mode = true,
            "--summary" => summary_mode = true,
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
                println!("  --json       Output report as JSON");
                println!("  --summary    Output single-line summary for status bars");
                println!("  --db <PATH>  Override path to state.vscdb");
                println!("  --help, -h   Show this help message");
                return;
            }
            _ => {}
        }
        i += 1;
    }

    let db_path = match custom_db.or_else(db::default_db_path) {
        Some(p) => p,
        None => {
            eprintln!("Error: $HOME is not set; cannot locate Antigravity state database.");
            std::process::exit(1);
        }
    };

    match db::read_token_report(&db_path) {
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
