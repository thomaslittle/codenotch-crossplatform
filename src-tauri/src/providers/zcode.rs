use crate::model::{LimitWindow, ProviderAccount, ProviderSnapshot};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Utc};
use reqwest::redirect::Policy;
use ring::{aead, digest};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const ZAI_HOST: &str = "api.z.ai";
const BIGMODEL_API_HOST: &str = "open.bigmodel.cn";
const BIGMODEL_MONITOR_HOST: &str = "bigmodel.cn";
const QUOTA_PATH: &str = "/api/monitor/usage/quota/limit";

#[derive(Debug, Clone)]
struct Credential {
    key: String,
    host: String,
    source: String,
    bearer_first: bool,
}

pub async fn snapshot() -> ProviderSnapshot {
    let credential = match read_credential() {
        Ok(credential) => credential,
        Err(message) => {
            return ProviderSnapshot::unavailable(
                "zcode",
                "ZCode / Z.ai",
                "Z",
                "needsAuth",
                message,
                "https://z.ai/manage-apikey/coding-plan/personal/my-plan",
                None,
            )
        }
    };

    let bigmodel = credential.host == BIGMODEL_API_HOST;
    let display_name = if bigmodel {
        "ZCode / BigModel"
    } else {
        "ZCode / Z.ai"
    };
    let manage_url = if bigmodel {
        "https://bigmodel.cn/coding-plan/personal/usage"
    } else {
        "https://z.ai/manage-apikey/coding-plan/personal/my-plan"
    };
    let account = Some(ProviderAccount {
        label: Some(if bigmodel { "BigModel" } else { "Z.ai" }.into()),
        plan: None,
        source: Some(credential.source.clone()),
    });

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .redirect(Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return ProviderSnapshot::unavailable(
                "zcode",
                display_name,
                "Z",
                "error",
                error.to_string(),
                manage_url,
                account,
            )
        }
    };

    match fetch_quota(&client, &credential).await {
        Ok(body) => match parse_quota(&body, &credential, display_name, manage_url) {
            Ok(snapshot) => snapshot,
            Err(message) => ProviderSnapshot::unavailable(
                "zcode",
                display_name,
                "Z",
                "error",
                message,
                manage_url,
                account,
            ),
        },
        Err((status, message)) => ProviderSnapshot::unavailable(
            "zcode",
            display_name,
            "Z",
            status,
            message,
            manage_url,
            account,
        ),
    }
}

async fn fetch_quota(
    client: &reqwest::Client,
    credential: &Credential,
) -> Result<Value, (&'static str, String)> {
    let monitor_host = if credential.host == BIGMODEL_API_HOST {
        BIGMODEL_MONITOR_HOST
    } else {
        ZAI_HOST
    };
    let url = format!("https://{monitor_host}{QUOTA_PATH}");
    let auth_orders = if credential.bearer_first {
        ["bearer", "raw"]
    } else {
        ["raw", "bearer"]
    };
    let mut last_auth_error: Option<String> = None;

    for auth_style in auth_orders {
        let auth = if auth_style == "bearer" {
            format!("Bearer {}", credential.key)
        } else {
            credential.key.clone()
        };
        let response = client
            .get(&url)
            .header(reqwest::header::AUTHORIZATION, auth)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|error| ("error", format!("ZCode quota request failed: {error}")))?;

        if response.status().is_redirection() {
            return Err((
                "error",
                format!("ZCode quota endpoint redirected with {}", response.status()),
            ));
        }
        if matches!(response.status().as_u16(), 401 | 403) {
            last_auth_error = Some(format!(
                "ZCode quota authentication was rejected using {auth_style} authorization."
            ));
            continue;
        }
        if !response.status().is_success() {
            return Err((
                "error",
                format!("ZCode quota endpoint returned {}", response.status()),
            ));
        }

        let body = response
            .json::<Value>()
            .await
            .map_err(|error| ("error", format!("Could not decode ZCode quota data: {error}")))?;
        if body.get("success").and_then(Value::as_bool) == Some(false)
            || body
                .get("code")
                .and_then(Value::as_i64)
                .is_some_and(|code| code >= 400)
        {
            let message = body
                .get("msg")
                .or_else(|| body.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("ZCode quota service rejected the request")
                .to_owned();
            if looks_like_auth_error(&message) {
                // ZCode has used both raw-token and Bearer authorization for
                // this monitor endpoint. A 200 JSON auth error from the first
                // form must not prevent us trying the other form.
                last_auth_error = Some(message);
                continue;
            }
            return Err(("error", message));
        }
        if body
            .get("data")
            .and_then(|data| data.get("limits"))
            .and_then(Value::as_array)
            .is_some()
        {
            return Ok(body);
        }
        return Err((
            "error",
            "ZCode quota response did not contain usage limits.".into(),
        ));
    }

    Err((
        "needsAuth",
        last_auth_error.unwrap_or_else(|| {
            "Z.ai/BigModel rejected the active ZCode Coding Plan credential.".into()
        }),
    ))
}

fn looks_like_auth_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    [
        "auth",
        "token",
        "login",
        "unauthorized",
        "expired",
        "incorrect",
        "credential",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn parse_quota(
    root: &Value,
    credential: &Credential,
    display_name: &str,
    manage_url: &str,
) -> Result<ProviderSnapshot, String> {
    let data = root
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| "ZCode quota response did not contain a data object.".to_string())?;
    let limits = data
        .get("limits")
        .and_then(Value::as_array)
        .ok_or_else(|| "ZCode quota response did not contain limits.".to_string())?;

    let mut five_hour = None;
    let mut weekly = None;
    let mut mcp = None;
    for limit in limits {
        let Some(object) = limit.as_object() else {
            continue;
        };
        let kind = object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_uppercase();
        let unit = object.get("unit").and_then(Value::as_i64);
        let number = object.get("number").and_then(Value::as_i64);
        let is_model_pool = kind == "TOKENS_LIMIT" || kind == "CREDIT_LIMIT";
        let window = normalized_window(object);

        if is_model_pool && unit == Some(3) && number == Some(5) {
            five_hour = window.map(|(used_fraction, resets_at)| LimitWindow {
                id: "5h".into(),
                label: "5-hour".into(),
                used_fraction,
                resets_at,
            });
        } else if is_model_pool && unit == Some(6) && number == Some(1) {
            weekly = window.map(|(used_fraction, resets_at)| LimitWindow {
                id: "weekly".into(),
                label: "Weekly".into(),
                used_fraction,
                resets_at,
            });
        } else if kind == "TIME_LIMIT" && unit == Some(5) && number == Some(1) {
            mcp = window.map(|(used_fraction, resets_at)| LimitWindow {
                id: "mcp-monthly".into(),
                label: "Monthly MCP".into(),
                used_fraction,
                resets_at,
            });
        }
    }

    let mut windows = Vec::new();
    if let Some(window) = five_hour {
        windows.push(window);
    }
    if let Some(window) = weekly {
        windows.push(window);
    }
    if let Some(window) = mcp {
        windows.push(window);
    }
    if windows.is_empty() {
        return Err(
            "ZCode returned quota rows, but none matched a supported 5-hour, weekly, or monthly MCP window."
                .into(),
        );
    }

    let plan = ["planName", "plan", "plan_type", "packageName", "level"]
        .iter()
        .find_map(|key| data.get(*key).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned);
    let headline_id = if windows.iter().any(|window| window.id == "5h") {
        Some("5h".into())
    } else if windows.iter().any(|window| window.id == "weekly") {
        Some("weekly".into())
    } else {
        windows.first().map(|window| window.id.clone())
    };

    Ok(ProviderSnapshot {
        id: "zcode".into(),
        display_name: display_name.into(),
        glyph: "Z".into(),
        fidelity: "official".into(),
        status: "ok".into(),
        windows,
        headline_id,
        fetched_at: Utc::now(),
        message: None,
        account: Some(ProviderAccount {
            label: Some(if credential.host == BIGMODEL_API_HOST {
                "BigModel"
            } else {
                "Z.ai"
            }
            .into()),
            plan,
            source: Some(credential.source.clone()),
        }),
        manage_url: Some(manage_url.into()),
        display_value: None,
        activity: None,
    })
}

fn normalized_window(object: &Map<String, Value>) -> Option<(f64, Option<DateTime<Utc>>)> {
    let used_fraction = object
        .get("percentage")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .map(|value| (value / 100.0).clamp(0.0, 1.0))
        .or_else(|| {
            let total = object.get("usage").and_then(Value::as_f64)?;
            let used = object.get("currentValue").and_then(Value::as_f64)?;
            (total > 0.0 && total.is_finite() && used.is_finite())
                .then(|| (used / total).clamp(0.0, 1.0))
        })?;
    let resets_at = object
        .get("nextResetTime")
        .and_then(Value::as_i64)
        .and_then(DateTime::<Utc>::from_timestamp_millis);
    Some((used_fraction, resets_at))
}

fn read_credential() -> Result<Credential, String> {
    for env_name in ["Z_AI_API_KEY", "ZAI_API_KEY"] {
        if let Ok(value) = std::env::var(env_name) {
            let host_override = std::env::var("Z_AI_API_HOST")
                .ok()
                .or_else(|| std::env::var("ZAI_API_HOST").ok());
            if let Some((host, key)) = parse_env_key(&value, host_override) {
                return Ok(Credential {
                    key,
                    host,
                    source: env_name.into(),
                    bearer_first: false,
                });
            }
        }
    }

    // ZCode's current provider configuration is authoritative. In API-key
    // mode an old credentials.json from a previous account can remain on disk;
    // preferring it caused exactly the misleading "token expired" state when
    // the active provider was a newly entered API key.
    for path in zcode_config_paths() {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) => continue,
        };
        let root: Value = match serde_json::from_str(&text) {
            Ok(root) => root,
            Err(_) => continue,
        };
        if let Some((host, key)) = find_coding_plan_key(&root) {
            return Ok(Credential {
                key,
                host,
                source: format!("ZCode config · {}", path.to_string_lossy()),
                bearer_first: false,
            });
        }
        if has_enabled_general_api_key(&root) {
            return Err(
                "ZCode is connected with a general Z.ai/BigModel API key, not a Coding Plan key. General `/api/paas/v4` balance is separate from ZCode Coding Plan quota. Use the Coding Plan endpoint `/api/coding/paas/v4` if this key belongs to a Coding Plan."
                    .into(),
            );
        }
    }

    // Account-bound login is a fallback when ZCode is not actively configured
    // for API-key mode. Values are decrypted locally with ZCode's own
    // same-device credential scheme; nothing is copied or persisted by us.
    for path in zcode_credentials_paths() {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) => continue,
        };
        if let Some(credential) = credential_from_credentials_file(&text, &path) {
            return Ok(credential);
        }
    }

    Err(
        "ZCode Coding Plan login or API key was not found. Codenotch reads the active Coding Plan API-key provider first, then the existing same-device ZCode login."
            .into(),
    )
}

fn parse_env_key(value: &str, host_override: Option<String>) -> Option<(String, String)> {
    let key = strip_bearer(value);
    if key.is_empty() {
        return None;
    }
    let host = host_override
        .as_deref()
        .and_then(canonical_host)
        .unwrap_or_else(|| ZAI_HOST.into());
    Some((host, key))
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
        let default = home.join(".zcode");
        if !homes.contains(&default) {
            homes.push(default);
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
    if let Some(value) = std::env::var_os("ZCODE_V2_CONFIG") {
        let config = PathBuf::from(value);
        if let Some(parent) = config.parent() {
            let path = parent.join("credentials.json");
            if !paths.contains(&path) {
                paths.push(path);
            }
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

fn credential_from_credentials_file(text: &str, path: &Path) -> Option<Credential> {
    let root: Value = serde_json::from_str(text).ok()?;
    let object = root.as_object()?;
    let field = |name: &str| {
        object
            .get(name)
            .and_then(Value::as_str)
            .and_then(reveal_credential)
    };

    let active_provider = field("oauth:active_provider")
        .unwrap_or_else(|| "zai".into())
        .to_ascii_lowercase();
    let host = if active_provider.contains("bigmodel") {
        BIGMODEL_API_HOST.into()
    } else {
        ZAI_HOST.into()
    };

    // The monitor endpoint primarily consumes the Z.ai OAuth access token.
    // zcodejwttoken is retained as a fallback for older/start-plan sessions.
    let key = ["oauth:zai:access_token", "zcodejwttoken"]
        .iter()
        .find_map(|name| field(name))
        .filter(|value| value.trim().len() >= 12)?;

    Some(Credential {
        key,
        host,
        source: format!("ZCode login · {}", path.to_string_lossy()),
        bearer_first: true,
    })
}

fn credential_secret() -> String {
    if let Ok(secret) = std::env::var("ZCODE_CREDENTIAL_SECRET") {
        let trimmed = secret.trim();
        if !trimmed.is_empty() {
            return trimmed.into();
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

fn reveal_credential(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("enc:v1:") {
        decrypt_credential_with_secret(trimmed, &credential_secret())
    } else {
        Some(trimmed.into())
    }
}

fn decrypt_credential_with_secret(envelope: &str, secret: &str) -> Option<String> {
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

fn find_coding_plan_key(root: &Value) -> Option<(String, String)> {
    for section in ["provider", "providers"] {
        if let Some(entries) = root.get(section).and_then(Value::as_object) {
            let mut values = entries.values().collect::<Vec<_>>();
            values.sort_by_key(|entry| entry.get("enabled").and_then(Value::as_bool) != Some(true));
            if let Some(found) = values
                .into_iter()
                .filter_map(Value::as_object)
                .find_map(coding_plan_key_from_object)
            {
                return Some(found);
            }
        }
    }

    fn visit(value: &Value) -> Option<(String, String)> {
        match value {
            Value::Object(object) => {
                coding_plan_key_from_object(object).or_else(|| object.values().find_map(visit))
            }
            Value::Array(values) => values.iter().find_map(visit),
            _ => None,
        }
    }
    visit(root)
}

fn coding_plan_key_from_object(object: &Map<String, Value>) -> Option<(String, String)> {
    let connection = object
        .get("options")
        .and_then(Value::as_object)
        .unwrap_or(object);
    let base_url = connection
        .get("baseURL")
        .or_else(|| connection.get("baseUrl"))
        .or_else(|| connection.get("base_url"))
        .and_then(Value::as_str)?;
    if !is_coding_plan_url(base_url) {
        return None;
    }
    let key = api_key_from_connection(connection)?;
    let host = canonical_host(base_url)?;
    Some((host, key))
}

fn has_enabled_general_api_key(root: &Value) -> bool {
    for section in ["provider", "providers"] {
        let Some(entries) = root.get(section).and_then(Value::as_object) else {
            continue;
        };
        for entry in entries.values() {
            let Some(object) = entry.as_object() else {
                continue;
            };
            if object.get("enabled").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            let connection = object
                .get("options")
                .and_then(Value::as_object)
                .unwrap_or(object);
            let Some(base_url) = connection
                .get("baseURL")
                .or_else(|| connection.get("baseUrl"))
                .or_else(|| connection.get("base_url"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let lower = base_url.to_ascii_lowercase();
            let canonical = lower.contains(ZAI_HOST) || lower.contains(BIGMODEL_API_HOST);
            let general = canonical && lower.contains("/api/paas/v4") && !lower.contains("/coding/");
            if general && api_key_from_connection(connection).is_some() {
                return true;
            }
        }
    }
    false
}

fn api_key_from_connection(connection: &Map<String, Value>) -> Option<String> {
    let key = ["apiKey", "api_key", "access_token", "token"]
        .iter()
        .find_map(|name| connection.get(*name).and_then(Value::as_str))
        .and_then(reveal_credential)
        .map(|value| strip_bearer(&value))?;
    (!key.is_empty()).then_some(key)
}

fn is_coding_plan_url(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    (lower.contains(ZAI_HOST) || lower.contains(BIGMODEL_API_HOST))
        && (lower.contains("/coding/") || lower.contains("/anthropic"))
}

fn canonical_host(value: &str) -> Option<String> {
    let lower = value.trim().to_ascii_lowercase();
    if lower.contains(ZAI_HOST) {
        Some(ZAI_HOST.into())
    } else if lower.contains(BIGMODEL_API_HOST) || lower.contains(BIGMODEL_MONITOR_HOST) {
        Some(BIGMODEL_API_HOST.into())
    } else {
        None
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_credential() -> Credential {
        Credential {
            key: "key".into(),
            host: ZAI_HOST.into(),
            source: "test".into(),
            bearer_first: true,
        }
    }

    #[test]
    fn parses_legacy_three_window_shape() {
        let root = serde_json::json!({
            "code": 200,
            "success": true,
            "data": {
                "level": "pro",
                "limits": [
                    {"type":"TIME_LIMIT","unit":5,"number":1,"usage":1000,"currentValue":82,"remaining":918,"percentage":8,"nextResetTime":1781661646979},
                    {"type":"TOKENS_LIMIT","unit":3,"number":5,"percentage":37,"nextResetTime":1780602733798},
                    {"type":"TOKENS_LIMIT","unit":6,"number":1,"percentage":25,"nextResetTime":1780970446997}
                ]
            }
        });
        let snapshot = parse_quota(&root, &test_credential(), "ZCode / Z.ai", "https://z.ai").unwrap();
        assert_eq!(snapshot.windows.len(), 3);
        assert_eq!(snapshot.headline_id.as_deref(), Some("5h"));
        assert!((snapshot.windows[0].used_fraction - 0.37).abs() < 0.0001);
    }

    #[test]
    fn parses_new_credit_limit_shape() {
        let root = serde_json::json!({
            "code": 200,
            "success": true,
            "data": {
                "level": "lite",
                "limits": [
                    {"type":"CREDIT_LIMIT","unit":3,"number":5,"usage":2000,"currentValue":1653,"remaining":346,"percentage":82,"nextResetTime":1787176502893},
                    {"type":"CREDIT_LIMIT","unit":6,"number":1,"usage":10000,"currentValue":4562,"remaining":5437,"percentage":45,"nextResetTime":1787607163997}
                ]
            }
        });
        let snapshot = parse_quota(&root, &test_credential(), "ZCode / Z.ai", "https://z.ai").unwrap();
        assert_eq!(snapshot.windows.len(), 2);
        assert!((snapshot.windows[0].used_fraction - 0.82).abs() < 0.0001);
        assert!((snapshot.windows[1].used_fraction - 0.45).abs() < 0.0001);
    }

    #[test]
    fn discovers_coding_plan_key_and_distinguishes_general_api_key() {
        let coding = serde_json::json!({
            "provider": {
                "zai": {"enabled": true, "options": {"apiKey":"Bearer zai-key", "baseURL":"https://api.z.ai/api/coding/paas/v4"}}
            }
        });
        assert_eq!(find_coding_plan_key(&coding), Some((ZAI_HOST.into(), "zai-key".into())));
        assert!(!has_enabled_general_api_key(&coding));

        let general = serde_json::json!({
            "provider": {
                "zai": {"enabled": true, "options": {"apiKey":"general-key", "baseURL":"https://api.z.ai/api/paas/v4"}}
            }
        });
        assert!(find_coding_plan_key(&general).is_none());
        assert!(has_enabled_general_api_key(&general));
    }

    #[test]
    fn login_prefers_oauth_access_token_over_legacy_zcode_jwt() {
        let root = serde_json::json!({
            "oauth:active_provider": "zai",
            "oauth:zai:access_token": "fresh-oauth-access-token",
            "zcodejwttoken": "old-zcode-jwt-token"
        });
        let credential = credential_from_credentials_file(&root.to_string(), Path::new("credentials.json")).unwrap();
        assert_eq!(credential.key, "fresh-oauth-access-token");
        assert!(credential.bearer_first);
    }

    #[test]
    fn decrypts_zcode_v1_credential_envelope() {
        let secret = "unit-test-secret";
        let plaintext = b"test-zcode-jwt-token";
        let key_bytes = digest::digest(&digest::SHA256, secret.as_bytes());
        let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, key_bytes.as_ref()).unwrap();
        let key = aead::LessSafeKey::new(unbound);
        let nonce_bytes = [7u8; 12];
        let nonce = aead::Nonce::assume_unique_for_key(nonce_bytes);
        let mut sealed = plaintext.to_vec();
        key.seal_in_place_append_tag(nonce, aead::Aad::empty(), &mut sealed)
            .unwrap();
        let (ciphertext, tag) = sealed.split_at(plaintext.len());
        let envelope = format!(
            "enc:v1:{}.{}.{}",
            URL_SAFE_NO_PAD.encode(nonce_bytes),
            URL_SAFE_NO_PAD.encode(tag),
            URL_SAFE_NO_PAD.encode(ciphertext)
        );
        assert_eq!(
            decrypt_credential_with_secret(&envelope, secret).as_deref(),
            Some("test-zcode-jwt-token")
        );
    }

    #[test]
    fn auth_errors_are_classified_for_retry() {
        assert!(looks_like_auth_error("token expired or incorrect"));
        assert!(looks_like_auth_error("Unauthorized"));
        assert!(!looks_like_auth_error("quota service unavailable"));
    }
}
