use crate::model::{LimitWindow, ProviderSnapshot};
use base64::Engine;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const OPENROUTER_CREDITS_ENDPOINT: &str = "https://openrouter.ai/api/v1/credits";

#[derive(Debug, Clone)]
struct OpenRouterCredential {
    key: String,
}

#[derive(Debug, Clone, Copy)]
struct CreditSummary {
    total: f64,
    used: f64,
    remaining: f64,
}

pub async fn openrouter_snapshot() -> ProviderSnapshot {
    let mut snapshot = super::openrouter::snapshot().await;

    // A per-key spending limit is already the best gauge when one exists.
    // For an uncapped key, use the account credit pool as the denominator so
    // the ring remains an honest Remaining gauge instead of an empty circle.
    if snapshot.status != "ok" || !snapshot.windows.is_empty() {
        return snapshot;
    }

    let Some(credential) = read_openrouter_credential() else {
        return snapshot;
    };
    let Some(credits) = fetch_credit_summary(&credential).await else {
        return snapshot;
    };
    if credits.total <= 0.0 || !credits.total.is_finite() {
        return snapshot;
    }

    snapshot.windows = vec![LimitWindow {
        id: "account-credits".into(),
        label: format!("Account credits · ${:.2} left", credits.remaining),
        used_fraction: (credits.used / credits.total).clamp(0.0, 1.0),
        resets_at: None,
    }];
    snapshot.headline_id = Some("account-credits".into());
    snapshot.display_value = Some(format!("${:.2}", credits.remaining));

    let weekly = snapshot
        .message
        .as_deref()
        .and_then(extract_weekly_spend)
        .map(|value| format!(" Weekly spend is ${value:.2}."))
        .unwrap_or_default();
    snapshot.message = Some(format!(
        "OpenRouter account credits: ${:.2} remaining out of ${:.2} purchased. This balance does not reset.{}",
        credits.remaining, credits.total, weekly
    ));
    snapshot
}

pub async fn zcode_snapshot() -> ProviderSnapshot {
    let mut snapshot = super::zcode::snapshot().await;
    let Some(message) = snapshot.message.as_deref() else {
        return snapshot;
    };
    let lower = message.to_ascii_lowercase();
    let no_coding_plan = message.contains("当前用户不存在coding plan")
        || message.contains("当前用户不存在 Coding Plan")
        || lower.contains("does not have coding plan")
        || lower.contains("no coding plan");

    if no_coding_plan {
        let source = snapshot
            .account
            .as_ref()
            .and_then(|account| account.source.as_deref())
            .unwrap_or("");
        let api_key_mode = source.contains("ZCode config")
            || source == "Z_AI_API_KEY"
            || source == "ZAI_API_KEY";
        snapshot.status = "needsAuth".into();
        snapshot.message = Some(if api_key_mode {
            "This Z.ai API key belongs to an account with no active GLM Coding Plan. ZCode API-key mode is separate from the account Start Plan/trial. To use the Start Plan, connect the Z.ai account directly in ZCode; otherwise use a Coding Plan API key with the /api/coding/paas/v4 endpoint."
                .into()
        } else {
            "The Z.ai account currently signed into ZCode does not have an active GLM Coding Plan. If this is a new account using the Start Plan/trial, make sure that account is connected directly in ZCode rather than only through API-key mode."
                .into()
        });
    }
    snapshot
}

async fn fetch_credit_summary(credential: &OpenRouterCredential) -> Option<CreditSummary> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .ok()?;
    let response = client
        .get(OPENROUTER_CREDITS_ENDPOINT)
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
    let total = data.get("total_credits")?.as_f64()?;
    let used = data.get("total_usage")?.as_f64()?;
    if !total.is_finite() || !used.is_finite() {
        return None;
    }
    Some(CreditSummary {
        total,
        used,
        remaining: (total - used).max(0.0),
    })
}

fn extract_weekly_spend(message: &str) -> Option<f64> {
    let marker = "Weekly spend is $";
    let tail = message.split(marker).nth(1)?;
    let number = tail
        .chars()
        .take_while(|character| character.is_ascii_digit() || *character == '.')
        .collect::<String>();
    number.parse().ok()
}

fn read_openrouter_credential() -> Option<OpenRouterCredential> {
    if let Ok(value) = std::env::var("OPENROUTER_API_KEY") {
        let key = value.trim();
        if !key.is_empty() {
            return Some(OpenRouterCredential { key: key.into() });
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
                return Some(OpenRouterCredential { key: key.into() });
            }
        }
    }

    for settings_path in t3_settings_paths() {
        if let Some(key) = openrouter_key_from_t3(&settings_path) {
            return Some(OpenRouterCredential { key });
        }
    }

    for path in opencode_auth_paths() {
        if let Some(key) = openrouter_key_from_opencode(&path) {
            return Some(OpenRouterCredential { key });
        }
    }
    None
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
        let path = home
            .join(".local")
            .join("share")
            .join("opencode")
            .join("auth.json");
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

fn openrouter_key_from_opencode(path: &Path) -> Option<String> {
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
    entry
        .get("key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(ToOwned::to_owned)
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

fn openrouter_key_from_t3(settings_path: &Path) -> Option<String> {
    let text = fs::read_to_string(settings_path).ok()?;
    let root: Value = serde_json::from_str(&text).ok()?;
    let instances = root.get("providerInstances")?.as_object()?;

    for (instance_id, instance) in instances {
        let Some(environment) = instance.get("environment").and_then(Value::as_array) else {
            continue;
        };
        if let Some(entry) = environment
            .iter()
            .find(|entry| env_name(entry) == Some("OPENROUTER_API_KEY"))
        {
            if let Some(key) = t3_env_value(settings_path, instance_id, entry) {
                return Some(key);
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
                    return Some(key);
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
    let bytes = fs::read(state_dir.join("secrets").join(format!("{secret_name}.bin"))).ok()?;
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
    fn extracts_weekly_spend_from_existing_message() {
        assert_eq!(
            extract_weekly_spend(
                "OpenRouter account balance is $2.44. This API key has no separate spending limit. Weekly spend is $0.00."
            ),
            Some(0.0)
        );
    }

    #[test]
    fn recognizes_zcode_no_plan_message() {
        let message = "当前用户不存在coding plan";
        assert!(message.contains("当前用户不存在coding plan"));
    }
}
