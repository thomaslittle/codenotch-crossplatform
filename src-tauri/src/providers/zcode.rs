use crate::model::{LimitWindow, ProviderAccount, ProviderSnapshot};
use chrono::{DateTime, Utc};
use reqwest::redirect::Policy;
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const ZAI_HOST: &str = "api.z.ai";
const BIGMODEL_HOST: &str = "open.bigmodel.cn";

#[derive(Debug, Clone)]
struct Credential {
    key: String,
    host: String,
    source: String,
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

    let display_name = if credential.host == BIGMODEL_HOST {
        "ZCode / BigModel"
    } else {
        "ZCode / Z.ai"
    };
    let manage_url = if credential.host == BIGMODEL_HOST {
        "https://bigmodel.cn/coding-plan/personal/usage"
    } else {
        "https://z.ai/manage-apikey/coding-plan/personal/my-plan"
    };
    let account = Some(ProviderAccount {
        label: Some(if credential.host == BIGMODEL_HOST { "BigModel" } else { "Z.ai" }.into()),
        plan: None,
        source: Some(credential.source.clone()),
    });

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        // Never follow a redirect while carrying a provider API key.
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
    // The official Coding Plan UI/plugin has used quota/limit; newer plan/API
    // deployments have also returned the same limits under /api/monitor/usage.
    // Try both official hosts/paths but never send credentials anywhere else.
    let paths = ["/api/monitor/usage/quota/limit", "/api/monitor/usage"];
    let auth_orders: [&str; 2] = if credential.host == BIGMODEL_HOST {
        ["raw", "bearer"]
    } else {
        ["bearer", "raw"]
    };
    let mut last_error = None;

    for path in paths {
        let url = format!("https://{}{}", credential.host, path);
        for auth_style in auth_orders {
            let auth = if auth_style == "bearer" {
                format!("Bearer {}", credential.key)
            } else {
                credential.key.clone()
            };
            let response = match client
                .get(&url)
                .header(reqwest::header::AUTHORIZATION, auth)
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    last_error = Some(format!("ZCode quota request failed: {error}"));
                    continue;
                }
            };

            if response.status().is_redirection() {
                last_error = Some(format!("ZCode quota endpoint redirected with {}", response.status()));
                continue;
            }
            if matches!(response.status().as_u16(), 401 | 403) {
                last_error = Some("Z.ai/BigModel rejected the Coding Plan API key.".into());
                continue;
            }
            if !response.status().is_success() {
                last_error = Some(format!("ZCode quota endpoint returned {}", response.status()));
                continue;
            }
            let body = response
                .json::<Value>()
                .await
                .map_err(|error| ("error", format!("Could not decode ZCode quota data: {error}")))?;
            if body.get("success").and_then(Value::as_bool) == Some(false)
                || body.get("code").and_then(Value::as_i64).is_some_and(|code| code >= 400)
            {
                last_error = Some(
                    body.get("msg")
                        .and_then(Value::as_str)
                        .unwrap_or("ZCode quota service rejected the request")
                        .to_owned(),
                );
                continue;
            }
            if body
                .get("data")
                .and_then(|data| data.get("limits"))
                .and_then(Value::as_array)
                .is_some()
            {
                return Ok(body);
            }
            last_error = Some("ZCode quota response did not contain usage limits.".into());
        }
    }

    let message = last_error.unwrap_or_else(|| "ZCode quota could not be read.".into());
    let status = if message.contains("rejected") || message.contains("API key") {
        "needsAuth"
    } else {
        "error"
    };
    Err((status, message))
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
        let Some(object) = limit.as_object() else { continue };
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
    if let Some(window) = five_hour { windows.push(window); }
    if let Some(window) = weekly { windows.push(window); }
    if let Some(window) = mcp { windows.push(window); }
    if windows.is_empty() {
        return Err("ZCode returned quota rows, but none matched a supported 5-hour, weekly, or monthly MCP window.".into());
    }

    let plan = ["planName", "plan", "plan_type", "packageName", "level"]
        .iter()
        .find_map(|key| data.get(*key).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.to_owned());
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
            label: Some(if credential.host == BIGMODEL_HOST { "BigModel" } else { "Z.ai" }.into()),
            plan,
            source: Some(credential.source.clone()),
        }),
        manage_url: Some(manage_url.into()),
        display_value: None,
        activity: None,
    })
}

fn normalized_window(object: &Map<String, Value>) -> Option<(f64, Option<String>)> {
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
        .and_then(DateTime::<Utc>::from_timestamp_millis)
        .map(|date| date.to_rfc3339());
    Some((used_fraction, resets_at))
}

fn read_credential() -> Result<Credential, String> {
    if let Ok(value) = std::env::var("Z_AI_API_KEY") {
        if let Some((host, key)) = parse_env_key(&value, std::env::var("Z_AI_API_HOST").ok()) {
            return Ok(Credential {
                key,
                host,
                source: "Z_AI_API_KEY".into(),
            });
        }
    }

    for path in zcode_config_paths() {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) => continue,
        };
        let root: Value = match serde_json::from_str(&text) {
            Ok(root) => root,
            Err(_) => continue,
        };
        if let Some((host, key)) = find_provider_key(&root) {
            return Ok(Credential {
                key,
                host,
                source: path.to_string_lossy().into_owned(),
            });
        }
    }

    Err(
        "ZCode Coding Plan API key was not found. Codenotch checks Z_AI_API_KEY and ~/.zcode/v2/config.json. Account-bound ZCode login credentials are device-encrypted; API Key mode is supported without copying the key into Codenotch."
            .into(),
    )
}

fn parse_env_key(value: &str, host_override: Option<String>) -> Option<(String, String)> {
    let key = strip_bearer(value);
    if key.is_empty() { return None; }
    let host = host_override
        .as_deref()
        .and_then(canonical_host)
        .unwrap_or_else(|| ZAI_HOST.into());
    Some((host, key))
}

fn zcode_config_paths() -> Vec<PathBuf> {
    if let Ok(value) = std::env::var("ZCODE_V2_CONFIG") {
        let path = PathBuf::from(value);
        if !path.as_os_str().is_empty() {
            return vec![path];
        }
    }
    dirs::home_dir()
        .map(|home| vec![home.join(".zcode").join("v2").join("config.json")])
        .unwrap_or_default()
}

fn find_provider_key(root: &Value) -> Option<(String, String)> {
    fn visit(value: &Value) -> Option<(String, String)> {
        match value {
            Value::Object(object) => {
                if let Some(found) = key_from_object(object) {
                    return Some(found);
                }
                object.values().find_map(visit)
            }
            Value::Array(values) => values.iter().find_map(visit),
            _ => None,
        }
    }
    visit(root)
}

fn key_from_object(object: &Map<String, Value>) -> Option<(String, String)> {
    // ZCode provider objects put connection fields in `options`; tolerate a
    // direct object too so config migrations do not break discovery.
    let connection = object
        .get("options")
        .and_then(Value::as_object)
        .unwrap_or(object);
    let key = connection.get("apiKey").and_then(Value::as_str).map(strip_bearer)?;
    if key.is_empty() { return None; }
    let base_url = connection
        .get("baseURL")
        .or_else(|| connection.get("baseUrl"))
        .and_then(Value::as_str)?;
    let host = canonical_host(base_url)?;
    Some((host, key))
}

fn canonical_host(value: &str) -> Option<String> {
    let lower = value.trim().to_ascii_lowercase();
    if lower.contains(ZAI_HOST) {
        Some(ZAI_HOST.into())
    } else if lower.contains(BIGMODEL_HOST) {
        Some(BIGMODEL_HOST.into())
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
        let credential = Credential { key: "key".into(), host: ZAI_HOST.into(), source: "test".into() };
        let snapshot = parse_quota(&root, &credential, "ZCode / Z.ai", "https://z.ai").unwrap();
        assert_eq!(snapshot.windows.len(), 3);
        assert_eq!(snapshot.headline_id.as_deref(), Some("5h"));
        assert_eq!(snapshot.windows[0].label, "5-hour");
        assert!((snapshot.windows[0].used_fraction - 0.37).abs() < 0.0001);
        assert_eq!(snapshot.windows[1].label, "Weekly");
        assert_eq!(snapshot.windows[2].label, "Monthly MCP");
        assert_eq!(snapshot.account.unwrap().plan.as_deref(), Some("pro"));
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
        let credential = Credential { key: "key".into(), host: ZAI_HOST.into(), source: "test".into() };
        let snapshot = parse_quota(&root, &credential, "ZCode / Z.ai", "https://z.ai").unwrap();
        assert_eq!(snapshot.windows.len(), 2);
        assert!((snapshot.windows[0].used_fraction - 0.82).abs() < 0.0001);
        assert!((snapshot.windows[1].used_fraction - 0.45).abs() < 0.0001);
    }

    #[test]
    fn discovers_only_canonical_zai_or_bigmodel_provider_objects() {
        let root = serde_json::json!({
            "providers": {
                "unrelated": {"options": {"apiKey":"do-not-use", "baseURL":"https://example.com/v1"}},
                "zai": {"options": {"apiKey":"Bearer zai-key", "baseURL":"https://api.z.ai/api/coding/paas/v4"}}
            }
        });
        assert_eq!(find_provider_key(&root), Some((ZAI_HOST.into(), "zai-key".into())));

        let bigmodel = serde_json::json!({
            "options": {"apiKey":"cn-key", "baseURL":"https://open.bigmodel.cn/api/coding/paas/v4"}
        });
        assert_eq!(find_provider_key(&bigmodel), Some((BIGMODEL_HOST.into(), "cn-key".into())));
    }

    #[test]
    fn derives_percentage_when_provider_omits_percentage_field() {
        let object = serde_json::json!({"usage":1000.0,"currentValue":125.0,"nextResetTime":1781661646979});
        let normalized = normalized_window(object.as_object().unwrap()).unwrap();
        assert!((normalized.0 - 0.125).abs() < 0.0001);
        assert!(normalized.1.is_some());
    }
}
