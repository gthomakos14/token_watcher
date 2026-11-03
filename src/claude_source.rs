use crate::models::{ModelQuota, PlanInfo, Provider, TokenReport, UserInfo};
use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Deserialize;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Deserialize)]
struct UsageResponse {
    #[serde(default)]
    is_error: bool,
    result: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthStatus {
    email: Option<String>,
    subscription_type: Option<String>,
}

/// Runs `claude` with a timeout, since this is polled and a hung process must not pile up.
fn run_claude(claude_path: &Path, args: &[&str]) -> Result<String, String> {
    let mut child = Command::new(claude_path)
        .args(args)
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to execute claude: {}", e))?;

    let started = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(_) => break,
            None if started.elapsed() > COMMAND_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("claude {} timed out", args.join(" ")));
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }

    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!("claude {} exited with {}", args.join(" "), output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Parses a reset string such as `Nov 2, 12:59pm` or `12:59pm`, as local time.
fn parse_reset(raw: &str, now: NaiveDateTime) -> Option<NaiveDateTime> {
    let normalized = raw.replace(" at ", ", ");
    let (date_part, time_part) = match normalized.split_once(',') {
        Some((date, time)) => (Some(date.trim()), time.trim()),
        None => (None, normalized.trim()),
    };

    let mut time_text = time_part.to_lowercase().replace(' ', "");
    if !time_text.contains(':') {
        let meridiem_at = time_text.find(['a', 'p'])?;
        time_text.insert_str(meridiem_at, ":00");
    }
    let time = NaiveTime::parse_from_str(&time_text, "%I:%M%P").ok()?;

    match date_part {
        Some(date_text) => {
            // The text has no year, so a date far in the past means it wrapped into next year.
            let date = NaiveDate::parse_from_str(&format!("{} {}", date_text, now.year()), "%b %d %Y").ok()?;
            let reset = date.and_time(time);
            if reset < now - TimeDelta::days(180) {
                date.with_year(now.year() + 1).map(|d| d.and_time(time))
            } else {
                Some(reset)
            }
        }
        None => {
            let reset = now.date().and_time(time);
            Some(if reset <= now { reset + TimeDelta::days(1) } else { reset })
        }
    }
}

/// Resolves a reset string as wall-clock time in `zone`, relative to `now` as seen in that zone.
fn reset_in_zone<Z: TimeZone>(zone: &Z, raw: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let naive = parse_reset(raw, now.with_timezone(zone).naive_local())?;
    zone.from_local_datetime(&naive)
        .earliest()
        .map(|reset| reset.with_timezone(&Utc))
}

/// Splits `Nov 2, 12:59pm (America/New_York)` into the reset text and its time zone, if named.
fn split_reset_zone(text: &str) -> (&str, Option<Tz>) {
    let text = text.trim();
    match text.rfind(" (") {
        Some(tz_at) => {
            let zone = text[tz_at + 2..]
                .strip_suffix(')')
                .and_then(|name| name.trim().parse::<Tz>().ok());
            (text[..tz_at].trim(), zone)
        }
        None => (text, None),
    }
}

/// Parses one limit line: `Current session: 4% used · resets Nov 2, 12:59pm (America/New_York)`.
fn parse_usage_line(line: &str, id: u32, now: DateTime<Utc>) -> Option<ModelQuota> {
    let (label, rest) = line.trim().split_once(':')?;
    let name = label.strip_prefix("Current ")?;
    let (used_text, after_used) = rest.split_once("% used")?;
    let used_pct = used_text.trim().parse::<f32>().ok()?.clamp(0.0, 100.0);

    let mut chars = name.chars();
    let display_name = chars.next()?.to_uppercase().chain(chars).collect();

    let (reset_raw, reset_zone) = match after_used.split_once("resets ") {
        Some((_, r)) => {
            let (raw, zone) = split_reset_zone(r);
            (Some(raw), zone)
        }
        None => (None, None),
    };

    let mut reset_timestamp = None;
    let mut time_until_reset = None;
    let mut reset_time = reset_raw.map(str::to_string);

    // Claude names the zone it printed the time in; the system zone is only a fallback.
    let reset_utc = reset_raw.and_then(|raw| match reset_zone {
        Some(zone) => reset_in_zone(&zone, raw, now),
        None => reset_in_zone(&Local, raw, now),
    });
    if let Some(reset) = reset_utc {
        let ts = reset.timestamp();
        reset_timestamp = Some(ts);
        reset_time = Some(reset.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string());
        time_until_reset = Some(crate::parser::format_duration_until(ts));
    }

    Some(ModelQuota {
        name: display_name,
        id,
        remaining_percentage: 100.0 - used_pct,
        used_percentage: used_pct,
        reset_timestamp,
        reset_time,
        time_until_reset,
        badge: None,
        is_selected: false,
    })
}

fn parse_usage_text(text: &str, now: DateTime<Utc>) -> Vec<ModelQuota> {
    text.lines()
        .filter_map(|line| parse_usage_line(line, 0, now))
        .enumerate()
        .map(|(idx, quota)| ModelQuota { id: (idx + 1) as u32, ..quota })
        .collect()
}

fn plan_name(subscription: Option<&str>) -> String {
    let mut chars = subscription.unwrap_or_default().chars();
    match chars.next() {
        Some(first) => format!("Claude {}{}", first.to_uppercase(), chars.as_str()),
        None => "Claude".to_string(),
    }
}

pub fn fetch_token_report() -> Result<TokenReport, String> {
    let claude_bin = crate::cli_source::find_binary("claude")
        .ok_or_else(|| "claude CLI binary not found in PATH or ~/.local/bin/claude".to_string())?;

    let auth_bin = claude_bin.clone();
    let auth_handle = std::thread::spawn(move || run_claude(&auth_bin, &["auth", "status", "--json"]));

    let usage_raw = run_claude(
        &claude_bin,
        &["-p", "/usage", "--output-format", "json", "--no-session-persistence"],
    )?;
    let usage: UsageResponse = serde_json::from_str(&usage_raw)
        .map_err(|e| format!("Failed to parse claude /usage JSON: {}", e))?;
    if usage.is_error {
        return Err(format!("claude /usage failed: {}", usage.result.unwrap_or_default()));
    }

    let text = usage.result.unwrap_or_default();
    let models = parse_usage_text(&text, Utc::now());
    if models.is_empty() {
        return Err(format!("No usage limits found in claude /usage output: {}", text));
    }

    // Account details are decoration, so a failure here must not lose the usage numbers.
    let auth = auth_handle
        .join()
        .ok()
        .and_then(Result::ok)
        .and_then(|raw| serde_json::from_str::<AuthStatus>(&raw).ok());
    let email = auth.as_ref().and_then(|a| a.email.clone()).unwrap_or_default();
    let subscription = auth.and_then(|a| a.subscription_type);

    Ok(TokenReport {
        provider: Provider::Claude,
        user: Some(UserInfo {
            name: std::env::var("USER").unwrap_or_else(|_| "Claude User".to_string()),
            email,
            profile_url: None,
        }),
        plan: Some(PlanInfo {
            id: subscription.clone().unwrap_or_default(),
            name: plan_name(subscription.as_deref()),
            description: None,
        }),
        selected_model_id: None,
        selected_model_name: None,
        models,
        fetched_at: Utc::now().timestamp(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap()
    }

    fn utc(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        at(year, month, day, hour, minute).and_utc()
    }

    #[test]
    fn parses_real_usage_output() {
        let text = "You are currently using your subscription to power your Claude Code usage\n\n\
            Current session: 4% used · resets Nov 2, 12:59pm (America/New_York)\n\
            Current week (all models): 1% used · resets Nov 4, 9:59pm (America/New_York)";
        // 9:30am in New York (EST, UTC-5).
        let models = parse_usage_text(text, utc(2026, 11, 2, 14, 30));

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "Session");
        assert_eq!(models[0].id, 1);
        assert_eq!(models[0].used_percentage, 4.0);
        assert_eq!(models[0].remaining_percentage, 96.0);
        assert_eq!(models[0].reset_timestamp, Some(utc(2026, 11, 2, 17, 59).timestamp()));
        assert_eq!(models[1].name, "Week (all models)");
        assert_eq!(models[1].id, 2);
        assert_eq!(models[1].remaining_percentage, 99.0);
    }

    #[test]
    fn ignores_text_that_is_not_a_limit() {
        assert!(parse_usage_text("Please log in\nNote: 5% used elsewhere", utc(2026, 1, 1, 0, 0)).is_empty());
    }

    #[test]
    fn reset_uses_the_named_zone_not_the_system_zone() {
        // 11pm UTC on Nov 2 is already 8am Nov 3 in Tokyo, so a bare "9am" is one hour away.
        let models = parse_usage_text("Current session: 10% used · resets 9am (Asia/Tokyo)", utc(2026, 11, 2, 23, 0));
        assert_eq!(models[0].reset_timestamp, Some(utc(2026, 11, 3, 0, 0).timestamp()));
    }

    #[test]
    fn reset_handles_daylight_saving_in_the_named_zone() {
        // July in New York is EDT (UTC-4).
        let models = parse_usage_text(
            "Current week (all models): 1% used · resets Jul 4, 9:59pm (America/New_York)",
            utc(2026, 7, 1, 12, 0),
        );
        assert_eq!(models[0].reset_timestamp, Some(utc(2026, 7, 5, 1, 59).timestamp()));
    }

    #[test]
    fn unknown_zone_is_dropped_from_the_reset_text() {
        assert_eq!(split_reset_zone("Nov 2, 1pm (Mars/Olympus)"), ("Nov 2, 1pm", None));
        assert_eq!(split_reset_zone("Nov 2, 1pm (UTC)"), ("Nov 2, 1pm", Some(Tz::UTC)));
        assert_eq!(split_reset_zone("1pm"), ("1pm", None));
    }

    #[test]
    fn keeps_percentage_when_reset_is_unreadable() {
        let models = parse_usage_text("Current session: 100% used · resets soon", utc(2026, 1, 1, 0, 0));
        assert_eq!(models[0].remaining_percentage, 0.0);
        assert_eq!(models[0].reset_timestamp, None);
        assert_eq!(models[0].reset_time.as_deref(), Some("soon"));
    }

    #[test]
    fn reset_rolls_over_the_new_year() {
        assert_eq!(parse_reset("Jan 2, 3am", at(2026, 12, 30, 8, 0)), Some(at(2027, 1, 2, 3, 0)));
    }

    #[test]
    fn reset_without_a_date_is_the_next_occurrence() {
        assert_eq!(parse_reset("1pm", at(2026, 3, 1, 9, 0)), Some(at(2026, 3, 1, 13, 0)));
        assert_eq!(parse_reset("8:15am", at(2026, 3, 1, 9, 0)), Some(at(2026, 3, 2, 8, 15)));
    }

    #[test]
    fn plan_names() {
        assert_eq!(plan_name(Some("pro")), "Claude Pro");
        assert_eq!(plan_name(None), "Claude");
    }
}
