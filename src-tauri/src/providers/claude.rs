use crate::model::{ActivitySummary, LimitWindow, ProviderAccount, ProviderSnapshot};
use chrono::{DateTime, TimeZone, Utc};
use reqwest::header::{AUTHORIZATION, HeaderName, HeaderValue, USER_AGENT};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

const ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";
const MANAGE_URL: &str = "https://claude.ai/settings/usage";
// The OAuth usage endpoint partitions/throttles clients by request identity.
// Claude Code usage requests carry a `claude-code/<version>` user-agent; use
// the same compatibility prefix rather than falling into the generic 429 path.
const CLAUDE_USAGE_USER_AGENT: &str = "claude-code/2.1.34";

pub async fn snapshot() -> ProviderSnapshot {
    let credential_path = credential_path();
    let credential = match read_credential(&credential_path) {
        Ok(value) => value,
        Err(message) => return ProviderSnapshot::unavailable(
            "claude", "Claude", "✳", "needsAuth", message, MANAGE_URL, None,
        ),
    };
    if credential.expires_at <= Utc::now() {
        return ProviderSnapshot::unavailable(
            "claude", "Claude", "✳", "stale",
            "Claude Code's OAuth credential is expired. Run Claude Code once so it can refresh its own login.",
            MANAGE_URL,
            Some(ProviderAccount { label: None, plan: credential.subscription_type.clone(), source: Some("Claude Code".into()) }),
        );
    }

    let client = match reqwest::Client::builder().timeout(Duration::from_secs(15)).build() {
        Ok(client) => client,
        Err(error) => return ProviderSnapshot::unavailable("claude", "Claude", "✳", "error", error.to_string(), MANAGE_URL, None),
    };
    let beta = HeaderName::from_static("anthropic-beta");
    let response = match client.get(ENDPOINT)
        .header(AUTHORIZATION, format!("Bearer {}", credential.access_token))
        .header(beta, HeaderValue::from_static("oauth-2025-04-20"))
        .header(USER_AGENT, HeaderValue::from_static(CLAUDE_USAGE_USER_AGENT))
        .header("x-app", HeaderValue::from_static("cli"))
        .send().await
    {
        Ok(response) => response,
        Err(error) => return ProviderSnapshot::unavailable("claude", "Claude", "✳", "error", format!("Claude usage request failed: {error}"), MANAGE_URL, None),
    };
    if matches!(response.status().as_u16(), 401 | 403) {
        return ProviderSnapshot::unavailable("claude", "Claude", "✳", "needsAuth", "Claude rejected the saved Claude Code login. Run Claude Code and sign in again.", MANAGE_URL, None);
    }
    if response.status().as_u16() == 429 {
        return ProviderSnapshot::unavailable("claude", "Claude", "✳", "stale", "Claude usage is temporarily rate limited. Codenotch is backing off and will keep the last successful reading visible until the next safe retry.", MANAGE_URL, None);
    }
    if !response.status().is_success() {
        let status = response.status();
        return ProviderSnapshot::unavailable("claude", "Claude", "✳", "error", format!("Claude usage endpoint returned {status}"), MANAGE_URL, None);
    }
    match response.json::<Value>().await {
        Ok(body) => match parse_usage(&body) {
            Ok(windows) => ProviderSnapshot {
                id: "claude".into(), display_name: "Claude".into(), glyph: "✳".into(), fidelity: "official".into(), status: "ok".into(),
                windows, headline_id: Some("session".into()), fetched_at: Utc::now(), message: None,
                account: Some(ProviderAccount { label: None, plan: credential.subscription_type, source: Some("Claude Code".into()) }),
                manage_url: Some(MANAGE_URL.into()), display_value: None, activity: claude_activity(),
            },
            Err(message) => ProviderSnapshot::unavailable("claude", "Claude", "✳", "error", message, MANAGE_URL, None),
        },
        Err(error) => ProviderSnapshot::unavailable("claude", "Claude", "✳", "error", format!("Could not parse Claude usage: {error}"), MANAGE_URL, None),
    }
}

struct Credential {
    access_token: String,
    expires_at: DateTime<Utc>,
    subscription_type: Option<String>,
}

fn config_dir() -> PathBuf {
    std::env::var_os("CLAUDE_SECURESTORAGE_CONFIG_DIR").map(PathBuf::from)
        .or_else(|| std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from))
        .or_else(|| dirs::home_dir().map(|home| home.join(".claude")))
        .unwrap_or_else(|| PathBuf::from(".claude"))
}

fn credential_path() -> PathBuf { config_dir().join(".credentials.json") }

fn read_credential(path: &Path) -> Result<Credential, String> {
    let text = fs::read_to_string(path).map_err(|_| format!("Claude Code credentials were not found at {}. Run Claude Code and sign in first.", path.display()))?;
    let root: Value = serde_json::from_str(&text).map_err(|error| format!("Claude Code credential file is invalid JSON: {error}"))?;
    let oauth = root.get("claudeAiOauth").ok_or("Claude Code OAuth data is missing from its credential file.")?;
    let access_token = oauth.get("accessToken").and_then(Value::as_str).filter(|value| !value.is_empty()).ok_or("Claude Code access token is missing.")?.to_owned();
    let expires_at = parse_expiry(oauth.get("expiresAt").or_else(|| oauth.get("expires_at")))?;
    Ok(Credential {
        access_token,
        expires_at,
        subscription_type: oauth.get("subscriptionType").and_then(Value::as_str).map(str::to_owned),
    })
}

/// `expiresAt` is documented as milliseconds since the epoch, but tolerate
/// seconds and string forms so a format change degrades to a working reading
/// instead of a permanent "expired" status.
fn parse_expiry(value: Option<&Value>) -> Result<DateTime<Utc>, String> {
    let Some(value) = value else { return Err("Claude Code credential expiry is missing.".into()) };
    let millis = match value {
        Value::Number(number) => number.as_f64().ok_or("Claude Code credential expiry is invalid.")?,
        Value::String(text) => text.trim().parse::<f64>().map_err(|_| "Claude Code credential expiry is invalid.")?,
        _ => return Err("Claude Code credential expiry is invalid.".into()),
    };
    if !millis.is_finite() || millis <= 0.0 {
        return Err("Claude Code credential expiry is invalid.".into());
    }
    // ms since epoch is ~1.7e12 today; seconds would be ~1.7e9.
    let millis = if millis < 1e12 { millis * 1000.0 } else { millis };
    Utc.timestamp_millis_opt(millis as i64).single().ok_or("Claude Code credential expiry is invalid.".into())
}

fn parse_usage(root: &Value) -> Result<Vec<LimitWindow>, String> {
    let mut windows = Vec::new();
    if let Some(limits) = root.get("limits").and_then(Value::as_array) {
        for item in limits {
            let Some(kind) = item.get("kind").and_then(Value::as_str) else { continue };
            // Current responses use `percent`; tolerate `utilization` too.
            let Some(percent) = item.get("percent").or_else(|| item.get("utilization")).and_then(Value::as_f64) else { continue };
            let resets_at = parse_date(item.get("resets_at").or_else(|| item.get("resetsAt")));
            windows.push(LimitWindow { id: kind.into(), label: label(kind), used_fraction: percent / 100.0, resets_at });
        }
    }
    merge_named(&mut windows, root.get("five_hour").or_else(|| root.get("fiveHour")), "session", "Current session");
    merge_named(&mut windows, root.get("seven_day").or_else(|| root.get("sevenDay")), "weekly_all", "All models");
    if windows.is_empty() { Err("Claude returned no usage windows.".into()) } else {
        windows.sort_by_key(|window| match window.id.as_str() { "session" => 0, "weekly_all" => 1, _ => 2 });
        Ok(windows)
    }
}

fn merge_named(windows: &mut Vec<LimitWindow>, value: Option<&Value>, id: &str, label_text: &str) {
    if windows.iter().any(|window| window.id == id) { return; }
    let Some(value) = value else { return };
    let Some(percent) = value.get("utilization").and_then(Value::as_f64) else { return };
    windows.push(LimitWindow { id: id.into(), label: label_text.into(), used_fraction: percent / 100.0, resets_at: parse_date(value.get("resets_at").or_else(|| value.get("resetsAt"))) });
}

fn parse_date(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let text = value?.as_str()?.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(date) = DateTime::parse_from_rfc3339(text) {
        return Some(date.with_timezone(&Utc));
    }
    // Tolerate a space separator or a missing offset (assume UTC).
    let normalized = text.replace(' ', "T");
    if let Ok(date) = DateTime::parse_from_rfc3339(&normalized) {
        return Some(date.with_timezone(&Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some(naive.and_utc());
    }
    None
}

fn label(kind: &str) -> String {
    match kind {
        "session" => "Current session".into(), "weekly_all" => "All models".into(),
        "weekly_opus" => "Opus".into(), "weekly_sonnet" => "Sonnet".into(),
        other => other.trim_start_matches("weekly_").replace('_', " ").split_whitespace()
            .map(|word| { let mut chars = word.chars(); chars.next().map(|first| first.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default() })
            .collect::<Vec<_>>().join(" "),
    }
}

fn claude_activity() -> Option<ActivitySummary> {
    let projects = config_dir().join("projects");
    let newest = WalkDir::new(projects).max_depth(5).into_iter().filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file() && entry.path().extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|entry| entry.metadata().ok()?.modified().ok()).max()?;
    let age = SystemTime::now().duration_since(newest).ok()?.as_secs();
    (age <= 8).then(|| ActivitySummary { state: "working".into(), label: Some("Working now".into()) })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_named_windows() {
        let body: Value = serde_json::json!({"five_hour":{"utilization":73.0,"resets_at":"2026-09-05T17:00:00Z"},"seven_day":{"utilization":7.0,"resets_at":"2026-09-10T00:00:00Z"}});
        let windows = parse_usage(&body).unwrap();
        assert_eq!(windows[0].id, "session");
        assert!((windows[0].used_fraction - 0.73).abs() < 0.001);
    }

    #[test]
    fn parses_live_oauth_shape() {
        // Shape of GET /api/oauth/usage as observed 2026-09-06: fractional
        // seconds with a numeric offset, integer percents, and extra null keys.
        let body: Value = serde_json::json!({
            "five_hour": {"utilization": 0.0, "resets_at": "2026-09-06T04:50:00.458196+00:00"},
            "seven_day": {"utilization": 0.0, "resets_at": "2026-09-12T21:00:00.458213+00:00"},
            "seven_day_opus": null,
            "limits": [
                {"kind": "session", "percent": 0, "resets_at": "2026-09-06T04:50:00.458196+00:00"},
                {"kind": "weekly_all", "percent": 0, "resets_at": "2026-09-12T21:00:00.458213+00:00"}
            ]
        });
        let windows = parse_usage(&body).unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].id, "session");
        assert_eq!(windows[1].id, "weekly_all");
        assert!(windows.iter().all(|window| window.resets_at.is_some()));
    }

    #[test]
    fn expiry_accepts_seconds_and_strings() {
        let base = Utc::now() + chrono::Duration::hours(2);
        let as_seconds = base.timestamp() as f64;
        let parsed = parse_expiry(Some(&serde_json::json!(as_seconds))).unwrap();
        assert!((parsed - base).num_seconds().abs() < 5);
        let as_string = serde_json::json!(as_seconds.to_string());
        let parsed = parse_expiry(Some(&as_string)).unwrap();
        assert!((parsed - base).num_seconds().abs() < 5);
    }
}
