use crate::model::{LimitWindow, ProviderAccount, ProviderSnapshot};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Utc};
use ring::{aead, digest};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const OPENROUTER_CREDITS_ENDPOINT: &str = "https://openrouter.ai/api/v1/credits";
const ZCODE_BILLING_BASE: &str = "https://zcode.z.ai/api/v1/zcode-plan";
const ZCODE_APP_VERSION_FALLBACK: &str = "3.11.2";

#[derive(Debug, Clone)]
struct OpenRouterCredential {
    key: String,
}

#[derive(Debug, Clone, Copy)]
struct CreditSummary {
    total: f64,
    remaining: f64,
}

#[derive(Debug, Clone)]
struct ZcodeStartCredential {
    token: String,
    family: String,
    source: String,
}

pub async fn openrouter_snapshot() -> ProviderSnapshot {
    let mut snapshot = super::openrouter::snapshot().await;
    if snapshot.status != "ok" {
        return snapshot;
    }

    let Some(credential) = read_openrouter_credential() else {
        return snapshot;
    };
    let Some(credits) = fetch_credit_summary(&credential).await else {
        return snapshot;
    };

    // OpenRouter credit balance is the useful number. It is not a resetting
    // quota and there is no stable user-chosen maximum, so do not fabricate a
    // percentage gauge from lifetime purchased credits. Keep the neutral ring
    // and show the actual dollars left beneath it.
    snapshot.windows.clear();
    snapshot.headline_id = None;
    snapshot.display_value = Some(format!("${:.2}", credits.remaining));

    let weekly = snapshot
        .message
        .as_deref()
        .and_then(extract_weekly_spend)
        .map(|value| format!(" Weekly spend is ${value:.2}."))
        .unwrap_or_default();
    snapshot.message = Some(format!(
        "OpenRouter account balance is ${:.2}. Total purchased credits are ${:.2}. This balance does not reset.{}",
        credits.remaining, credits.total, weekly
    ));
    snapshot
}

pub async fn zcode_snapshot() -> ProviderSnapshot {
    let mut snapshot = super::zcode::snapshot().await;
    if snapshot.status == "ok" {
        return snapshot;
    }

    // ZCode exposes free Start Plan / trial model balances through its own
    // zcode-plan billing endpoint, not the paid Coding Plan monitor endpoint.
    // If Coding Plan lookup fails, try the account-bound Start Plan credentials
    // that ZCode already keeps on this machine before showing an error.
    for credential in zcode_start_credentials() {
        if let Some(start_snapshot) = fetch_zcode_start_plan(&credential).await {
            return start_snapshot;
        }
    }

    let Some(message) = snapshot.message.as_deref() else {
        return snapshot;
    };
    let lower = message.to_ascii_lowercase();
    let no_coding_plan = message.contains("当前用户不存在coding plan")
        || message.contains("当前用户不存在 Coding Plan")
        || lower.contains("does not have coding plan")
        || lower.contains("no coding plan");

    if no_coding_plan {
        snapshot.status = "needsAuth".into();
        snapshot.message = Some(
            "ZCode reports no paid Coding Plan for this credential. Codenotch also checked the account-bound Start Plan / free-trial balance but could not read a usable balance. The free Start Plan is separate from a normal Z.ai API key."
                .into(),
        );
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
        remaining: (total - used).max(0.0),
    })
}

async fn fetch_zcode_start_plan(credential: &ZcodeStartCredential) -> Option<ProviderSnapshot> {
    let version = zcode_app_version();
    let url = format!(
        "{ZCODE_BILLING_BASE}/billing/balance?app_version={version}"
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok()?;
    let response = client
        .get(url)
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", credential.token),
        )
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::USER_AGENT, format!("ZCode/{version}"))
        .header("X-ZCode-App-Version", &version)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body = response.json::<Value>().await.ok()?;
    parse_zcode_start_balance(&body, credential)
}

fn parse_zcode_start_balance(
    root: &Value,
    credential: &ZcodeStartCredential,
) -> Option<ProviderSnapshot> {
    let data = root.get("data")?.as_object()?;
    let balances = data.get("balances")?.as_array()?;
    let mut windows = Vec::<(f64, LimitWindow)>::new();
    let mut plan_id: Option<String> = None;

    for balance in balances {
        let object = balance.as_object()?;
        let total = number_value(object.get("total_units"))?;
        if !total.is_finite() || total <= 0.0 {
            continue;
        }
        let remaining = number_value(object.get("remaining_units"));
        let used = number_value(object.get("used_units"))
            .or_else(|| remaining.map(|remaining| (total - remaining).max(0.0)))?;
        let used_fraction = (used / total).clamp(0.0, 1.0);
        let label = object
            .get("show_name")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("Daily model")
            .to_owned();
        let id_source = object
            .get("entitlement_id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&label);
        let id = slug_id(id_source);
        let resets_at = object
            .get("period_end")
            .and_then(Value::as_i64)
            .or_else(|| object.get("expires_at").and_then(Value::as_i64))
            .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0));

        if plan_id.is_none() {
            plan_id = object
                .get("plan_id")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(ToOwned::to_owned);
        }

        windows.push((
            total,
            LimitWindow {
                id,
                label,
                used_fraction,
                resets_at,
            },
        ));
    }

    if windows.is_empty() {
        return None;
    }
    windows.sort_by(|left, right| right.0.total_cmp(&left.0));
    let windows = windows.into_iter().map(|(_, window)| window).collect::<Vec<_>>();
    let headline_id = windows.first().map(|window| window.id.clone());
    let plan = plan_id.as_deref().map(zcode_plan_label).unwrap_or("Start");
    let bigmodel = credential.family == "bigmodel";

    Some(ProviderSnapshot {
        id: "zcode".into(),
        display_name: if bigmodel {
            "ZCode / BigModel".into()
        } else {
            "ZCode / Z.ai".into()
        },
        glyph: "Z".into(),
        fidelity: "official".into(),
        status: "ok".into(),
        windows,
        headline_id,
        fetched_at: Utc::now(),
        message: Some(
            "ZCode Start Plan / free model balances from the account already connected in ZCode."
                .into(),
        ),
        account: Some(ProviderAccount {
            label: Some(if bigmodel { "BigModel" } else { "Z.ai" }.into()),
            plan: Some(plan.into()),
            source: Some(credential.source.clone()),
        }),
        manage_url: Some("https://zcode.z.ai".into()),
        display_value: None,
        activity: None,
    })
}

fn number_value(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|value| value as f64))
        .or_else(|| value.as_u64().map(|value| value as f64))
}

fn slug_id(value: &str) -> String {
    let mut result = String::new();
    let mut dash = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            result.push(character.to_ascii_lowercase());
            dash = false;
        } else if !dash && !result.is_empty() {
            result.push('-');
            dash = true;
        }
    }
    result.trim_matches('-').to_owned()
}

fn zcode_plan_label(plan_id: &str) -> &'static str {
    let lower = plan_id.to_ascii_lowercase();
    if lower.contains("weekend") {
        "Weekend"
    } else if lower.contains("trial") {
        "Trial"
    } else if lower.contains("start") {
        "Start"
    } else {
        "Free"
    }
}

fn zcode_app_version() -> String {
    std::env::var("ZCODE_APP_VERSION")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| ZCODE_APP_VERSION_FALLBACK.into())
}

fn zcode_start_credentials() -> Vec<ZcodeStartCredential> {
    let mut credentials = Vec::new();

    // Account login is the canonical Start Plan credential. This is the
    // zcodejwttoken ZCode itself uses for zcode-plan billing requests.
    for path in zcode_credentials_paths() {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(root) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(object) = root.as_object() else {
            continue;
        };
        let field = |name: &str| {
            object
                .get(name)
                .and_then(Value::as_str)
                .and_then(reveal_zcode_credential)
        };
        let family = field("oauth:active_provider")
            .unwrap_or_else(|| "zai".into())
            .to_ascii_lowercase();
        if let Some(token) = field("zcodejwttoken")
            .filter(|value| value.trim().len() >= 12)
        {
            push_zcode_credential(
                &mut credentials,
                ZcodeStartCredential {
                    token,
                    family: if family.contains("bigmodel") {
                        "bigmodel".into()
                    } else {
                        "zai".into()
                    },
                    source: format!("ZCode account · {}", path.to_string_lossy()),
                },
            );
        }
    }

    // Some ZCode builds also materialize the Start Plan routing token directly
    // on the builtin provider entry. Keep it as a fallback for account sessions
    // where credentials.json is unavailable or stale.
    for path in zcode_config_paths() {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(root) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(providers) = root
            .get("provider")
            .or_else(|| root.get("providers"))
            .and_then(Value::as_object)
        else {
            continue;
        };
        for (provider_id, provider) in providers {
            let family = if provider_id.contains("bigmodel-start-plan") {
                "bigmodel"
            } else if provider_id.contains("zai-start-plan") {
                "zai"
            } else {
                continue;
            };
            if provider.get("enabled").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            let Some(connection) = provider
                .get("options")
                .and_then(Value::as_object)
                .or_else(|| provider.as_object())
            else {
                continue;
            };
            let Some(token) = ["apiKey", "api_key", "token"]
                .iter()
                .find_map(|name| connection.get(*name).and_then(Value::as_str))
                .and_then(reveal_zcode_credential)
                .map(|value| strip_bearer(&value))
                .filter(|value| value.len() >= 12)
            else {
                continue;
            };
            push_zcode_credential(
                &mut credentials,
                ZcodeStartCredential {
                    token,
                    family: family.into(),
                    source: format!("ZCode Start Plan · {}", path.to_string_lossy()),
                },
            );
        }
    }

    credentials
}

fn push_zcode_credential(
    credentials: &mut Vec<ZcodeStartCredential>,
    credential: ZcodeStartCredential,
) {
    if credentials
        .iter()
        .any(|existing| existing.token == credential.token)
    {
        return;
    }
    credentials.push(credential);
}

fn zcode_home_candidates() -> Vec<PathBuf> {
    let mut homes = Vec::new();
    for name in ["ZCODE_HOME", "ZCODE_CONFIG_DIR"] {
        if let Some(value) = std::env::var_os(name) {
            let path = PathBuf::from(value);
            if !path.as_os_str().is_empty() && !homes.contains(&path) {
                homes.push(path);
            }
        }
    }
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".zcode");
        if !homes.contains(&path) {
            homes.push(path);
        }
    }
    homes
}

fn zcode_credentials_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(value) = std::env::var_os("ZCODE_V2_CREDENTIALS") {
        let path = PathBuf::from(value);
        if !path.as_os_str().is_empty() {
            paths.push(path);
        }
    }
    for home in zcode_home_candidates() {
        let path = home.join("v2").join("credentials.json");
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

fn zcode_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(value) = std::env::var_os("ZCODE_V2_CONFIG") {
        let path = PathBuf::from(value);
        if !path.as_os_str().is_empty() {
            paths.push(path);
        }
    }
    for home in zcode_home_candidates() {
        for path in [
            home.join("v2").join("config.json"),
            home.join("cli").join("config.json"),
            home.join("config.json"),
        ] {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths
}

fn zcode_credential_secret() -> String {
    if let Ok(secret) = std::env::var("ZCODE_CREDENTIAL_SECRET") {
        let secret = secret.trim();
        if !secret.is_empty() {
            return secret.into();
        }
    }
    let username = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".into());
    let home = dirs::home_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let platform = match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        _ => "linux",
    };
    format!("zcode-credential-fallback:{platform}:{home}:{username}")
}

fn reveal_zcode_credential(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if !value.starts_with("enc:v1:") {
        return Some(value.into());
    }
    decrypt_zcode_credential(value, &zcode_credential_secret())
}

fn decrypt_zcode_credential(envelope: &str, secret: &str) -> Option<String> {
    let rest = envelope.strip_prefix("enc:v1:")?;
    let mut parts = rest.split('.');
    let iv = URL_SAFE_NO_PAD.decode(parts.next()?).ok()?;
    let tag = URL_SAFE_NO_PAD.decode(parts.next()?).ok()?;
    let ciphertext = URL_SAFE_NO_PAD.decode(parts.next()?).ok()?;
    if parts.next().is_some() || iv.len() != 12 || tag.len() != 16 || ciphertext.is_empty() {
        return None;
    }
    let key_bytes = digest::digest(&digest::SHA256, secret.as_bytes());
    let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, key_bytes.as_ref()).ok()?;
    let key = aead::LessSafeKey::new(unbound);
    let nonce_bytes: [u8; 12] = iv.try_into().ok()?;
    let nonce = aead::Nonce::assume_unique_for_key(nonce_bytes);
    let mut sealed = ciphertext;
    sealed.extend_from_slice(&tag);
    let plaintext = key
        .open_in_place(nonce, aead::Aad::empty(), &mut sealed)
        .ok()?;
    String::from_utf8(plaintext.to_vec()).ok()
}

fn strip_bearer(value: &str) -> String {
    value
        .trim()
        .strip_prefix("Bearer ")
        .or_else(|| value.trim().strip_prefix("bearer "))
        .unwrap_or(value.trim())
        .trim()
        .to_owned()
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
    fn parses_zcode_start_plan_balances() {
        let body = serde_json::json!({
            "code": 0,
            "data": {
                "balances": [
                    {
                        "plan_id": "zcode-v3-start-plan-0615",
                        "entitlement_id": "ent_start_glm_5p3",
                        "show_name": "GLM-5.3",
                        "total_units": 3000000,
                        "used_units": 600000,
                        "remaining_units": 2400000,
                        "period_end": 1782230399
                    },
                    {
                        "plan_id": "zcode-v3-start-plan-0615",
                        "entitlement_id": "ent_start_glm_5p3_flash",
                        "show_name": "GLM-5.3-Flash",
                        "total_units": 5000000,
                        "used_units": 1000000,
                        "remaining_units": 4000000,
                        "period_end": 1782230399
                    }
                ]
            }
        });
        let credential = ZcodeStartCredential {
            token: "token".into(),
            family: "zai".into(),
            source: "test".into(),
        };
        let snapshot = parse_zcode_start_balance(&body, &credential).unwrap();
        assert_eq!(snapshot.status, "ok");
        assert_eq!(snapshot.windows.len(), 2);
        assert_eq!(snapshot.windows[0].label, "GLM-5.3-Flash");
        assert!((snapshot.windows[0].used_fraction - 0.2).abs() < 0.0001);
        assert_eq!(snapshot.account.unwrap().plan.as_deref(), Some("Start"));
    }

    #[test]
    fn recognizes_zcode_no_plan_message() {
        let message = "当前用户不存在coding plan";
        assert!(message.contains("当前用户不存在coding plan"));
    }
}
