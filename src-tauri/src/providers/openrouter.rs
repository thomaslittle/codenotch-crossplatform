use crate::model::{LimitWindow, ProviderAccount, ProviderSnapshot};
use base64::Engine;
use chrono::Utc;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MANAGE_URL: &str = "https://openrouter.ai/settings/keys";
const KEY_ENDPOINT: &str = "https://openrouter.ai/api/v1/key";
const CREDITS_ENDPOINT: &str = "https://openrouter.ai/api/v1/credits";

#[derive(Debug, Clone)]
struct Credential {
    key: String,
    source: String,
}

pub async fn snapshot() -> ProviderSnapshot {
    let credential = match read_api_key() {
        Ok(credential) => credential,
        Err(message) => {
            return ProviderSnapshot::unavailable(
                "openrouter",
                "OpenRouter",
                "OR",
                "needsAuth",
                message,
                MANAGE_URL,
                None,
            )
        }
    };

    let account = |label: Option<String>, free_tier: Option<bool>| {
        Some(account_from_source(&credential.source, label, free_tier))
    };

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return ProviderSnapshot::unavailable(
                "openrouter",
                "OpenRouter",
                "OR",
                "error",
                error.to_string(),
                MANAGE_URL,
                account(None, None),
            )
        }
    };

    let response = match client
        .get(KEY_ENDPOINT)
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", credential.key),
        )
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return ProviderSnapshot::unavailable(
                "openrouter",
                "OpenRouter",
                "OR",
                "error",
                format!("OpenRouter usage request failed: {error}"),
                MANAGE_URL,
                account(None, None),
            )
        }
    };

    if matches!(response.status().as_u16(), 401 | 403) {
        return ProviderSnapshot::unavailable(
            "openrouter",
            "OpenRouter",
            "OR",
            "needsAuth",
            "OpenRouter rejected the discovered API key. Refresh the key in OpenRouter, OpenCode, or the T3 provider instance that owns it.",
            MANAGE_URL,
            account(None, None),
        );
    }
    if !response.status().is_success() {
        let status = response.status();
        return ProviderSnapshot::unavailable(
            "openrouter",
            "OpenRouter",
            "OR",
            "error",
            format!("OpenRouter key endpoint returned {status}"),
            MANAGE_URL,
            account(None, None),
        );
    }

    let body = match response.json::<Value>().await {
        Ok(body) => body,
        Err(error) => {
            return ProviderSnapshot::unavailable(
                "openrouter",
                "OpenRouter",
                "OR",
                "error",
                format!("Could not decode OpenRouter usage: {error}"),
                MANAGE_URL,
                account(None, None),
            )
        }
    };

    // /key describes the routing key itself. The separate /credits endpoint
    // contains account-level purchased credits and usage. OpenRouter's current
    // reference labels /credits as management-key-only, but live routing keys
    // can also be accepted on some accounts. Try the same key safely and fall
    // back to per-key spend data if OpenRouter returns 401/403.
    let credit_balance = fetch_credit_balance(&client, &credential).await;

    match parse_key_usage(&body, &credential.source, credit_balance) {
        Ok(snapshot) => snapshot,
        Err(message) => ProviderSnapshot::unavailable(
            "openrouter",
            "OpenRouter",
            "OR",
            "error",
            message,
            MANAGE_URL,
            account(None, None),
        ),
    }
}

async fn fetch_credit_balance(client: &reqwest::Client, credential: &Credential) -> Option<f64> {
    let response = client
        .get(CREDITS_ENDPOINT)
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", credential.key),
        )
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body = response.json::<Value>().await.ok()?;
    let data = body.get("data")?.as_object()?;
    let total_credits = data.get("total_credits")?.as_f64()?;
    let total_usage = data.get("total_usage")?.as_f64()?;
    if !total_credits.is_finite() || !total_usage.is_finite() {
        return None;
    }
    Some((total_credits - total_usage).max(0.0))
}

fn account_from_source(
    source: &str,
    label: Option<String>,
    free_tier: Option<bool>,
) -> ProviderAccount {
    ProviderAccount {
        label,
        plan: free_tier.map(|free| if free { "Free".into() } else { "Paid".into() }),
        source: Some(source.into()),
    }
}

fn parse_key_usage(
    root: &Value,
    source: &str,
    account_balance: Option<f64>,
) -> Result<ProviderSnapshot, String> {
    let data = root
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| "OpenRouter key response did not contain a data object.".to_string())?;

    let label = data
        .get("label")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let free_tier = data.get("is_free_tier").and_then(Value::as_bool);
    let account = Some(account_from_source(source, label, free_tier));

    let limit = data.get("limit").and_then(Value::as_f64);
    let remaining = data.get("limit_remaining").and_then(Value::as_f64);
    let reset = data
        .get("limit_reset")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase);
    let weekly_spend = data
        .get("usage_weekly")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite());

    let mut windows = Vec::new();
    let mut headline_id = None;
    let mut display_value = None;
    let mut message = None;

    if let (Some(limit), Some(remaining)) = (limit, remaining) {
        if limit > 0.0 && limit.is_finite() && remaining.is_finite() {
            let id = match reset.as_deref() {
                Some("daily") => "daily",
                Some("weekly") => "weekly",
                Some("monthly") => "monthly",
                _ => "key-limit",
            };
            let label = match reset.as_deref() {
                Some("daily") => "Daily key limit",
                Some("weekly") => "Weekly key limit",
                Some("monthly") => "Monthly key limit",
                _ => "Key limit",
            };
            windows.push(LimitWindow {
                id: id.into(),
                label: label.into(),
                used_fraction: ((limit - remaining) / limit).clamp(0.0, 1.0),
                resets_at: None,
            });
            headline_id = Some(id.into());
            if let Some(balance) = account_balance {
                message = Some(format!("OpenRouter account balance ${balance:.2}."));
            }
        }
    }

    if windows.is_empty() {
        if let Some(balance) = account_balance.filter(|value| value.is_finite()) {
            display_value = Some(format!("${balance:.2}"));
            message = Some(match weekly_spend {
                Some(weekly) => format!(
                    "OpenRouter account balance is ${balance:.2}. This API key has no separate spending limit. Weekly spend is ${weekly:.2}."
                ),
                None => format!(
                    "OpenRouter account balance is ${balance:.2}. This API key has no separate spending limit."
                ),
            });
        } else if let Some(weekly) = weekly_spend {
            display_value = Some(format!("${weekly:.2}"));
            message = Some(format!(
                "No OpenRouter spending limit is set on this API key, and the account credit-balance endpoint was unavailable to this key. Weekly spend is ${weekly:.2}."
            ));
        } else {
            message = Some(
                "No OpenRouter spending limit is set on this API key, and the account credit-balance endpoint was unavailable to this key."
                    .into(),
            );
        }
    }

    Ok(ProviderSnapshot {
        id: "openrouter".into(),
        display_name: "OpenRouter".into(),
        glyph: "OR".into(),
        fidelity: "official".into(),
        status: "ok".into(),
        windows,
        headline_id,
        fetched_at: Utc::now(),
        message,
        account,
        manage_url: Some(MANAGE_URL.into()),
        display_value,
        activity: None,
    })
}

fn read_api_key() -> Result<Credential, String> {
    if let Ok(value) = std::env::var("OPENROUTER_API_KEY") {
        let key = value.trim();
        if !key.is_empty() {
            return Ok(Credential {
                key: key.into(),
                source: "OPENROUTER_API_KEY".into(),
            });
        }
    }

    for (base_name, token_name) in [
        ("ANTHROPIC_BASE_URL", "ANTHROPIC_AUTH_TOKEN"),
        ("OPENAI_BASE_URL", "OPENAI_API_KEY"),
    ] {
        let base = std::env::var(base_name).unwrap_or_default();
        if !is_openrouter_url(&base) {
            continue;
        }
        if let Ok(value) = std::env::var(token_name) {
            let key = value.trim();
            if !key.is_empty() {
                return Ok(Credential {
                    key: key.into(),
                    source: format!("{token_name} · OpenRouter"),
                });
            }
        }
    }

    for settings_path in t3_settings_paths() {
        if let Some(credential) = credential_from_t3_settings(&settings_path) {
            return Ok(credential);
        }
    }

    for auth_path in opencode_auth_paths() {
        if let Some(credential) = credential_from_opencode_auth(&auth_path) {
            return Ok(credential);
        }
    }

    Err(
        "OpenRouter API key was not found. Codenotch checks OPENROUTER_API_KEY, T3 Code router instances, and OpenCode's saved OpenRouter connection."
            .into(),
    )
}

fn opencode_auth_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        let xdg = PathBuf::from(xdg);
        if !xdg.as_os_str().is_empty() {
            paths.push(xdg.join("opencode").join("auth.json"));
        }
    }
    if let Some(home) = dirs::home_dir() {
        let default = home
            .join(".local")
            .join("share")
            .join("opencode")
            .join("auth.json");
        if !paths.contains(&default) {
            paths.push(default);
        }
    }
    paths
}

fn credential_from_opencode_auth(path: &Path) -> Option<Credential> {
    let text = fs::read_to_string(path).ok()?;
    let root: Value = serde_json::from_str(&text).ok()?;
    let entry = root.get("openrouter")?.as_object()?;
    if entry
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind != "api")
    {
        return None;
    }
    let key = entry.get("key")?.as_str()?.trim();
    if key.is_empty() {
        return None;
    }
    Some(Credential {
        key: key.into(),
        source: "OpenCode · OpenRouter".into(),
    })
}

fn t3_settings_paths() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(configured) = std::env::var_os("T3CODE_HOME") {
        let configured = PathBuf::from(configured);
        if !configured.as_os_str().is_empty() {
            roots.push(configured);
        }
    }
    if let Some(home) = dirs::home_dir() {
        let default = home.join(".t3");
        if !roots.contains(&default) {
            roots.push(default);
        }
    }

    let mut paths = Vec::new();
    for root in roots {
        paths.push(root.join("userdata").join("settings.json"));
        paths.push(root.join("dev").join("settings.json"));
    }
    paths
}

fn credential_from_t3_settings(settings_path: &Path) -> Option<Credential> {
    let text = fs::read_to_string(settings_path).ok()?;
    let root: Value = serde_json::from_str(&text).ok()?;
    let instances = root.get("providerInstances")?.as_object()?;

    for (instance_id, instance) in instances {
        let Some(environment) = instance.get("environment").and_then(Value::as_array) else {
            continue;
        };
        let display_name = instance
            .get("displayName")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(instance_id);

        if let Some(entry) = environment
            .iter()
            .find(|entry| env_name(entry) == Some("OPENROUTER_API_KEY"))
        {
            if let Some(key) = t3_env_value(settings_path, instance_id, entry) {
                return Some(Credential {
                    key,
                    source: format!("T3 Code · {display_name}"),
                });
            }
        }

        let routes_openrouter = environment.iter().any(|entry| {
            env_name(entry).is_some_and(|name| {
                matches!(name, "ANTHROPIC_BASE_URL" | "OPENAI_BASE_URL")
                    && entry
                        .get("value")
                        .and_then(Value::as_str)
                        .is_some_and(is_openrouter_url)
            })
        });
        if !routes_openrouter {
            continue;
        }

        for token_name in ["ANTHROPIC_AUTH_TOKEN", "OPENAI_API_KEY"] {
            if let Some(entry) = environment
                .iter()
                .find(|entry| env_name(entry) == Some(token_name))
            {
                if let Some(key) = t3_env_value(settings_path, instance_id, entry) {
                    return Some(Credential {
                        key,
                        source: format!("T3 Code · {display_name}"),
                    });
                }
            }
        }
    }
    None
}

fn env_name(entry: &Value) -> Option<&str> {
    entry.get("name").and_then(Value::as_str)
}

fn t3_env_value(settings_path: &Path, instance_id: &str, entry: &Value) -> Option<String> {
    if let Some(value) = entry
        .get("value")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(value.into());
    }

    if entry.get("sensitive").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let name = env_name(entry)?;
    let secret_name = t3_provider_secret_name(instance_id, name);
    let state_dir = settings_path.parent()?;
    let secret_path = state_dir.join("secrets").join(format!("{secret_name}.bin"));
    let bytes = fs::read(secret_path).ok()?;
    let text = std::str::from_utf8(&bytes).ok()?.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn t3_provider_secret_name(instance_id: &str, name: &str) -> String {
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    format!(
        "provider-env-{}-{}",
        engine.encode(instance_id.as_bytes()),
        engine.encode(name.as_bytes())
    )
}

fn is_openrouter_url(value: &str) -> bool {
    value
        .trim()
        .to_ascii_lowercase()
        .contains("openrouter.ai")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_t3_secret_store_name_contract() {
        assert_eq!(
            t3_provider_secret_name("claude_openrouter", "ANTHROPIC_AUTH_TOKEN"),
            "provider-env-Y2xhdWRlX29wZW5yb3V0ZXI-QU5USFJPUElDX0FVVEhfVE9LRU4"
        );
    }

    #[test]
    fn recognizes_openrouter_router_urls() {
        assert!(is_openrouter_url("https://openrouter.ai/api"));
        assert!(is_openrouter_url("HTTPS://OPENROUTER.AI/API/V1"));
        assert!(!is_openrouter_url("https://api.anthropic.com"));
    }

    #[test]
    fn parses_opencode_openrouter_auth() {
        let dir = std::env::temp_dir().join(format!("codenotch-openrouter-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("auth.json");
        fs::write(
            &path,
            r#"{"openrouter":{"type":"api","key":"sk-or-v1-test"}}"#,
        )
        .unwrap();
        let credential = credential_from_opencode_auth(&path).unwrap();
        assert_eq!(credential.key, "sk-or-v1-test");
        assert_eq!(credential.source, "OpenCode · OpenRouter");
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir(dir);
    }

    #[test]
    fn parses_capped_monthly_key_as_remaining_quota() {
        let body = serde_json::json!({
            "data": {
                "label": "sk-or-v1-test...123",
                "is_free_tier": false,
                "limit": 100.0,
                "limit_remaining": 74.5,
                "limit_reset": "monthly",
                "usage": 25.5,
                "usage_weekly": 11.0
            }
        });
        let snapshot = parse_key_usage(&body, "T3 Code · Router", Some(12.34)).unwrap();
        assert_eq!(snapshot.headline_id.as_deref(), Some("monthly"));
        assert_eq!(snapshot.windows.len(), 1);
        assert!((snapshot.windows[0].used_fraction - 0.255).abs() < 0.0001);
        assert!(snapshot.message.as_deref().is_some_and(|message| message.contains("$12.34")));
    }

    #[test]
    fn uncapped_key_prefers_account_balance_over_weekly_spend() {
        let body = serde_json::json!({
            "data": {
                "label": "sk-or-v1-test...123",
                "is_free_tier": false,
                "limit": null,
                "limit_remaining": null,
                "limit_reset": null,
                "usage": 47.56,
                "usage_weekly": 0.0
            }
        });
        let snapshot = parse_key_usage(&body, "OPENROUTER_API_KEY", Some(2.44)).unwrap();
        assert!(snapshot.windows.is_empty());
        assert_eq!(snapshot.display_value.as_deref(), Some("$2.44"));
        assert!(snapshot.message.as_deref().is_some_and(|message| message.contains("account balance is $2.44")));
    }

    #[test]
    fn uncapped_key_falls_back_to_weekly_spend_when_balance_is_unavailable() {
        let body = serde_json::json!({
            "data": {
                "limit": null,
                "limit_remaining": null,
                "usage_weekly": 11.25
            }
        });
        let snapshot = parse_key_usage(&body, "OPENROUTER_API_KEY", None).unwrap();
        assert_eq!(snapshot.display_value.as_deref(), Some("$11.25"));
        assert!(snapshot.message.as_deref().is_some_and(|message| message.contains("balance endpoint was unavailable")));
    }
}
