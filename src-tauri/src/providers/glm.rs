use crate::model::{LimitWindow, ProviderAccount, ProviderSnapshot};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// GLM Coding Plan usage from Z.ai's own monitor endpoint, with the key
/// borrowed from whichever coding tool already holds one (Claude Code's
/// `settings.json`, ZCode, or OpenCode). Mirrors upstream `GLMCredentials` +
/// `GLMUsage`: session = (hours, 5), weekly = (weeks, 1), MCP = TIME_LIMIT.
pub async fn snapshot() -> ProviderSnapshot {
    const MANAGE_GLOBAL: &str = "https://z.ai/manage-apikey/apikey-list";
    const MANAGE_CN: &str = "https://open.bigmodel.cn/usage";
    let credential = match load() {
        Some(value) => value,
        None => {
            return ProviderSnapshot::unavailable(
                "glm",
                "GLM",
                "△",
                "needsAuth",
                "Usage rides on a Z.ai GLM Coding Plan key held by a coding tool — Claude Code's settings.json, ZCode or OpenCode. Set one up there and the notch reads it.",
                MANAGE_GLOBAL,
                None,
            )
        }
    };
    let manage_url = if credential.base_url.contains("bigmodel.cn") { MANAGE_CN } else { MANAGE_GLOBAL };
    let account = Some(ProviderAccount {
        label: None,
        plan: None,
        source: Some(credential.source.clone()),
    });

    let client = match reqwest::Client::builder().timeout(Duration::from_secs(15)).build() {
        Ok(client) => client,
        Err(error) => {
            return ProviderSnapshot::unavailable("glm", "GLM", "△", "error", error.to_string(), manage_url, account)
        }
    };
    let url = format!("{}/api/monitor/usage/quota/limit", credential.base_url.trim_end_matches('/'));
    let response = match client
        .get(&url)
        .header("Authorization", credential.token.clone())
        .header("Content-Type", "application/json")
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return ProviderSnapshot::unavailable(
                "glm",
                "GLM",
                "△",
                "error",
                format!("GLM usage request failed: {error}"),
                manage_url,
                account,
            )
        }
    };
    if matches!(response.status().as_u16(), 401 | 403) {
        return ProviderSnapshot::unavailable(
            "glm",
            "GLM",
            "△",
            "needsAuth",
            "Z.ai rejected the borrowed plan key. Refresh it in the tool that holds it.",
            manage_url,
            account,
        );
    }
    if response.status().as_u16() == 429 {
        return ProviderSnapshot::unavailable(
            "glm",
            "GLM",
            "△",
            "stale",
            "Z.ai usage is temporarily rate limited. Codenotch is backing off and will retry automatically.",
            manage_url,
            account,
        );
    }
    if !response.status().is_success() {
        let status = response.status();
        return ProviderSnapshot::unavailable(
            "glm",
            "GLM",
            "△",
            "error",
            format!("Z.ai usage endpoint returned {status}"),
            manage_url,
            account,
        );
    }
    match response.json::<Value>().await {
        Ok(body) => match parse_usage(&body) {
            Ok((level, windows)) => ProviderSnapshot {
                id: "glm".into(),
                display_name: "GLM".into(),
                glyph: "△".into(),
                fidelity: "official".into(),
                status: "ok".into(),
                windows,
                headline_id: Some("session".into()),
                fetched_at: Utc::now(),
                message: None,
                account: Some(ProviderAccount {
                    label: None,
                    plan: level,
                    source: Some(credential.source),
                }),
                manage_url: Some(manage_url.into()),
                display_value: None,
                activity: None,
            },
            Err(message) => ProviderSnapshot::unavailable("glm", "GLM", "△", "error", message, manage_url, account),
        },
        Err(error) => ProviderSnapshot::unavailable(
            "glm",
            "GLM",
            "△",
            "error",
            format!("Could not parse GLM usage: {error}"),
            manage_url,
            account,
        ),
    }
}

struct Credential {
    token: String,
    base_url: String,
    source: String,
}

fn load() -> Option<Credential> {
    claude_code()
        .or_else(zcode_plan_key)
        .or_else(zcode_token)
        .or_else(opencode)
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// `~/.claude/settings.json` → `env.ANTHROPIC_AUTH_TOKEN` + base URL.
/// Only claimed when the base URL is a Z.ai host.
fn claude_code() -> Option<Credential> {
    let root = json_file(&home().join(".claude").join("settings.json"))?;
    let env = root.get("env")?.as_object()?;
    let token = env
        .get("ANTHROPIC_AUTH_TOKEN")
        .or_else(|| env.get("ANTHROPIC_API_KEY"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())?;
    let base = env.get("ANTHROPIC_BASE_URL").and_then(Value::as_str)?;
    let host = base.split("://").nth(1).unwrap_or(base).split('/').next().unwrap_or("");
    if !is_zai_host(host) {
        return None;
    }
    Some(Credential {
        token: token.to_owned(),
        base_url: console_base(host),
        source: "Claude Code".into(),
    })
}

/// `~/.zcode/v2/config.json` → enabled `builtin:*-coding-plan` apiKey.
fn zcode_plan_key() -> Option<Credential> {
    let root = json_file(&home().join(".zcode").join("v2").join("config.json"))?;
    let providers = root.get("provider")?.as_object()?;
    let mut ids: Vec<&String> = providers.keys().collect();
    ids.sort();
    for id in ids {
        if !id.contains("coding-plan") {
            continue;
        }
        let provider = providers.get(id)?.as_object()?;
        if provider.get("enabled").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let options = provider.get("options")?.as_object()?;
        let key = options.get("apiKey").and_then(Value::as_str).filter(|value| !value.is_empty())?;
        let console = options
            .get("baseURL")
            .and_then(Value::as_str)
            .and_then(|base| base.split("://").nth(1))
            .and_then(|rest| rest.split('/').next())
            .map(console_base)
            .unwrap_or_else(|| "https://api.z.ai".into());
        return Some(Credential { token: key.to_owned(), base_url: console, source: "ZCode".into() });
    }
    None
}

/// `~/.zcode/v2/credentials.json` → `oauth:zai:access_token` (plaintext only).
fn zcode_token() -> Option<Credential> {
    let root = json_file(&home().join(".zcode").join("v2").join("credentials.json"))?;
    let token = root.get("oauth:zai:access_token").and_then(Value::as_str).filter(|value| !value.is_empty())?;
    if token.starts_with("enc:v1:") {
        return None;
    }
    Some(Credential {
        token: token.to_owned(),
        base_url: "https://api.z.ai".into(),
        source: "ZCode".into(),
    })
}

const OPENCODE_IDS: [&str; 7] = ["zai-coding-plan", "zai", "z-ai", "z.ai", "glm", "zhipu", "zhipuai"];

/// `~/.local/share/opencode/auth.json` under the Z.ai provider names.
fn opencode() -> Option<Credential> {
    let path = home().join(".local").join("share").join("opencode").join("auth.json");
    let root = json_file(&path)?;
    for id in OPENCODE_IDS {
        let entry = root.get(id)?;
        if let Some(token) = entry.as_str().filter(|value| !value.is_empty()) {
            return Some(Credential {
                token: token.to_owned(),
                base_url: console_for_provider(id),
                source: "OpenCode".into(),
            });
        }
        if let Some(object) = entry.as_object() {
            let key = ["apiKey", "api_key", "token", "key", "accessToken", "auth_token"]
                .iter()
                .filter_map(|field| object.get(*field))
                .filter_map(Value::as_str)
                .find(|value| !value.is_empty());
            if let Some(key) = key {
                return Some(Credential {
                    token: key.to_owned(),
                    base_url: console_for_provider(id),
                    source: "OpenCode".into(),
                });
            }
        }
    }
    None
}

fn console_for_provider(id: &str) -> String {
    if id.starts_with("zhipu") { "https://open.bigmodel.cn".into() } else { "https://api.z.ai".into() }
}

fn is_zai_host(host: &str) -> bool {
    host == "api.z.ai" || host.ends_with(".z.ai") || host == "open.bigmodel.cn" || host.ends_with(".bigmodel.cn")
}

fn console_base(host: &str) -> String {
    if host.ends_with("bigmodel.cn") { "https://open.bigmodel.cn".into() } else { "https://api.z.ai".into() }
}

fn json_file(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

fn parse_usage(root: &Value) -> Result<(Option<String>, Vec<LimitWindow>), String> {
    if let Some(code) = root.get("code").and_then(Value::as_i64) {
        let success = root.get("success").and_then(Value::as_bool).unwrap_or(false);
        if !success || code != 200 {
            return match code {
                401 | 403 => Err("Z.ai rejected the borrowed plan key.".into()),
                429 => Err("Z.ai usage is temporarily rate limited.".into()),
                _ => Err(format!("Z.ai usage endpoint returned status {code}")),
            };
        }
    }
    let data = root.get("data").unwrap_or(root);
    let level = data.get("level").and_then(Value::as_str).map(str::to_owned);
    let limits = data.get("limits").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut windows: Vec<LimitWindow> = limits.iter().filter_map(window).collect();
    if windows.is_empty() {
        return Err("Z.ai returned no usage windows.".into());
    }
    windows.sort_by_key(|window| match window.id.as_str() {
        "session" => 0,
        "weekly" => 1,
        "mcp" => 2,
        _ => 3,
    });
    Ok((level, windows))
}

fn window(limit: &Value) -> Option<LimitWindow> {
    let percentage = limit.get("percentage").and_then(Value::as_f64)?;
    let kind = limit.get("type").and_then(Value::as_str).unwrap_or("");
    let unit = limit.get("unit").and_then(Value::as_i64);
    let number = limit.get("number").and_then(Value::as_i64);
    let (id, label) = match kind {
        "TIME_LIMIT" => ("mcp".to_string(), "MCP (1 month)".to_string()),
        _ => match (unit, number) {
            (Some(3), Some(5)) => ("session".to_string(), "Current session".to_string()),
            (Some(6), Some(1)) => ("weekly".to_string(), "Weekly".to_string()),
            (Some(unit), Some(number)) => (
                format!("window-{unit}x{number}"),
                if unit == 3 {
                    format!("Usage ({number} h)")
                } else if unit == 6 {
                    format!("Usage ({number} wk)")
                } else {
                    "Usage".to_string()
                },
            ),
            _ => (kind.to_lowercase(), "Usage".to_string()),
        },
    };
    let resets_at = limit.get("nextResetTime").and_then(Value::as_f64).and_then(|millis| {
        DateTime::from_timestamp_millis(millis as i64).map(|date| date.with_timezone(&Utc))
    });
    Some(LimitWindow { id, label, used_fraction: percentage / 100.0, resets_at })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_session_weekly_mcp() {
        let body = json!({
            "code": 200, "success": true,
            "data": {"level": "pro", "limits": [
                {"type": "TOKENS_LIMIT", "unit": 3, "number": 5, "percentage": 12.5, "nextResetTime": 1788682200000.0},
                {"type": "TOKENS_LIMIT", "unit": 6, "number": 1, "percentage": 8.1, "nextResetTime": 1789190400000.0},
                {"type": "TIME_LIMIT", "percentage": 4.0}
            ]}
        });
        let (level, windows) = parse_usage(&body).unwrap();
        assert_eq!(level.as_deref(), Some("pro"));
        assert_eq!(windows[0].id, "session");
        assert_eq!(windows[1].id, "weekly");
        assert_eq!(windows[2].id, "mcp");
        assert!((windows[0].used_fraction - 0.125).abs() < 0.001);
        assert!(windows[0].resets_at.is_some());
    }

    #[test]
    fn envelope_401_is_auth() {
        let body = json!({"code": 401, "success": false, "msg": "unauthorized"});
        assert!(parse_usage(&body).is_err());
    }
}
