use crate::model::{LimitWindow, ProviderAccount, ProviderSnapshot};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

/// GitHub Copilot quotas from GitHub's editor endpoint, borrowing the token
/// from `GH_TOKEN`/`GITHUB_TOKEN`, `gh`'s hosts file, or `gh auth token`.
/// Mirrors upstream `GitHubCopilotProvider` (endpoint
/// `GET /copilot_internal/user`, `quota_snapshots` parsing).
const ENDPOINT: &str = "https://api.github.com/copilot_internal/user";
const MANAGE_URL: &str = "https://github.com/settings/copilot";

pub async fn snapshot() -> ProviderSnapshot {
    let credential = match load_credential() {
        Ok(value) => value,
        Err(message) => {
            return ProviderSnapshot::unavailable(
                "copilot",
                "GitHub Copilot",
                "⛁",
                "needsAuth",
                message,
                MANAGE_URL,
                account(),
            )
        }
    };
    let account = account().or(Some(ProviderAccount {
        label: None,
        plan: None,
        source: Some(credential.source.clone()),
    }));

    let client = match reqwest::Client::builder().timeout(Duration::from_secs(15)).build() {
        Ok(client) => client,
        Err(error) => {
            return ProviderSnapshot::unavailable(
                "copilot",
                "GitHub Copilot",
                "⛁",
                "error",
                error.to_string(),
                MANAGE_URL,
                account,
            )
        }
    };
    let response = match client
        .get(ENDPOINT)
        .header("Authorization", format!("Bearer {}", credential.token))
        .header("Accept", "application/json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "Codenotch")
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return ProviderSnapshot::unavailable(
                "copilot",
                "GitHub Copilot",
                "⛁",
                "error",
                format!("GitHub Copilot usage request failed: {error}"),
                MANAGE_URL,
                account,
            )
        }
    };
    if matches!(response.status().as_u16(), 401 | 403) {
        return ProviderSnapshot::unavailable(
            "copilot",
            "GitHub Copilot",
            "⛁",
            "needsAuth",
            "GitHub rejected the saved login. Run `gh auth login`, then enable GitHub Copilot.",
            MANAGE_URL,
            account,
        );
    }
    if !response.status().is_success() {
        let status = response.status();
        return ProviderSnapshot::unavailable(
            "copilot",
            "GitHub Copilot",
            "⛁",
            "error",
            format!("GitHub Copilot endpoint returned {status}"),
            MANAGE_URL,
            account,
        );
    }
    match response.json::<Value>().await {
        Ok(body) => match parse_usage(&body) {
            Ok(windows) => {
                let headline_id = windows
                    .iter()
                    .find(|window| window.id == "premium_interactions")
                    .or(windows.first())
                    .map(|window| window.id.clone());
                ProviderSnapshot {
                    id: "copilot".into(),
                    display_name: "GitHub Copilot".into(),
                    glyph: "⛁".into(),
                    fidelity: "official".into(),
                    status: "ok".into(),
                    windows,
                    headline_id,
                    fetched_at: Utc::now(),
                    message: None,
                    account,
                    manage_url: Some(MANAGE_URL.into()),
                    display_value: None,
                    activity: None,
                }
            }
            Err(message) => ProviderSnapshot::unavailable(
                "copilot",
                "GitHub Copilot",
                "⛁",
                "error",
                message,
                MANAGE_URL,
                account,
            ),
        },
        Err(error) => ProviderSnapshot::unavailable(
            "copilot",
            "GitHub Copilot",
            "⛁",
            "error",
            format!("Could not parse GitHub Copilot usage: {error}"),
            MANAGE_URL,
            account,
        ),
    }
}

struct Credential {
    token: String,
    source: String,
}

fn hosts_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("GitHub CLI")
            .join("hosts.yml")
    }
    #[cfg(not(target_os = "windows"))]
    {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
            .join("gh")
            .join("hosts.yml")
    }
}

fn load_credential() -> Result<Credential, String> {
    if let Some(token) = std::env::var_os("GH_TOKEN")
        .or_else(|| std::env::var_os("GITHUB_TOKEN"))
        .and_then(|value| value.into_string().ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        return Ok(Credential { token, source: "GitHub".into() });
    }
    let text = fs::read_to_string(hosts_path()).unwrap_or_default();
    let (username, token) = parse_hosts(&text);
    if let Some(token) = token {
        return Ok(Credential {
            token,
            source: "GitHub CLI".into(),
            // username retained for account(); token is what matters here.
        });
    }
    let _ = username;
    if let Some(token) = gh_token() {
        return Ok(Credential { token, source: "GitHub CLI".into() });
    }
    Err("Sign in with GitHub CLI using `gh auth login`, then enable GitHub Copilot.".into())
}

fn account() -> Option<ProviderAccount> {
    let text = fs::read_to_string(hosts_path()).ok()?;
    let (username, _) = parse_hosts(&text);
    username.map(|username| ProviderAccount {
        label: Some(username),
        plan: None,
        source: Some("GitHub".into()),
    })
}

fn gh_token() -> Option<String> {
    let candidates = if cfg!(target_os = "windows") {
        vec!["gh.exe", "gh"]
    } else {
        vec![
            "/opt/homebrew/bin/gh",
            "/usr/local/bin/gh",
            "/usr/bin/gh",
            "gh",
        ]
    };
    for candidate in candidates {
        let output = std::process::Command::new(candidate)
            .args(["auth", "token", "--hostname", "github.com"])
            .output()
            .ok()?;
        if !output.status.success() {
            continue;
        }
        let token = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !token.is_empty() {
            return Some(token);
        }
    }
    None
}

fn parse_hosts(text: &str) -> (Option<String>, Option<String>) {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|line| line.trim() == "github.com:");
    let Some(start) = start else { return (None, None) };
    let mut username = None;
    let mut token = None;
    for line in lines.iter().skip(start + 1) {
        if !(line.starts_with(' ') || line.starts_with('\t')) {
            break;
        }
        let trimmed = line.trim();
        if let Some(value) = yaml_value(trimmed, "user") {
            username = Some(value);
        }
        if let Some(value) = yaml_value(trimmed, "oauth_token") {
            token = Some(value);
        }
    }
    (username, token)
}

fn yaml_value(line: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    let rest = line.strip_prefix(&prefix)?.trim();
    let value = rest.trim_matches(|c: char| c == '"' || c == '\'').trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn parse_usage(root: &Value) -> Result<Vec<LimitWindow>, String> {
    let quotas = root
        .get("quota_snapshots")
        .and_then(Value::as_object)
        .ok_or("GitHub Copilot returned no quota snapshots.")?;
    let order = ["premium_interactions", "chat", "completions"];
    let mut keys: Vec<String> = order.iter().map(|key| key.to_string()).collect();
    let mut extra: Vec<String> = quotas.keys().filter(|key| !order.contains(&key.as_str())).cloned().collect();
    extra.sort();
    keys.extend(extra);
    let reset_fallback = parse_date(root.get("quota_reset_date"));
    let mut windows = Vec::new();
    for key in keys {
        let Some(quota) = quotas.get(&key) else { continue };
        if let Some(window) = window(&key, quota, reset_fallback) {
            windows.push(window);
        }
    }
    if windows.is_empty() {
        return Err("GitHub Copilot reported no metered quotas.".into());
    }
    Ok(windows)
}

fn window(id: &str, quota: &Value, fallback_reset: Option<DateTime<Utc>>) -> Option<LimitWindow> {
    if quota.get("unlimited").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let entitlement = quota.get("entitlement").and_then(Value::as_f64);
    let remaining = quota.get("remaining").and_then(Value::as_f64);
    let used = quota.get("used").and_then(Value::as_f64);
    let resets_at = parse_date(quota.get("reset_date").or_else(|| quota.get("reset_at")).or_else(|| quota.get("resets_at")))
        .or(fallback_reset);
    if entitlement == Some(0.0) {
        return None;
    }
    if let Some(entitlement) = entitlement.filter(|value| *value > 0.0) {
        let consumed = used.unwrap_or_else(|| (entitlement - remaining.unwrap_or(entitlement)).max(0.0));
        return Some(LimitWindow {
            id: id.into(),
            label: label(id),
            used_fraction: (consumed / entitlement).max(0.0),
            resets_at,
        });
    }
    if let Some(remaining) = remaining.filter(|_| used.is_none()) {
        if remaining >= 0.0 {
            return Some(LimitWindow {
                id: id.into(),
                label: label(id),
                // No denominator published; surface as a count-style reading.
                used_fraction: 0.0,
                resets_at,
            });
        }
    }
    if let Some(used) = used.filter(|value| *value >= 0.0) {
        let _ = used;
        return Some(LimitWindow {
            id: id.into(),
            label: label(id),
            used_fraction: 0.0,
            resets_at,
        });
    }
    None
}

fn label(id: &str) -> String {
    match id {
        "premium_interactions" => "Premium requests".into(),
        "chat" => "Chat requests".into(),
        "completions" => "Completions".into(),
        other => other
            .replace('_', " ")
            .split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                chars.next().map(|first| first.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn parse_date(value: Option<&Value>) -> Option<DateTime<Utc>> {
    match value {
        Some(Value::Number(number)) => {
            let seconds = number.as_f64()?;
            let seconds = if seconds > 10_000_000_000.0 { seconds / 1000.0 } else { seconds };
            DateTime::from_timestamp(seconds as i64, 0)
        }
        Some(Value::String(text)) => {
            let text = text.trim();
            DateTime::parse_from_rfc3339(text).ok().map(|date| date.with_timezone(&Utc))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_quota_snapshots() {
        let body = json!({
            "quota_snapshots": {
                "premium_interactions": {
                    "entitlement": 100.0, "remaining": 80.0,
                    "reset_date": "2026-10-01T00:00:00Z"
                },
                "chat": {"unlimited": true, "entitlement": 0, "remaining": 0}
            }
        });
        let windows = parse_usage(&body).unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].id, "premium_interactions");
        assert!((windows[0].used_fraction - 0.2).abs() < 0.001);
        assert!(windows[0].resets_at.is_some());
    }

    #[test]
    fn parses_hosts_file() {
        let text = "github.com:\n    user: octocat\n    oauth_token: secret\n";
        let (username, token) = parse_hosts(text);
        assert_eq!(username.as_deref(), Some("octocat"));
        assert_eq!(token.as_deref(), Some("secret"));
    }
}
