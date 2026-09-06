use crate::model::{LimitWindow, ProviderAccount, ProviderSnapshot};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

const MANAGE_URL: &str = "https://opencode.ai/docs/zen/";
const USAGE_ENDPOINT: &str = "https://opencode.ai/zen/go/v1/usage";

pub async fn snapshot() -> ProviderSnapshot {
    let api_key = match read_api_key() {
        Ok(key) => key,
        Err(message) => return ProviderSnapshot::unavailable(
            "opencode", "OpenCode", "▣", "needsAuth", message, MANAGE_URL, None,
        ),
    };
    let account = Some(ProviderAccount {
        label: None,
        plan: Some("Zen".into()),
        source: Some("OpenCode".into()),
    });

    let client = match reqwest::Client::builder().timeout(Duration::from_secs(15)).build() {
        Ok(client) => client,
        Err(error) => return ProviderSnapshot::unavailable("opencode", "OpenCode", "▣", "error", error.to_string(), MANAGE_URL, account),
    };
    let response = match client
        .get(USAGE_ENDPOINT)
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => return ProviderSnapshot::unavailable("opencode", "OpenCode", "▣", "error", format!("OpenCode usage request failed: {error}"), MANAGE_URL, account),
    };
    if matches!(response.status().as_u16(), 401 | 403) {
        return ProviderSnapshot::unavailable("opencode", "OpenCode", "▣", "needsAuth", "OpenCode rejected the saved Zen API key. Run `/connect` in OpenCode and paste a fresh key.", MANAGE_URL, account);
    }
    if !response.status().is_success() {
        let status = response.status();
        return ProviderSnapshot::unavailable("opencode", "OpenCode", "▣", "error", format!("OpenCode usage endpoint returned {status}"), MANAGE_URL, account);
    }
    match response.json::<Value>().await {
        Ok(body) => match parse_usage(&body) {
            Ok(windows) => {
                // Headline the most-constrained window: a full monthly quota
                // blocks usage even when the rolling window looks fine.
                let headline = headline_id(&windows);
                ProviderSnapshot {
                    id: "opencode".into(), display_name: "OpenCode".into(), glyph: "▣".into(),
                    fidelity: "official".into(), status: "ok".into(), windows,
                    headline_id: headline, fetched_at: Utc::now(), message: None,
                    account, manage_url: Some(MANAGE_URL.into()), display_value: None, activity: None,
                }
            }
            Err(message) => ProviderSnapshot::unavailable("opencode", "OpenCode", "▣", "error", message, MANAGE_URL, account),
        },
        Err(error) => ProviderSnapshot::unavailable("opencode", "OpenCode", "▣", "error", format!("Could not decode OpenCode usage: {error}"), MANAGE_URL, account),
    }
}

/// OpenCode stores `/connect` keys in `~/.local/share/opencode/auth.json` on
/// every platform (XDG-style even on Windows), keyed by provider id.
fn auth_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".local").join("share").join("opencode").join("auth.json"));
    }
    if let Some(data) = dirs::data_dir() {
        let fallback = data.join("opencode").join("auth.json");
        if !paths.contains(&fallback) {
            paths.push(fallback);
        }
    }
    paths
}

fn read_api_key() -> Result<String, String> {
    let searched = auth_paths();
    let mut tried = Vec::new();
    for path in &searched {
        tried.push(path.display().to_string());
        let Ok(text) = fs::read_to_string(path) else { continue };
        let Ok(root) = serde_json::from_str::<Value>(&text) else { continue };
        if let Some(key) = root.get("opencode").and_then(|entry| entry.get("key")).and_then(Value::as_str).filter(|key| !key.is_empty()) {
            return Ok(key.to_owned());
        }
        return Err("OpenCode is not connected to Zen (no `opencode` key in auth.json). Run `/connect` in OpenCode and choose OpenCode Zen.".into());
    }
    Err(format!("OpenCode auth was not found (looked in {}). Run `/connect` in OpenCode first.", tried.join(", ")))
}

/// Headline the most-constrained window so a full monthly quota reads 100%
/// on the ring even when the rolling window looks fine.
fn headline_id(windows: &[LimitWindow]) -> Option<String> {
    windows
        .iter()
        .max_by(|a, b| a.used_fraction.partial_cmp(&b.used_fraction).unwrap_or(std::cmp::Ordering::Equal))
        .map(|window| window.id.clone())
}

fn parse_usage(root: &Value) -> Result<Vec<LimitWindow>, String> {    let usage = root.get("usage").unwrap_or(root);
    let mut windows = Vec::new();
    for (id, label) in [("rolling", "Current"), ("weekly", "Weekly"), ("monthly", "Monthly")] {
        let Some(bucket) = usage.get(id) else { continue };
        let Some(percent) = bucket.get("percent").and_then(Value::as_f64) else { continue };
        let resets_at = bucket
            .get("resetsAt").or_else(|| bucket.get("resets_at"))
            .and_then(|value| value.as_str())
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .map(|date| date.with_timezone(&Utc));
        windows.push(LimitWindow { id: id.into(), label: label.into(), used_fraction: percent / 100.0, resets_at });
    }
    if windows.is_empty() { Err("OpenCode returned no usage windows.".into()) } else { Ok(windows) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_zen_go_usage() {
        let body: Value = serde_json::json!({
            "usage": {
                "rolling": {"status": "ok", "percent": 0, "resetsAt": "2026-09-06T05:09:17.909Z"},
                "weekly": {"status": "ok", "percent": 16, "resetsAt": "2026-09-07T00:00:00.909Z"},
                "monthly": {"status": "rate-limited", "percent": 100, "resetsAt": "2026-09-19T10:11:08.909Z"}
            }
        });
        let windows = parse_usage(&body).unwrap();
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0].id, "rolling");
        assert!((windows[0].used_fraction).abs() < 0.001);
        assert!((windows[1].used_fraction - 0.16).abs() < 0.001);
        assert!((windows[2].used_fraction - 1.0).abs() < 0.001);
        assert!(windows[0].resets_at.is_some());
    }

    #[test]
    fn rejects_empty_usage() {
        let body: Value = serde_json::json!({"usage": {}});
        assert!(parse_usage(&body).is_err());
    }

    #[test]
    fn headlines_full_monthly_over_empty_rolling() {
        let body: Value = serde_json::json!({
            "usage": {
                "rolling": {"status": "ok", "percent": 0, "resetsAt": "2026-09-06T05:09:17.909Z"},
                "weekly": {"status": "ok", "percent": 16, "resetsAt": "2026-09-07T00:00:00.909Z"},
                "monthly": {"status": "rate-limited", "percent": 100, "resetsAt": "2026-09-19T10:11:08.909Z"}
            }
        });
        let windows = parse_usage(&body).unwrap();
        assert_eq!(headline_id(&windows).as_deref(), Some("monthly"));
    }
}
