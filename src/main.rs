mod claude_source;
mod cli_source;
mod db;
mod models;
mod parser;
mod recency;
#[cfg(test)]
mod test_util;

use models::{Provider, TokenReport};
use std::fmt::Write as _;
use std::path::PathBuf;

const STALE_CACHE_SECS: u64 = 86400 * 7;

const HELP: &str = "\
Antigravity & Claude Token Watcher
Usage: token-watcher [OPTIONS]

Options:
  --json             Output report as JSON
  --summary          Output single-line summary for status bars
  --refresh, --force Bypass cache and fetch fresh usage from the CLI
  --ttl <SECONDS>    Cache TTL in seconds (default: 60)
  --db <PATH>        Override path to state.vscdb (forces SQLite mode)
  --provider <NAME>  Use claude or antigravity instead of the most recently prompted
  --help, -h         Show this help message
";

#[derive(Debug, Clone, PartialEq)]
struct Options {
    json: bool,
    summary: bool,
    force_refresh: bool,
    ttl_secs: u64,
    custom_db: Option<PathBuf>,
    provider: Option<Provider>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            json: false,
            summary: false,
            force_refresh: false,
            ttl_secs: 60,
            custom_db: None,
            provider: None,
        }
    }
}

#[derive(Debug, PartialEq)]
enum Cli {
    Help,
    Run(Options),
}

/// Parses the arguments after the program name. Unknown flags and bad values are ignored.
fn parse_args(args: &[String]) -> Cli {
    let mut opts = Options::default();
    let mut i = 0;
    while i < args.len() {
        let value = args.get(i + 1);
        match args[i].as_str() {
            "--json" => opts.json = true,
            "--summary" => opts.summary = true,
            "--refresh" | "--force" => opts.force_refresh = true,
            "--ttl" => {
                if let Some(v) = value {
                    if let Ok(ttl) = v.parse::<u64>() {
                        opts.ttl_secs = ttl;
                    }
                    i += 1;
                }
            }
            "--db" => {
                if let Some(v) = value {
                    opts.custom_db = Some(PathBuf::from(v));
                    i += 1;
                }
            }
            "--provider" => {
                if let Some(v) = value {
                    opts.provider = match v.as_str() {
                        "claude" => Some(Provider::Claude),
                        "antigravity" => Some(Provider::Antigravity),
                        _ => None,
                    };
                    i += 1;
                }
            }
            "--help" | "-h" => return Cli::Help,
            _ => {}
        }
        i += 1;
    }
    Cli::Run(opts)
}

fn format_human_report(report: &TokenReport) -> String {
    const RULE: &str = "==========================================================";
    const THIN_RULE: &str = "----------------------------------------------------------";
    let title = match report.provider {
        Provider::Antigravity => "ANTIGRAVITY TOKEN & QUOTA WATCHER",
        Provider::Claude => "CLAUDE USAGE WATCHER",
    };

    let mut out = String::new();
    // Writing to a String cannot fail, so the results are ignored.
    let _ = writeln!(out, "{RULE}");
    let _ = writeln!(out, "{:^58}", title);
    let _ = writeln!(out, "{RULE}");

    if let Some(user) = &report.user {
        if !user.email.is_empty() {
            let _ = writeln!(out, "User:         {} <{}>", user.name, user.email);
        } else {
            let _ = writeln!(out, "User:         {}", user.name);
        }
    }
    if let Some(plan) = &report.plan {
        let _ = writeln!(out, "Plan:         {}", plan.name);
    }
    if let Some(sel) = &report.selected_model_name {
        let _ = writeln!(out, "Active Model: {}", sel);
    }
    let _ = writeln!(out, "{THIN_RULE}");
    let _ = writeln!(out, "{:<36} {:>8}  {:>14}  {:<10}", "MODEL / GROUP", "REMAIN", "RESETS IN", "STATUS");
    let _ = writeln!(out, "{THIN_RULE}");

    for m in &report.models {
        let prefix = if m.is_selected { "*" } else { " " };
        let bar_len: usize = 10;
        let filled = ((m.remaining_percentage / 100.0) * bar_len as f32).round() as usize;
        let bar: String = "█".repeat(filled) + &"░".repeat(bar_len.saturating_sub(filled));

        let reset_str = m.time_until_reset.as_deref().unwrap_or("N/A");
        let badge_str = m.badge.as_deref().unwrap_or("");

        let _ = writeln!(
            out,
            "{}{:<35} {:>5.1}% [{}] {:>10}  {:<8}",
            prefix, m.name, m.remaining_percentage, bar, reset_str, badge_str
        );
    }
    let _ = writeln!(out, "{RULE}");
    out
}

fn format_summary_line(report: &TokenReport) -> String {
    if report.provider == Provider::Claude {
        let parts: Vec<String> = report
            .models
            .iter()
            .map(|m| format!("{}: {:.0}%", m.name, m.remaining_percentage))
            .collect();
        return format!("Claude {}", parts.join(" | "));
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
            return format!("{}: {:.1}%", first.name, first.remaining_percentage);
        }
    }
    parts.join(" | ")
}

/// Everything the report loaders read from or write to, so tests can substitute fakes.
trait Sources {
    fn read_cache(&self, provider: Provider, max_age_secs: u64) -> Option<TokenReport>;
    fn write_cache(&self, report: &TokenReport);
    fn agy_installed(&self) -> bool;
    fn fetch_agy(&self) -> Result<TokenReport, String>;
    fn fetch_claude(&self) -> Result<TokenReport, String>;
    /// Reads the Antigravity IDE's state.vscdb at its default location, if present.
    fn read_sqlite(&self) -> Option<TokenReport>;
}

struct SystemSources;

impl Sources for SystemSources {
    fn read_cache(&self, provider: Provider, max_age_secs: u64) -> Option<TokenReport> {
        cli_source::read_cached_report(provider, max_age_secs)
    }
    fn write_cache(&self, report: &TokenReport) {
        let _ = cli_source::write_cached_report(report);
    }
    fn agy_installed(&self) -> bool {
        cli_source::find_binary("agy").is_some()
    }
    fn fetch_agy(&self) -> Result<TokenReport, String> {
        cli_source::fetch_token_report_from_cli()
    }
    fn fetch_claude(&self) -> Result<TokenReport, String> {
        claude_source::fetch_token_report()
    }
    fn read_sqlite(&self) -> Option<TokenReport> {
        db::default_db_path().ok().and_then(|p| db::read_token_report(&p).ok())
    }
}

/// Prefers the agy CLI (with caching), then falls back to stale cache and SQLite.
fn load_antigravity_report(src: &impl Sources, force_refresh: bool, ttl_secs: u64) -> Result<TokenReport, String> {
    let provider = Provider::Antigravity;
    if !force_refresh {
        if let Some(cached) = src.read_cache(provider, ttl_secs) {
            return Ok(cached);
        }
    }

    let report = if src.agy_installed() {
        match src.fetch_agy() {
            Ok(rep) => {
                src.write_cache(&rep);
                Some(rep)
            }
            Err(e) => {
                // If the CLI fetch fails, prefer a stale cache, then SQLite.
                let fallback = src.read_cache(provider, STALE_CACHE_SECS).or_else(|| src.read_sqlite());
                if fallback.is_none() {
                    eprintln!("Warning: Failed to fetch from agy CLI ({e})");
                }
                fallback
            }
        }
    } else {
        src.read_sqlite()
    };

    report.ok_or_else(|| "Failed to fetch token data from agy CLI or local SQLite database".to_string())
}

fn load_claude_report(src: &impl Sources, force_refresh: bool, ttl_secs: u64) -> Result<TokenReport, String> {
    let provider = Provider::Claude;
    if !force_refresh {
        if let Some(cached) = src.read_cache(provider, ttl_secs) {
            return Ok(cached);
        }
    }

    match src.fetch_claude() {
        Ok(report) => {
            src.write_cache(&report);
            Ok(report)
        }
        Err(e) => src.read_cache(provider, STALE_CACHE_SECS).ok_or(e),
    }
}

fn load_report(src: &impl Sources, opts: &Options, choose_provider: impl FnOnce() -> Provider) -> Result<TokenReport, String> {
    if let Some(db_path) = &opts.custom_db {
        // Explicit SQLite mode
        return db::read_token_report(db_path);
    }
    match opts.provider.unwrap_or_else(choose_provider) {
        Provider::Antigravity => load_antigravity_report(src, opts.force_refresh, opts.ttl_secs),
        Provider::Claude => load_claude_report(src, opts.force_refresh, opts.ttl_secs),
    }
}

fn error_json(message: &str) -> String {
    serde_json::json!({ "error": message }).to_string()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match parse_args(&args) {
        Cli::Help => {
            print!("{HELP}");
            return;
        }
        Cli::Run(opts) => opts,
    };

    match load_report(&SystemSources, &opts, recency::most_recent_provider) {
        Ok(report) => {
            if opts.json {
                match serde_json::to_string(&report) {
                    Ok(json) => println!("{}", json),
                    Err(e) => {
                        eprintln!("{}", error_json(&format!("JSON serialization error: {e}")));
                        std::process::exit(1);
                    }
                }
            } else if opts.summary {
                println!("{}", format_summary_line(&report));
            } else {
                print!("{}", format_human_report(&report));
            }
        }
        Err(e) => {
            if opts.json {
                println!("{}", error_json(&e));
            } else {
                eprintln!("Error: {}", e);
            }
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use models::{ModelQuota, PlanInfo, UserInfo};
    use std::cell::RefCell;

    fn quota(name: &str, remaining: f32) -> ModelQuota {
        ModelQuota {
            name: name.to_string(),
            id: 1,
            remaining_percentage: remaining,
            used_percentage: 100.0 - remaining,
            reset_timestamp: None,
            reset_time: None,
            time_until_reset: None,
            badge: None,
            is_selected: false,
        }
    }

    fn report(provider: Provider, models: Vec<ModelQuota>) -> TokenReport {
        TokenReport {
            provider,
            user: None,
            plan: None,
            selected_model_id: None,
            selected_model_name: None,
            models,
            fetched_at: 0,
        }
    }

    /// A report whose single model name says where it came from.
    fn tagged(provider: Provider, tag: &str) -> TokenReport {
        report(provider, vec![quota(tag, 50.0)])
    }

    fn tag(r: &TokenReport) -> &str {
        &r.models[0].name
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    // --- parse_args ---

    #[test]
    fn no_arguments_gives_defaults() {
        assert_eq!(parse_args(&[]), Cli::Run(Options::default()));
    }

    #[test]
    fn every_flag_is_parsed() {
        let cli = parse_args(&args(&[
            "--json", "--summary", "--refresh", "--ttl", "5", "--db", "/x.db", "--provider", "claude",
        ]));
        assert_eq!(
            cli,
            Cli::Run(Options {
                json: true,
                summary: true,
                force_refresh: true,
                ttl_secs: 5,
                custom_db: Some(PathBuf::from("/x.db")),
                provider: Some(Provider::Claude),
            })
        );
    }

    #[test]
    fn force_is_an_alias_for_refresh_and_antigravity_is_a_provider() {
        let Cli::Run(opts) = parse_args(&args(&["--force", "--provider", "antigravity"])) else { panic!() };
        assert!(opts.force_refresh);
        assert_eq!(opts.provider, Some(Provider::Antigravity));
    }

    #[test]
    fn help_wins_wherever_it_appears() {
        assert_eq!(parse_args(&args(&["--help"])), Cli::Help);
        assert_eq!(parse_args(&args(&["--json", "-h"])), Cli::Help);
    }

    #[test]
    fn bad_values_are_ignored_but_still_consumed() {
        // "abc" is not a number, so the TTL stays at its default and "abc" is not read as a flag.
        let Cli::Run(opts) = parse_args(&args(&["--ttl", "abc", "--json"])) else { panic!() };
        assert_eq!(opts.ttl_secs, 60);
        assert!(opts.json);

        let Cli::Run(opts) = parse_args(&args(&["--provider", "bogus"])) else { panic!() };
        assert_eq!(opts.provider, None);
    }

    #[test]
    fn a_flag_missing_its_value_is_ignored() {
        assert_eq!(parse_args(&args(&["--ttl"])), Cli::Run(Options::default()));
        assert_eq!(parse_args(&args(&["--db"])), Cli::Run(Options::default()));
        assert_eq!(parse_args(&args(&["--provider"])), Cli::Run(Options::default()));
    }

    #[test]
    fn unknown_arguments_are_ignored() {
        assert_eq!(parse_args(&args(&["--nope", "stray"])), Cli::Run(Options::default()));
    }

    // --- output formatting ---

    #[test]
    fn claude_summary_lists_every_limit() {
        let r = report(Provider::Claude, vec![quota("Session", 93.4), quota("Week (all models)", 95.0)]);
        assert_eq!(format_summary_line(&r), "Claude Session: 93% | Week (all models): 95%");
    }

    #[test]
    fn antigravity_summary_groups_by_model_family() {
        let r = report(
            Provider::Antigravity,
            vec![quota("Gemini (Flash & Pro)", 72.0), quota("Claude & GPT (Sonnet, Opus, GPT-OSS)", 100.0)],
        );
        assert_eq!(format_summary_line(&r), "Gemini: 72% | Claude: 100%");
    }

    #[test]
    fn antigravity_summary_falls_back_to_the_first_model() {
        let r = report(Provider::Antigravity, vec![quota("Mystery", 41.25)]);
        assert_eq!(format_summary_line(&r), "Mystery: 41.2%");
        assert_eq!(format_summary_line(&report(Provider::Antigravity, vec![])), "");
    }

    #[test]
    fn human_report_shows_header_user_plan_and_models() {
        let mut r = report(Provider::Antigravity, vec![quota("Gemini Flash", 72.0), quota("Claude", 100.0)]);
        r.user = Some(UserInfo { name: "Ada".into(), email: "ada@example.com".into(), profile_url: None });
        r.plan = Some(PlanInfo { id: "pro".into(), name: "Pro Plan".into(), description: None });
        r.selected_model_name = Some("Claude".into());
        r.models[1].is_selected = true;
        r.models[0].time_until_reset = Some("2h 5m".into());
        r.models[0].badge = Some("Fast".into());

        let out = format_human_report(&r);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[1].trim(), "ANTIGRAVITY TOKEN & QUOTA WATCHER");
        assert_eq!(lines[1].chars().count(), 58, "title is centered in 58 columns");
        assert!(out.contains("User:         Ada <ada@example.com>\n"));
        assert!(out.contains("Plan:         Pro Plan\n"));
        assert!(out.contains("Active Model: Claude\n"));
        // 72% fills 7 of 10 bar cells; the unselected row starts with a space.
        assert!(out.contains(" Gemini Flash"));
        assert!(out.contains("72.0% [███████░░░]      2h 5m  Fast"));
        // Selected rows are starred; a missing reset shows N/A.
        assert!(out.contains("*Claude"));
        assert!(out.contains("100.0% [██████████]        N/A"));
        assert!(out.ends_with("==========================================================\n"));
    }

    #[test]
    fn human_report_omits_missing_details() {
        let mut r = report(Provider::Claude, vec![quota("Session", 0.0)]);
        r.user = Some(UserInfo { name: "glenn".into(), email: String::new(), profile_url: None });

        let out = format_human_report(&r);
        assert!(out.contains("CLAUDE USAGE WATCHER"));
        assert!(out.contains("User:         glenn\n"), "no empty <> when there is no email");
        assert!(!out.contains("Plan:"));
        assert!(!out.contains("Active Model:"));
        assert!(out.contains("0.0% [░░░░░░░░░░]"));
    }

    #[test]
    fn error_json_escapes_the_message() {
        let json = error_json("bad \"quote\"\nnew line");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["error"], "bad \"quote\"\nnew line");
    }

    // --- report loading ---

    /// In-memory stand-in for the cache, CLIs, and SQLite, recording what was used.
    struct Fake {
        /// (provider, age in seconds, report)
        cache: Vec<(Provider, u64, TokenReport)>,
        agy_installed: bool,
        agy: Result<TokenReport, String>,
        claude: Result<TokenReport, String>,
        sqlite: Option<TokenReport>,
        calls: RefCell<Vec<String>>,
    }

    impl Default for Fake {
        fn default() -> Self {
            Fake {
                cache: vec![],
                agy_installed: true,
                agy: Err("agy failed".into()),
                claude: Err("claude failed".into()),
                sqlite: None,
                calls: RefCell::new(vec![]),
            }
        }
    }

    impl Fake {
        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
        fn log(&self, call: String) {
            self.calls.borrow_mut().push(call);
        }
    }

    impl Sources for Fake {
        fn read_cache(&self, provider: Provider, max_age_secs: u64) -> Option<TokenReport> {
            self.log(format!("read_cache {provider:?} {max_age_secs}"));
            self.cache
                .iter()
                .find(|(p, age, _)| *p == provider && *age <= max_age_secs)
                .map(|(_, _, r)| r.clone())
        }
        fn write_cache(&self, report: &TokenReport) {
            self.log(format!("write_cache {}", tag(report)));
        }
        fn agy_installed(&self) -> bool {
            self.agy_installed
        }
        fn fetch_agy(&self) -> Result<TokenReport, String> {
            self.log("fetch_agy".into());
            self.agy.clone()
        }
        fn fetch_claude(&self) -> Result<TokenReport, String> {
            self.log("fetch_claude".into());
            self.claude.clone()
        }
        fn read_sqlite(&self) -> Option<TokenReport> {
            self.log("read_sqlite".into());
            self.sqlite.clone()
        }
    }

    const AG: Provider = Provider::Antigravity;
    const CL: Provider = Provider::Claude;

    #[test]
    fn antigravity_fresh_cache_skips_the_cli() {
        let src = Fake { cache: vec![(AG, 10, tagged(AG, "cached"))], ..Fake::default() };
        assert_eq!(tag(&load_antigravity_report(&src, false, 60).unwrap()), "cached");
        assert_eq!(src.calls(), ["read_cache Antigravity 60"]);
    }

    #[test]
    fn antigravity_refresh_bypasses_the_cache_and_saves_the_result() {
        let src = Fake {
            cache: vec![(AG, 10, tagged(AG, "cached"))],
            agy: Ok(tagged(AG, "fresh")),
            ..Fake::default()
        };
        assert_eq!(tag(&load_antigravity_report(&src, true, 60).unwrap()), "fresh");
        assert_eq!(src.calls(), ["fetch_agy", "write_cache fresh"]);
    }

    #[test]
    fn antigravity_expired_cache_fetches() {
        let src = Fake {
            cache: vec![(AG, 120, tagged(AG, "old"))],
            agy: Ok(tagged(AG, "fresh")),
            ..Fake::default()
        };
        assert_eq!(tag(&load_antigravity_report(&src, false, 60).unwrap()), "fresh");
    }

    #[test]
    fn antigravity_failed_fetch_uses_stale_cache_before_sqlite() {
        let src = Fake {
            cache: vec![(AG, 3600, tagged(AG, "stale"))],
            sqlite: Some(tagged(AG, "sqlite")),
            ..Fake::default()
        };
        assert_eq!(tag(&load_antigravity_report(&src, false, 60).unwrap()), "stale");
        assert!(!src.calls().contains(&"read_sqlite".to_string()));
        assert!(src.calls().contains(&format!("read_cache Antigravity {STALE_CACHE_SECS}")));
    }

    #[test]
    fn antigravity_failed_fetch_without_cache_uses_sqlite() {
        let src = Fake { sqlite: Some(tagged(AG, "sqlite")), ..Fake::default() };
        assert_eq!(tag(&load_antigravity_report(&src, false, 60).unwrap()), "sqlite");
    }

    #[test]
    fn antigravity_cache_older_than_a_week_is_not_used() {
        let src = Fake { cache: vec![(AG, STALE_CACHE_SECS + 1, tagged(AG, "ancient"))], ..Fake::default() };
        assert!(load_antigravity_report(&src, false, 60).is_err());
    }

    #[test]
    fn antigravity_with_nothing_available_is_an_error() {
        let err = load_antigravity_report(&Fake::default(), false, 60).unwrap_err();
        assert!(err.contains("agy CLI or local SQLite"));
    }

    #[test]
    fn antigravity_without_agy_reads_sqlite_directly() {
        let src = Fake {
            agy_installed: false,
            agy: Ok(tagged(AG, "never")),
            sqlite: Some(tagged(AG, "sqlite")),
            ..Fake::default()
        };
        assert_eq!(tag(&load_antigravity_report(&src, false, 60).unwrap()), "sqlite");
        assert!(!src.calls().contains(&"fetch_agy".to_string()));

        let src = Fake { agy_installed: false, ..Fake::default() };
        assert!(load_antigravity_report(&src, false, 60).is_err());
    }

    #[test]
    fn antigravity_ignores_claude_cache() {
        let src = Fake { cache: vec![(CL, 0, tagged(CL, "claude"))], ..Fake::default() };
        assert!(load_antigravity_report(&src, false, 60).is_err());
    }

    #[test]
    fn claude_fresh_cache_skips_the_cli() {
        let src = Fake { cache: vec![(CL, 10, tagged(CL, "cached"))], ..Fake::default() };
        assert_eq!(tag(&load_claude_report(&src, false, 60).unwrap()), "cached");
        assert_eq!(src.calls(), ["read_cache Claude 60"]);
    }

    #[test]
    fn claude_refresh_fetches_and_saves() {
        let src = Fake {
            cache: vec![(CL, 10, tagged(CL, "cached"))],
            claude: Ok(tagged(CL, "fresh")),
            ..Fake::default()
        };
        assert_eq!(tag(&load_claude_report(&src, true, 60).unwrap()), "fresh");
        assert_eq!(src.calls(), ["fetch_claude", "write_cache fresh"]);
    }

    #[test]
    fn claude_failed_fetch_uses_stale_cache() {
        let src = Fake { cache: vec![(CL, 3600, tagged(CL, "stale"))], ..Fake::default() };
        assert_eq!(tag(&load_claude_report(&src, false, 60).unwrap()), "stale");
    }

    #[test]
    fn claude_failed_fetch_without_cache_reports_the_fetch_error() {
        let src = Fake { sqlite: Some(tagged(AG, "sqlite")), ..Fake::default() };
        assert_eq!(load_claude_report(&src, false, 60).unwrap_err(), "claude failed");
        // Claude never falls back to Antigravity sources.
        assert!(!src.calls().iter().any(|c| c == "read_sqlite" || c == "fetch_agy"));
    }

    #[test]
    fn explicit_provider_overrides_the_automatic_choice() {
        let src = Fake { claude: Ok(tagged(CL, "claude")), ..Fake::default() };
        let opts = Options { provider: Some(Provider::Claude), ..Options::default() };
        let r = load_report(&src, &opts, || panic!("should not auto-select")).unwrap();
        assert_eq!(tag(&r), "claude");
    }

    #[test]
    fn automatic_choice_is_used_without_an_explicit_provider() {
        let src = Fake { claude: Ok(tagged(CL, "claude")), agy: Ok(tagged(AG, "agy")), ..Fake::default() };
        assert_eq!(tag(&load_report(&src, &Options::default(), || Provider::Claude).unwrap()), "claude");
        assert_eq!(tag(&load_report(&src, &Options::default(), || Provider::Antigravity).unwrap()), "agy");
    }

    #[test]
    fn custom_db_bypasses_every_other_source() {
        let src = Fake::default();
        let opts = Options { custom_db: Some(PathBuf::from("/nonexistent/state.vscdb")), ..Options::default() };
        let err = load_report(&src, &opts, || panic!("should not auto-select")).unwrap_err();
        assert!(err.contains("does not exist"));
        assert!(src.calls().is_empty());
    }
}
