use crate::model::{LimitWindow, ProviderAccount, ProviderSnapshot};
use base64::Engine;
use chrono::Utc;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MANAGE_URL: &str = "https://openrouter.ai/settings/keys";
const KEY_ENDPOINT: &str = "https://openrouter.ai/api/v1/key";

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
                Some(account_from_source(&credential.source, None, None)),
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
                Some(account_from_source(&credential.source, None, None)),
            )
        }
    };

    if matches!(response.status().as_u16(), 401 | 403) {
        return ProviderSnapshot::unavailable(
            "openrouter",
            "OpenRouter",
            "OR",
            "needsAuth",
            "OpenRouter rejected the discovered API key. Refresh the key in OpenRouter or the T3 provider instance that owns it.",
            MANAGE_URL,
            Some(account_from_source(&credential.source, None, None)),
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
            Some(account_from_source(&credential.source, None, None)),
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
                Some(account_from_source(&credential.source, None, None)),
            )
        }
    };

    match parse_key_usage(&body, &credential.source) {
        Ok(snapshot) => snapshot,
        Err(message) => ProviderSnapshot::unavailable(
            "openrouter",
            "OpenRouter",
            "OR",
            "error",
            message,
            MANAGE_URL,
            Some(account_from_source(&credential.source, None, None)),
        ),
    }
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

fn parse_key_usage(root: &Value, source: &str) -> Result<ProviderSnapshot, String> {
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
            let used_fraction = ((limit - remaining) / limit).clamp(0.0, 1.0);
            windows.push(LimitWindow {
                id: id.into(),
                label: label.into(),
                used_fraction,
                // OpenRouter exposes the cadence but not the exact next reset
                // timestamp from GET /api/v1/key. Do not invent one.
                resets_at: None,
            });
            headline_id = Some(id.into());
        }
    }

    if windows.is_empty() {
        // Ordinary OpenRouter API keys are often uncapped. The key endpoint can
        // still report spend, but without a denominator there is no honest
        // remaining percentage to draw. Show weekly spend as an absolute value
        // and explain how a key limit enables the gauge instead of fabricating
        // a quota.
        if let Some(weekly) = data.get("usage_weekly").and_then(Value::as_f64) {
            if weekly.is_finite() {
                display_value = Some(format!("${weekly:.2}"));
                message = Some(format!(
                    "No OpenRouter spending limit is set on this API key. Weekly spend is ${weekly:.2}. Set a key limit in OpenRouter to enable a remaining-percent gauge."
                ));
            }
        }
        if message.is_none() {
            message = Some(
                "No OpenRouter spending limit is set on this API key, so there is no honest remaining percentage to display. Set a key limit in OpenRouter to enable the gauge."
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

    // Also support a Codenotch process launched from a router-configured shell.
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

    Err(
        "OpenRouter API key was not found. Codenotch checks OPENROUTER_API_KEY and T3 Code provider instances configured to use openrouter.ai."
            .into(),
    )
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
        // Useful for contributors running T3's desktop/server dev build.
        paths.push(root.join("dev").join("settings.json"));
    }
    paths
}

fn credential_from_t3_settings(settings_path: &Path) -> Option<Credential> {
    let text = fs::read_to_string(settings_path).ok()?;
    let root: Value = serde_json::from_str(&text).ok()?;
    let instances = root.get("providerInstances")?.as_object()?;

    for (instance_id, instance) in instances {
        let environment = instance.get("environment")?.as_array()?;
        let display_name = instance
            .get("displayName")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(instance_id);

        let openrouter_key = environment.iter().find(|entry| {
            env_name(entry).is_some_and(|name| name == "OPENROUTER_API_KEY")
        });
        if let Some(entry) = openrouter_key {
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
                .find(|entry| env_name(entry).is_some_and(|name| name == token_name))
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
    let direct = entry
        .get("value")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(value) = direct {
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
    fn parses_capped_monthly_key_as_remaining_quota() {
        let body = serde_json::json!({
            "data": {
                "label": "sk-or-v1-test...123",
                "is_free_tier": false,
                "limit": 100.0,
                "limit_remaining": 74.5,
                "limit_reset": "monthly",
                "usage": 25.5,
                "usage_daily": 2.0,
                "usage_weekly": 11.0,
                "usage_monthly": 25.5
            }
        });
        let snapshot = parse_key_usage(&body, "T3 Code · Router").unwrap();
        assert_eq!(snapshot.status, "ok");
        assert_eq!(snapshot.headline_id.as_deref(), Some("monthly"));
        assert_eq!(snapshot.windows.len(), 1);
        assert!((snapshot.windows[0].used_fraction - 0.255).abs() < 0.0001);
        assert_eq!(snapshot.account.unwrap().source.as_deref(), Some("T3 Code · Router"));
    }

    #[test]
    fn uncapped_key_uses_absolute_weekly_spend_without_fake_percentage() {
        let body = serde_json::json!({
            "data": {
                "label": "sk-or-v1-test...123",
                "is_free_tier": false,
                "limit": null,
                "limit_remaining": null,
                "limit_reset": null,
                "usage": 25.5,
                "usage_daily": 2.0,
                "usage_weekly": 11.25,
                "usage_monthly": 25.5
            }
        });
        let snapshot = parse_key_usage(&body, "OPENROUTER_API_KEY").unwrap();
        assert!(snapshot.windows.is_empty());
        assert_eq!(snapshot.display_value.as_deref(), Some("$11.25"));
        assert!(snapshot.message.as_deref().is_some_and(|message| message.contains("No OpenRouter spending limit")));
    }
}
