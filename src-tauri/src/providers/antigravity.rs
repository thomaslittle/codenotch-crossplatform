use crate::model::{LimitWindow, ProviderAccount, ProviderSnapshot};
use base64::Engine;
use chrono::{DateTime, Utc};
use keyring::Entry;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

const MANAGE_URL: &str = "https://antigravity.google/";
const INFO_ENDPOINT: &str = "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist";
const QUOTA_ENDPOINT: &str = "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary";

pub async fn snapshot() -> ProviderSnapshot {
    let credential = match read_credential() {
        Ok(value) => value,
        Err(message) => return ProviderSnapshot::unavailable(
            "gemini", "Antigravity", "◆", "needsAuth", message, MANAGE_URL, None,
        ),
    };
    let account = Some(ProviderAccount {
        label: None,
        plan: Some(if credential.auth_method == "consumer" { "Personal".into() } else { credential.auth_method.clone() }),
        source: Some("Antigravity".into()),
    });
    if credential.expires_at <= Utc::now() {
        return ProviderSnapshot::unavailable(
            "gemini", "Antigravity", "◆", "stale",
            "Antigravity's saved Google token is expired. Open Antigravity so it can refresh its own login.",
            MANAGE_URL, account,
        );
    }

    let client = match reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build() {
        Ok(client) => client,
        Err(error) => return ProviderSnapshot::unavailable("gemini", "Antigravity", "◆", "error", error.to_string(), MANAGE_URL, account),
    };

    let bearer = format!("Bearer {}", credential.access_token);
    let info_response = client.post(INFO_ENDPOINT)
        .header(AUTHORIZATION, &bearer).header(CONTENT_TYPE, "application/json")
        .json(&serde_json::json!({"metadata":{"pluginType":"GEMINI"}})).send().await;
    match info_response {
        Ok(response) if matches!(response.status().as_u16(), 401 | 403) => {
            return ProviderSnapshot::unavailable("gemini", "Antigravity", "◆", "needsAuth", "Antigravity rejected its saved Google login. Open Antigravity and sign in again.", MANAGE_URL, account);
        }
        Ok(response) if !response.status().is_success() && response.status().as_u16() != 429 => {
            return ProviderSnapshot::unavailable("gemini", "Antigravity", "◆", "error", format!("Antigravity account endpoint returned {}", response.status()), MANAGE_URL, account);
        }
        Err(error) => {
            return ProviderSnapshot::unavailable("gemini", "Antigravity", "◆", "error", format!("Antigravity account request failed: {error}"), MANAGE_URL, account);
        }
        _ => {}
    }

    if let Ok(response) = client.post(QUOTA_ENDPOINT)
        .header(AUTHORIZATION, &bearer).header(CONTENT_TYPE, "application/json")
        .body("{}").send().await
    {
        if response.status().is_success() {
            if let Ok(body) = response.json::<Value>().await {
                let windows = parse_quota(&body);
                if !windows.is_empty() {
                    return ProviderSnapshot {
                        id: "gemini".into(), display_name: "Antigravity".into(), glyph: "◆".into(), fidelity: "official".into(),
                        status: "ok".into(), windows, headline_id: None, fetched_at: Utc::now(), message: None,
                        account, manage_url: Some(MANAGE_URL.into()), display_value: None, activity: None,
                    };
                }
            }
        }
    }

    let requests = requests_today();
    let display = requests.map(|count| format!("~{count}"));
    let message = match requests {
        Some(count) => format!("~{count} request{} today · Google publishes no limit for this account", if count == 1 { "" } else { "s" }),
        None => "Google publishes no metered quota for this Antigravity account.".into(),
    };
    ProviderSnapshot {
        id: "gemini".into(), display_name: "Antigravity".into(), glyph: "◆".into(), fidelity: "derived".into(),
        status: "ok".into(), windows: Vec::new(), headline_id: None, fetched_at: Utc::now(), message: Some(message),
        account, manage_url: Some(MANAGE_URL.into()), display_value: display, activity: None,
    }
}

struct Credential {
    access_token: String,
    expires_at: DateTime<Utc>,
    auth_method: String,
}

fn credential_entry() -> Result<Entry, String> {
    #[cfg(target_os = "windows")]
    {
        Entry::new_with_target("gemini:antigravity", "gemini", "antigravity").map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Entry::new("gemini", "antigravity").map_err(|e| e.to_string())
    }
}

fn read_credential() -> Result<Credential, String> {
    let mut text = credential_entry()?.get_password().map_err(|error| format!("Antigravity credential is unavailable: {error}"))?;
    if let Some(rest) = text.strip_prefix("go-keyring-base64:") { text = rest.to_owned(); }
    let bytes = base64::engine::general_purpose::STANDARD.decode(text.trim()).map_err(|_| "Antigravity credential is not valid base64".to_string())?;
    let root: Value = serde_json::from_slice(&bytes).map_err(|error| format!("Antigravity credential is invalid JSON: {error}"))?;
    let token = root.get("token").ok_or("Antigravity token is missing")?;
    let access_token = token.get("access_token").and_then(Value::as_str).filter(|value| !value.is_empty()).ok_or("Antigravity access token is missing")?.to_owned();
    let expiry = token.get("expiry").and_then(Value::as_str).ok_or("Antigravity token expiry is missing")?;
    let expires_at = DateTime::parse_from_rfc3339(expiry).map_err(|_| "Antigravity token expiry is invalid")?.with_timezone(&Utc);
    let auth_method = root.get("auth_method").and_then(Value::as_str).unwrap_or("unknown").to_owned();
    Ok(Credential { access_token, expires_at, auth_method })
}

fn parse_quota(root: &Value) -> Vec<LimitWindow> {
    let mut buckets: Vec<&Value> = root.get("buckets").and_then(Value::as_array).map(|values| values.iter().collect()).unwrap_or_default();
    if let Some(groups) = root.get("quotaGroups").and_then(Value::as_array) {
        for group in groups {
            if let Some(values) = group.get("buckets").and_then(Value::as_array) { buckets.extend(values); }
        }
    }
    buckets.into_iter().filter_map(|bucket| {
        let limit = bucket.get("limit")?.as_f64()?;
        let used = bucket.get("used")?.as_f64()?;
        if limit <= 0.0 || used < 0.0 || used > limit * 1.5 { return None; }
        let name = bucket.get("name").and_then(Value::as_str).unwrap_or("usage");
        let label = bucket.get("displayName").and_then(Value::as_str).unwrap_or(name);
        let resets_at = bucket.get("resetTime").and_then(Value::as_str)
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok()).map(|value| value.with_timezone(&Utc));
        Some(LimitWindow { id: name.into(), label: label.into(), used_fraction: used / limit, resets_at })
    }).collect()
}

fn requests_today() -> Option<u64> {
    let root = dirs::home_dir()?.join(".gemini/antigravity/brain");
    let today = chrono::Local::now().date_naive();
    let mut count = 0u64;
    for trajectory in fs::read_dir(root).ok()?.filter_map(Result::ok) {
        let path: PathBuf = trajectory.path().join(".system_generated/logs/transcript.jsonl");
        let Ok(text) = fs::read_to_string(path) else { continue };
        for line in text.lines() {
            let Ok(value) = serde_json::from_str::<Value>(line) else { continue };
            if value.get("source").and_then(Value::as_str) != Some("MODEL") { continue; }
            let Some(stamp) = value.get("created_at").and_then(Value::as_str) else { continue };
            let Ok(at) = DateTime::parse_from_rfc3339(stamp) else { continue };
            if at.with_timezone(&chrono::Local).date_naive() == today { count += 1; }
        }
    }
    Some(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_quota_buckets() {
        let body = serde_json::json!({"buckets":[{"name":"gemini-weekly","displayName":"Gemini weekly","used":25,"limit":100,"resetTime":"2026-09-10T00:00:00Z"}]});
        let windows = parse_quota(&body);
        assert_eq!(windows.len(), 1);
        assert!((windows[0].used_fraction - 0.25).abs() < 0.001);
    }
}
