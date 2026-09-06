use crate::model::{ActivitySummary, LimitWindow, ProviderAccount, ProviderSnapshot};
use base64::Engine;
use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

const ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";
const MANAGE_URL: &str = "https://chatgpt.com/#settings/Account";

pub async fn snapshot() -> ProviderSnapshot {
    let root = codex_home();
    let auth_path = root.join("auth.json");
    let mut account = account(&auth_path);

    // Prefer the live account-level usage that Codex itself reads. Rollout logs
    // are only point-in-time snapshots written during a turn and can be stale
    // even while they remain the newest local file.
    if let Ok((windows, plan)) = live_usage(&auth_path).await {
        if let Some(plan) = plan {
            match account.as_mut() {
                Some(account) => account.plan = Some(plan),
                None => account = Some(ProviderAccount {
                    label: None,
                    plan: Some(plan),
                    source: Some("Codex".into()),
                }),
            }
        }
        let activity = newest_rollout(&root.join("sessions"))
            .and_then(|file| fs::metadata(file).and_then(|m| m.modified()).ok())
            .and_then(activity_from_modified);
        let headline_id = windows.iter().find(|window| window.id == "primary")
            .or_else(|| windows.first())
            .map(|window| window.id.clone());
        return ProviderSnapshot {
            id: "codex".into(), display_name: "Codex".into(), glyph: "✦".into(),
            fidelity: "official".into(), status: "ok".into(), windows,
            headline_id, fetched_at: Utc::now(), message: None, account,
            manage_url: Some(MANAGE_URL.into()), display_value: None, activity,
        };
    }

    // Live usage is best-effort because Codex may be signed in with an API key,
    // the internal endpoint can change, or the saved OAuth token may need Codex
    // itself to refresh it. Preserve the existing local fallback for those cases.
    let Some(file) = newest_rollout(&root.join("sessions")) else {
        return ProviderSnapshot::unavailable(
            "codex", "Codex", "✦", "needsAuth",
            "No live Codex usage or rollout log is available. Run Codex and sign in with ChatGPT first.",
            MANAGE_URL, account,
        );
    };

    match fs::read_to_string(&file) {
        Ok(text) => match parse_rollout(&text) {
            Ok((windows, recorded_at)) => {
                let modified = fs::metadata(&file).and_then(|m| m.modified()).ok();
                let activity = modified.and_then(activity_from_modified);
                let headline_id = windows.iter().find(|window| window.id == "primary")
                    .or_else(|| windows.first())
                    .map(|window| window.id.clone());
                ProviderSnapshot {
                    id: "codex".into(), display_name: "Codex".into(), glyph: "✦".into(),
                    fidelity: "official".into(), status: "ok".into(), windows,
                    headline_id, fetched_at: recorded_at.unwrap_or_else(Utc::now),
                    message: None, account, manage_url: Some(MANAGE_URL.into()), display_value: None, activity,
                }
            }
            Err(message) => ProviderSnapshot::unavailable(
                "codex", "Codex", "✦", "error", message, MANAGE_URL, account,
            ),
        },
        Err(error) => ProviderSnapshot::unavailable(
            "codex", "Codex", "✦", "error", format!("Could not read Codex rollout: {error}"), MANAGE_URL, account,
        ),
    }
}

fn codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME").map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

fn newest_rollout(root: &Path) -> Option<PathBuf> {
    WalkDir::new(root).into_iter().filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("rollout-") && entry.path().extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.into_path()))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

struct LiveAuth {
    access_token: String,
    account_id: Option<String>,
}

fn live_auth(path: &Path) -> Result<LiveAuth, String> {
    let text = fs::read_to_string(path).map_err(|_| "Codex auth.json was not found.".to_string())?;
    let root: Value = serde_json::from_str(&text).map_err(|error| format!("Codex auth.json is invalid JSON: {error}"))?;
    let tokens = root.get("tokens").ok_or("Codex ChatGPT login tokens are missing.")?;
    let access_token = tokens.get("access_token").and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("Codex ChatGPT access token is missing.")?
        .to_owned();
    let account_id = tokens.get("account_id").and_then(Value::as_str)
        .filter(|value| !value.is_empty()).map(str::to_owned);
    Ok(LiveAuth { access_token, account_id })
}

async fn live_usage(path: &Path) -> Result<(Vec<LimitWindow>, Option<String>), String> {
    let auth = live_auth(path)?;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(15)).build()
        .map_err(|error| format!("Could not create Codex usage client: {error}"))?;
    let mut request = client.get(ENDPOINT)
        .header("Authorization", format!("Bearer {}", auth.access_token))
        .header("Accept", "application/json")
        .header("Origin", "https://chatgpt.com")
        .header("Referer", "https://chatgpt.com/")
        .header("User-Agent", "Codenotch");
    if let Some(account_id) = auth.account_id {
        request = request.header("ChatGPT-Account-Id", account_id);
    }
    let response = request.send().await
        .map_err(|error| format!("Codex live usage request failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("Codex live usage endpoint returned {}", response.status()));
    }
    let body = response.json::<Value>().await
        .map_err(|error| format!("Could not parse live Codex usage: {error}"))?;
    let windows = parse_live_usage(&body)?;
    let plan = body.get("plan_type").or_else(|| body.get("planType"))
        .and_then(Value::as_str).map(str::to_owned);
    Ok((windows, plan))
}

fn parse_live_usage(root: &Value) -> Result<Vec<LimitWindow>, String> {
    let limits = root.get("rate_limit").or_else(|| root.get("rate_limits"))
        .ok_or("Codex returned no rate-limit object.")?;
    let now = Utc::now();
    let mut windows = Vec::new();
    for (bucket, fallback_id, fallback_label) in [
        (limits.get("primary_window").or_else(|| limits.get("primary")), "primary", "Current session"),
        (limits.get("secondary_window").or_else(|| limits.get("secondary")), "secondary", "Longer window"),
    ] {
        let Some(bucket) = bucket else { continue };
        let Some(percent) = bucket.get("used_percent").or_else(|| bucket.get("usedPercent")).and_then(Value::as_f64) else { continue };
        let minutes = window_minutes(bucket);
        let resets_at = reset_at(bucket, now);
        windows.push(LimitWindow {
            id: classify_window_id(minutes, fallback_id),
            label: window_label(minutes, fallback_label),
            used_fraction: percent / 100.0,
            resets_at,
        });
    }
    if windows.is_empty() {
        return Err("Codex returned no usage windows.".into());
    }
    sort_windows(&mut windows);
    Ok(windows)
}

fn parse_rollout(text: &str) -> Result<(Vec<LimitWindow>, Option<DateTime<Utc>>), String> {
    for line in text.lines().rev().filter(|line| line.contains("rate_limits")) {
        let Ok(root) = serde_json::from_str::<Value>(line) else { continue };
        let limits = root.get("rate_limits").or_else(|| root.get("payload")?.get("rate_limits"));
        let Some(limits) = limits else { continue };
        let now = Utc::now();
        let mut windows = Vec::new();
        for (raw_id, fallback) in [("primary", "Current session"), ("secondary", "Longer window")] {
            let Some(bucket) = limits.get(raw_id) else { continue };
            let Some(percent) = bucket.get("used_percent").or_else(|| bucket.get("usedPercent")).and_then(Value::as_f64) else { continue };
            let minutes = window_minutes(bucket);
            windows.push(LimitWindow {
                id: classify_window_id(minutes, raw_id),
                label: window_label(minutes, fallback),
                used_fraction: percent / 100.0,
                resets_at: reset_at(bucket, now),
            });
        }
        if windows.is_empty() { continue; }
        sort_windows(&mut windows);
        let recorded_at = root.get("timestamp").and_then(Value::as_str)
            .and_then(|stamp| DateTime::parse_from_rfc3339(stamp).ok()).map(|stamp| stamp.with_timezone(&Utc));
        return Ok((windows, recorded_at));
    }
    Err("Codex has not recorded a usage snapshot in its latest rollout yet.".into())
}

fn window_minutes(bucket: &Value) -> Option<f64> {
    bucket.get("window_minutes").or_else(|| bucket.get("windowDurationMins"))
        .and_then(Value::as_f64)
        .or_else(|| bucket.get("limit_window_seconds").or_else(|| bucket.get("limitWindowSeconds"))
            .and_then(Value::as_f64).map(|seconds| seconds / 60.0))
}

fn reset_at(bucket: &Value, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    bucket.get("reset_at").or_else(|| bucket.get("resets_at")).or_else(|| bucket.get("resetsAt"))
        .and_then(Value::as_f64)
        .and_then(|epoch| Utc.timestamp_opt(epoch as i64, 0).single())
        .or_else(|| bucket.get("reset_after_seconds").or_else(|| bucket.get("resets_in_seconds")).or_else(|| bucket.get("resetAfterSeconds"))
            .and_then(Value::as_f64).map(|seconds| now + chrono::Duration::seconds(seconds as i64)))
}

fn classify_window_id(minutes: Option<f64>, fallback: &str) -> String {
    match minutes {
        Some(minutes) if (minutes - 300.0).abs() < 1.0 => "primary".into(),
        Some(minutes) if (minutes - 10080.0).abs() < 1.0 => "secondary".into(),
        _ => fallback.into(),
    }
}

fn sort_windows(windows: &mut [LimitWindow]) {
    windows.sort_by_key(|window| match window.id.as_str() {
        "primary" => 0,
        "secondary" => 1,
        _ => 2,
    });
}

fn window_label(minutes: Option<f64>, fallback: &str) -> String {
    let Some(minutes) = minutes.filter(|v| *v > 0.0) else { return fallback.into(); };
    if minutes < 60.0 { return format!("{}m limit", minutes as i64); }
    if minutes < 1440.0 { return format!("{}h limit", (minutes / 60.0) as i64); }
    let days = (minutes / 1440.0).round() as i64;
    match days { 7 => "Weekly limit".into(), 30 => "Monthly limit".into(), _ => format!("{days}d limit") }
}

fn account(path: &Path) -> Option<ProviderAccount> {
    let root: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    let token = root.get("tokens")?.get("id_token")?.as_str()?;
    let payload = token.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).ok()?;
    let claims: Value = serde_json::from_slice(&decoded).ok()?;
    Some(ProviderAccount {
        label: claims.get("email").and_then(Value::as_str).map(str::to_owned),
        plan: claims.pointer("/https:~1~1api.openai.com~1auth/chatgpt_plan_type").and_then(Value::as_str).map(str::to_owned),
        source: Some("Codex".into()),
    })
}

fn activity_from_modified(modified: SystemTime) -> Option<ActivitySummary> {
    let age = SystemTime::now().duration_since(modified).ok()?.as_secs();
    (age <= 8).then(|| ActivitySummary { state: "working".into(), label: Some("Working now".into()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_latest_rate_limit_line() {
        let input = r#"{"timestamp":"2026-09-05T12:00:00Z","type":"event_msg","payload":{"rate_limits":{"primary":{"used_percent":52.0,"window_minutes":300,"resets_at":1790000000},"secondary":{"used_percent":13.0,"window_minutes":10080,"resets_at":1790500000}}}}"#;
        let (windows, _) = parse_rollout(input).unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].label, "5h limit");
        assert!((windows[0].used_fraction - 0.52).abs() < 0.001);
    }

    #[test]
    fn parses_live_wham_usage() {
        let body = serde_json::json!({
            "plan_type": "plus",
            "rate_limit": {
                "primary_window": {
                    "used_percent": 100.0,
                    "limit_window_seconds": 18000,
                    "reset_at": 1790000000
                },
                "secondary_window": {
                    "used_percent": 32.0,
                    "limit_window_seconds": 604800,
                    "reset_at": 1790500000
                }
            }
        });
        let windows = parse_live_usage(&body).unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].id, "primary");
        assert_eq!(windows[0].label, "5h limit");
        assert!((windows[0].used_fraction - 1.0).abs() < 0.001);
        assert_eq!(windows[1].id, "secondary");
        assert_eq!(windows[1].label, "Weekly limit");
        assert!((windows[1].used_fraction - 0.32).abs() < 0.001);
    }

    #[test]
    fn classifies_windows_by_duration_not_wire_slot() {
        let body = serde_json::json!({
            "rate_limit": {
                "primary_window": {"used_percent": 32.0, "limit_window_seconds": 604800},
                "secondary_window": {"used_percent": 100.0, "limit_window_seconds": 18000}
            }
        });
        let windows = parse_live_usage(&body).unwrap();
        assert_eq!(windows[0].id, "primary");
        assert!((windows[0].used_fraction - 1.0).abs() < 0.001);
        assert_eq!(windows[1].id, "secondary");
        assert!((windows[1].used_fraction - 0.32).abs() < 0.001);
    }
}
