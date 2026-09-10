use crate::model::{LimitWindow, ProviderAccount, ProviderSnapshot};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Grok Build credits from the same billing endpoint the Grok CLI's `/usage`
/// uses, reading the CLI's own `~/.grok/auth.json` session (read-only).
/// Mirrors upstream `GrokLocalProvider` + `GrokUsage`.
const ENDPOINT: &str = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
const MANAGE_URL: &str = "https://grok.com/?_s=usage";
const TRUSTED_ISSUER: &str = "https://auth.x.ai";

pub async fn snapshot() -> ProviderSnapshot {
    let credential = match load(&auth_path()) {
        Ok(value) => value,
        Err(message) => {
            return ProviderSnapshot::unavailable("grok", "Grok", "⛭", "needsAuth", message, MANAGE_URL, account());
        }
    };
    let account = account().or(Some(ProviderAccount {
        label: credential.email.clone(),
        plan: None,
        source: Some("Grok".into()),
    }));
    if credential.expires_at <= Utc::now() {
        return ProviderSnapshot::unavailable(
            "grok",
            "Grok",
            "⛭",
            "stale",
            "Grok's saved CLI session is expired. Run `grok login` so it can refresh its own token.",
            MANAGE_URL,
            account,
        );
    }

    let client = match reqwest::Client::builder().timeout(Duration::from_secs(15)).build() {
        Ok(client) => client,
        Err(error) => {
            return ProviderSnapshot::unavailable("grok", "Grok", "⛭", "error", error.to_string(), MANAGE_URL, account)
        }
    };
    let response = match client
        .get(ENDPOINT)
        .header("Authorization", format!("Bearer {}", credential.access_token))
        .header("X-XAI-Token-Auth", "xai-grok-cli")
        .header("Accept", "application/json")
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return ProviderSnapshot::unavailable(
                "grok",
                "Grok",
                "⛭",
                "error",
                format!("Grok usage request failed: {error}"),
                MANAGE_URL,
                account,
            )
        }
    };
    if matches!(response.status().as_u16(), 401 | 403) {
        return ProviderSnapshot::unavailable(
            "grok",
            "Grok",
            "⛭",
            "needsAuth",
            "Grok rejected the saved CLI session. Run `grok login` and sign in again.",
            MANAGE_URL,
            account,
        );
    }
    if response.status().as_u16() == 429 {
        return ProviderSnapshot::unavailable(
            "grok",
            "Grok",
            "⛭",
            "stale",
            "Grok usage is temporarily rate limited. Codenotch is backing off and will retry automatically.",
            MANAGE_URL,
            account,
        );
    }
    if !response.status().is_success() {
        let status = response.status();
        return ProviderSnapshot::unavailable(
            "grok",
            "Grok",
            "⛭",
            "error",
            format!("Grok usage endpoint returned {status}"),
            MANAGE_URL,
            account,
        );
    }
    match response.text().await {
        Ok(text) => match parse_usage(&text) {
            Ok(windows) => ProviderSnapshot {
                id: "grok".into(),
                display_name: "Grok".into(),
                glyph: "⛭".into(),
                fidelity: "official".into(),
                status: "ok".into(),
                windows,
                headline_id: Some("credits".into()),
                fetched_at: Utc::now(),
                message: None,
                account,
                manage_url: Some(MANAGE_URL.into()),
                display_value: None,
                activity: None,
            },
            Err(message) => ProviderSnapshot::unavailable("grok", "Grok", "⛭", "error", message, MANAGE_URL, account),
        },
        Err(error) => ProviderSnapshot::unavailable(
            "grok",
            "Grok",
            "⛭",
            "error",
            format!("Could not read Grok usage: {error}"),
            MANAGE_URL,
            account,
        ),
    }
}

struct Credential {
    access_token: String,
    expires_at: DateTime<Utc>,
    email: Option<String>,
}

fn auth_path() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".grok").join("auth.json")
}

fn account() -> Option<ProviderAccount> {
    let credential = load(&auth_path()).ok()?;
    Some(ProviderAccount { label: credential.email, plan: None, source: Some("Grok".into()) })
}

fn load(path: &Path) -> Result<Credential, String> {
    let text = fs::read_to_string(path).map_err(|_| "Run `grok login` — it signs in and refreshes the token this reads.".to_string())?;
    let root: Value =
        serde_json::from_str(&text).map_err(|_| "Grok auth.json is invalid JSON.".to_string())?;
    let entry = pick(&root).ok_or("Grok CLI session was not found in auth.json.".to_string())?;
    let access_token = entry
        .get("key")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("Grok CLI access token is missing.".to_string())?
        .to_owned();
    let expires_at = entry
        .get("expires_at")
        .and_then(Value::as_str)
        .and_then(parse_date)
        .unwrap_or_else(|| Utc::now() + chrono::Duration::days(30));
    let email = entry.get("email").and_then(Value::as_str).map(str::to_owned);
    Ok(Credential { access_token, expires_at, email })
}

/// The file is keyed by `issuer::client_id`; only a session minted by xAI
/// itself is trusted. Prefer a still-live entry, else the first trusted one.
fn pick(root: &Value) -> Option<Value> {
    let object = root.as_object()?;
    let mut trusted: Vec<&Value> = object
        .iter()
        .filter(|(key, entry)| is_trusted(key, entry))
        .map(|(_, entry)| entry)
        .collect();
    if trusted.is_empty() {
        return None;
    }
    let now = Utc::now();
    trusted.sort_by_key(|entry| {
        entry.get("expires_at").and_then(Value::as_str).and_then(parse_date).map(|date| {
            if date > now { 0 } else { 1 }
        }).unwrap_or(0)
    });
    trusted.into_iter().next().cloned()
}

fn is_trusted(key: &str, entry: &Value) -> bool {
    if key.starts_with(TRUSTED_ISSUER) {
        return true;
    }
    entry.get("oidc_issuer").and_then(Value::as_str) == Some(TRUSTED_ISSUER)
}

fn parse_date(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text.trim()).ok().map(|date| date.with_timezone(&Utc))
}

fn parse_usage(text: &str) -> Result<Vec<LimitWindow>, String> {
    let root: Value = serde_json::from_str(text).map_err(|_| "Grok returned an unreadable usage response.".to_string())?;
    let config = root.get("config").ok_or("Grok returned no usage config.")?;
    let mut windows = Vec::new();

    let period = config.get("currentPeriod");
    let end = period.and_then(|period| period.get("end")).and_then(Value::as_str).and_then(parse_date);
    let resets_at = end.or_else(|| config.get("billingPeriodEnd").and_then(Value::as_str).and_then(parse_date));

    if let Some(percent) = config.get("creditUsagePercent").and_then(Value::as_f64) {
        let label = config
            .get("productUsage")
            .and_then(Value::as_array)
            .and_then(|products| products.first())
            .and_then(|product| product.get("product"))
            .and_then(Value::as_str)
            .map(humanize)
            .unwrap_or_else(|| "Grok Build".into());
        windows.push(LimitWindow { id: "credits".into(), label, used_fraction: percent / 100.0, resets_at });
    } else if let Some(products) = config.get("productUsage").and_then(Value::as_array) {
        for (index, product) in products.iter().enumerate() {
            let Some(percent) = product.get("usagePercent").and_then(Value::as_f64) else { continue };
            let name = product.get("product").and_then(Value::as_str).map(humanize).unwrap_or_else(|| "Usage".into());
            let id = if index == 0 {
                "credits".to_string()
            } else {
                product.get("product").and_then(Value::as_str).unwrap_or(&name).to_owned()
            };
            windows.push(LimitWindow { id, label: name, used_fraction: percent / 100.0, resets_at });
        }
    }

    // A fresh weekly period states its window but no usage yet — mirror Grok's
    // own `/usage` with a 0% weekly bar rather than "unmetered".
    if windows.is_empty() {
        let weekly = period.and_then(|period| period.get("type")).and_then(Value::as_str).is_some_and(|kind| kind.contains("WEEKLY"));
        if weekly {
            windows.push(LimitWindow {
                id: "credits".into(),
                label: "Weekly limit".into(),
                used_fraction: 0.0,
                resets_at,
            });
        }
    }

    if windows.is_empty() {
        return Err("Grok has nothing metered on this account yet.".into());
    }
    Ok(windows)
}

fn humanize(name: &str) -> String {
    let mut result = String::new();
    for character in name.chars() {
        if character.is_uppercase() && !result.is_empty() {
            result.push(' ');
        }
        result.push(character);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_credits_weekly() {
        let text = r#"{"config":{"currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY","start":"2026-09-05T08:21:18Z","end":"2026-09-12T08:21:18Z"},"creditUsagePercent":8.0,"productUsage":[{"product":"GrokBuild","usagePercent":8.0}],"billingPeriodEnd":"2026-09-12T08:21:18Z"}}"#;
        let windows = parse_usage(text).unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].id, "credits");
        assert_eq!(windows[0].label, "Grok Build");
        assert!((windows[0].used_fraction - 0.08).abs() < 0.001);
        assert!(windows[0].resets_at.is_some());
    }

    #[test]
    fn fresh_weekly_period_is_zero_not_unmetered() {
        let text = r#"{"config":{"currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY","end":"2026-09-12T08:21:18Z"}}}"#;
        let windows = parse_usage(text).unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].used_fraction, 0.0);
    }

    #[test]
    fn untrusted_issuer_is_rejected() {
        let root: Value = serde_json::json!({"https://customer-idp.example::app": {"key": "x"}});
        assert!(pick(&root).is_none());
    }
}
