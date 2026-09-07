use crate::model::{LimitWindow, ProviderAccount, ProviderSnapshot};
use chrono::Utc;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_PROXY_BASE: &str = "https://cli-chat-proxy.grok.com/v1";
const MANAGE_URL: &str = "https://grok.com";

#[derive(Debug, Clone)]
struct GrokCredential {
    token: String,
    user_id: String,
    label: Option<String>,
    source: String,
    proxy_base: String,
}

pub async fn snapshot() -> ProviderSnapshot {
    let credential = match read_credential() {
        Ok(credential) => credential,
        Err(message) => {
            return ProviderSnapshot::unavailable(
                "grok",
                "Grok Build",
                "G",
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
        Err(error) => return unavailable_with_account("error", error.to_string(), &credential),
    };

    let url = format!(
        "{}/billing?format=credits",
        credential.proxy_base.trim_end_matches('/')
    );
    let response = match client
        .get(url)
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", credential.token),
        )
        // These are the same billing request headers used by Grok Build's own
        // x.ai/billing extension. Codenotch only reads the response.
        .header("X-XAI-Token-Auth", "xai-grok-cli")
        .header("x-userid", &credential.user_id)
        .header("x-grok-client-version", env!("CARGO_PKG_VERSION"))
        .header("x-grok-client-mode", "interactive")
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return unavailable_with_account(
                "error",
                format!("Grok Build billing request failed: {error}"),
                &credential,
            )
        }
    };

    if matches!(response.status().as_u16(), 401 | 403) {
        return unavailable_with_account(
            "needsAuth",
            "Grok Build rejected the saved login. Open Grok Build and run `grok login` again.",
            &credential,
        );
    }
    if !response.status().is_success() {
        let status = response.status();
        return unavailable_with_account(
            "error",
            format!("Grok Build billing endpoint returned {status}"),
            &credential,
        );
    }

    let body = match response.json::<Value>().await {
        Ok(body) => body,
        Err(error) => {
            return unavailable_with_account(
                "error",
                format!("Could not decode Grok Build billing data: {error}"),
                &credential,
            )
        }
    };

    match parse_billing(&body, &credential) {
        Ok(snapshot) => snapshot,
        Err(message) => unavailable_with_account("error", message, &credential),
    }
}

fn unavailable_with_account(
    status: &str,
    message: impl Into<String>,
    credential: &GrokCredential,
) -> ProviderSnapshot {
    ProviderSnapshot::unavailable(
        "grok",
        "Grok Build",
        "G",
        status,
        message,
        MANAGE_URL,
        Some(account(credential)),
    )
}

fn account(credential: &GrokCredential) -> ProviderAccount {
    ProviderAccount {
        label: credential.label.clone(),
        plan: Some("Grok Build".into()),
        source: Some(credential.source.clone()),
    }
}

fn parse_billing(root: &Value, credential: &GrokCredential) -> Result<ProviderSnapshot, String> {
    let config = root
        .get("config")
        .and_then(Value::as_object)
        .ok_or_else(|| "Grok Build billing response did not contain a config object.".to_string())?;

    let mut windows = Vec::new();
    let mut headline_id = None;

    if let Some(percent) = config.get("creditUsagePercent").and_then(Value::as_f64) {
        if percent.is_finite() {
            let period = config.get("currentPeriod").and_then(Value::as_object);
            let period_type = period
                .and_then(|period| period.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_uppercase();
            let (id, label) = if period_type.contains("WEEKLY") {
                ("weekly", "Weekly credits")
            } else if period_type.contains("MONTHLY") {
                ("monthly", "Monthly credits")
            } else {
                ("credits", "Credits")
            };
            let resets_at = period
                .and_then(|period| period.get("end"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
            windows.push(LimitWindow {
                id: id.into(),
                label: label.into(),
                used_fraction: (percent / 100.0).clamp(0.0, 1.0),
                resets_at,
            });
            headline_id = Some(id.into());
        }
    }

    // Older Grok Build servers returned monthlyLimit/used instead of the
    // normalized percentage. Preserve compatibility without changing the UI.
    if windows.is_empty() {
        let limit = cent_value(config.get("monthlyLimit"));
        let used = cent_value(config.get("used"));
        if let (Some(limit), Some(used)) = (limit, used) {
            if limit > 0 {
                let resets_at = config
                    .get("billingPeriodEnd")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned);
                windows.push(LimitWindow {
                    id: "monthly".into(),
                    label: "Monthly credits".into(),
                    used_fraction: (used as f64 / limit as f64).clamp(0.0, 1.0),
                    resets_at,
                });
                headline_id = Some("monthly".into());
            }
        }
    }

    if windows.is_empty() {
        return Err(
            "Grok Build billing data did not include a usable credit percentage or legacy credit limit."
                .into(),
        );
    }

    let prepaid = cent_value(config.get("prepaidBalance"));
    let on_demand_used = cent_value(config.get("onDemandUsed"));
    let mut details = Vec::new();
    if let Some(cents) = prepaid {
        if cents > 0 {
            details.push(format!("Prepaid balance ${:.2}", cents as f64 / 100.0));
        }
    }
    if let Some(cents) = on_demand_used {
        if cents > 0 {
            details.push(format!("On-demand used ${:.2}", cents as f64 / 100.0));
        }
    }

    Ok(ProviderSnapshot {
        id: "grok".into(),
        display_name: "Grok Build".into(),
        glyph: "G".into(),
        fidelity: "official".into(),
        status: "ok".into(),
        windows,
        headline_id,
        fetched_at: Utc::now(),
        message: (!details.is_empty()).then(|| details.join(" · ")),
        account: Some(account(credential)),
        manage_url: Some(MANAGE_URL.into()),
        display_value: None,
        activity: None,
    })
}

fn cent_value(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    if let Some(number) = value.as_i64() {
        return Some(number);
    }
    value
        .as_object()
        .and_then(|object| object.get("val"))
        .and_then(Value::as_i64)
        .or(Some(0).filter(|_| value.as_object().is_some()))
}

fn read_credential() -> Result<GrokCredential, String> {
    for (home, source, proxy_override) in grok_home_candidates() {
        let auth_path = home.join("auth.json");
        let text = match fs::read_to_string(&auth_path) {
            Ok(text) => text,
            Err(_) => continue,
        };
        let root: Value = match serde_json::from_str(&text) {
            Ok(root) => root,
            Err(_) => continue,
        };
        let Some(store) = root.as_object() else { continue };

        let mut candidates = store.values().filter_map(auth_candidate).collect::<Vec<_>>();
        candidates.sort_by_key(|candidate| candidate.priority);
        if let Some(candidate) = candidates.into_iter().next() {
            return Ok(GrokCredential {
                token: candidate.token,
                user_id: candidate.user_id,
                label: candidate.label,
                source,
                proxy_base: proxy_override
                    .or_else(|| std::env::var("GROK_CLI_CHAT_PROXY_BASE_URL").ok())
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| DEFAULT_PROXY_BASE.into()),
            });
        }
    }

    Err(
        "Grok Build login was not found. Run `grok login`; Codenotch reads the existing $GROK_HOME/auth.json or ~/.grok/auth.json session."
            .into(),
    )
}

struct AuthCandidate {
    token: String,
    user_id: String,
    label: Option<String>,
    priority: u8,
}

fn auth_candidate(value: &Value) -> Option<AuthCandidate> {
    let object = value.as_object()?;
    let token = object.get("key")?.as_str()?.trim();
    let user_id = object.get("user_id")?.as_str()?.trim();
    if token.is_empty() || user_id.is_empty() {
        return None;
    }
    let mode = object
        .get("auth_mode")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let issuer = object
        .get("oidc_issuer")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();

    // Billing is a grok.com consumer-session feature. Plain xAI API keys do
    // not carry the subscription billing identity, and arbitrary external
    // providers must not be sent to grok.com unless they identify as xAI.
    let priority = match mode.as_str() {
        "oidc" => 0,
        "external" if issuer.contains("auth.x.ai") => 1,
        _ => return None,
    };
    let label = object
        .get("email")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned);

    Some(AuthCandidate {
        token: token.into(),
        user_id: user_id.into(),
        label,
        priority,
    })
}

fn grok_home_candidates() -> Vec<(PathBuf, String, Option<String>)> {
    let mut candidates = Vec::new();
    if let Some(home) = std::env::var_os("GROK_HOME") {
        let path = PathBuf::from(home);
        if !path.as_os_str().is_empty() {
            push_home(&mut candidates, path, "$GROK_HOME".into(), None);
        }
    }

    for settings_path in t3_settings_paths() {
        for (home, name, proxy) in t3_grok_homes(&settings_path) {
            push_home(
                &mut candidates,
                home,
                format!("T3 Code · {name}"),
                proxy,
            );
        }
    }

    if let Some(home) = dirs::home_dir() {
        push_home(
            &mut candidates,
            home.join(".grok"),
            "~/.grok/auth.json".into(),
            None,
        );
    }
    candidates
}

fn push_home(
    candidates: &mut Vec<(PathBuf, String, Option<String>)>,
    path: PathBuf,
    source: String,
    proxy: Option<String>,
) {
    if candidates.iter().any(|(existing, _, _)| existing == &path) {
        return;
    }
    candidates.push((path, source, proxy));
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

fn t3_grok_homes(settings_path: &Path) -> Vec<(PathBuf, String, Option<String>)> {
    let text = match fs::read_to_string(settings_path) {
        Ok(text) => text,
        Err(_) => return Vec::new(),
    };
    let root: Value = match serde_json::from_str(&text) {
        Ok(root) => root,
        Err(_) => return Vec::new(),
    };
    let Some(instances) = root.get("providerInstances").and_then(Value::as_object) else {
        return Vec::new();
    };
    let Some(default_home) = dirs::home_dir().map(|home| home.join(".grok")) else {
        return Vec::new();
    };

    let mut result = Vec::new();
    for (instance_id, instance) in instances {
        if instance.get("driver").and_then(Value::as_str) != Some("grok") {
            continue;
        }
        if instance.get("enabled").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let display_name = instance
            .get("displayName")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(instance_id)
            .to_owned();
        let environment = instance.get("environment").and_then(Value::as_array);
        let value_for = |name: &str| {
            environment.and_then(|environment| {
                environment.iter().find_map(|entry| {
                    (entry.get("name").and_then(Value::as_str) == Some(name))
                        .then(|| entry.get("value").and_then(Value::as_str))
                        .flatten()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToOwned::to_owned)
                })
            })
        };
        let home = value_for("GROK_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| default_home.clone());
        let proxy = value_for("GROK_CLI_CHAT_PROXY_BASE_URL");
        result.push((home, display_name, proxy));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential() -> GrokCredential {
        GrokCredential {
            token: "token".into(),
            user_id: "user".into(),
            label: Some("dev@example.com".into()),
            source: "test".into(),
            proxy_base: DEFAULT_PROXY_BASE.into(),
        }
    }

    #[test]
    fn parses_current_weekly_credit_pool() {
        let root = serde_json::json!({
            "config": {
                "creditUsagePercent": 37.5,
                "currentPeriod": {
                    "type": "USAGE_PERIOD_TYPE_WEEKLY",
                    "start": "2026-09-01T00:00:00Z",
                    "end": "2026-09-08T00:00:00Z"
                },
                "prepaidBalance": {"val": 1200}
            }
        });
        let snapshot = parse_billing(&root, &credential()).unwrap();
        assert_eq!(snapshot.headline_id.as_deref(), Some("weekly"));
        assert_eq!(snapshot.windows.len(), 1);
        assert!((snapshot.windows[0].used_fraction - 0.375).abs() < 0.0001);
        assert_eq!(snapshot.windows[0].resets_at.as_deref(), Some("2026-09-08T00:00:00Z"));
        assert!(snapshot.message.as_deref().is_some_and(|value| value.contains("$12.00")));
    }

    #[test]
    fn falls_back_to_legacy_monthly_credit_shape() {
        let root = serde_json::json!({
            "config": {
                "monthlyLimit": {"val": 10000},
                "used": {"val": 2500},
                "billingPeriodEnd": "2026-10-01T00:00:00Z"
            }
        });
        let snapshot = parse_billing(&root, &credential()).unwrap();
        assert_eq!(snapshot.headline_id.as_deref(), Some("monthly"));
        assert!((snapshot.windows[0].used_fraction - 0.25).abs() < 0.0001);
    }

    #[test]
    fn selects_first_party_oidc_auth_not_plain_api_key() {
        let api_key = serde_json::json!({
            "key": "api-key",
            "auth_mode": "api_key",
            "user_id": "user"
        });
        assert!(auth_candidate(&api_key).is_none());

        let oidc = serde_json::json!({
            "key": "oauth-token",
            "auth_mode": "oidc",
            "user_id": "user",
            "email": "dev@example.com",
            "oidc_issuer": "https://auth.x.ai"
        });
        let candidate = auth_candidate(&oidc).unwrap();
        assert_eq!(candidate.token, "oauth-token");
        assert_eq!(candidate.priority, 0);
    }

    #[test]
    fn xai_external_auth_is_allowed_but_arbitrary_external_auth_is_not() {
        let xai = serde_json::json!({
            "key": "token",
            "auth_mode": "external",
            "user_id": "user",
            "oidc_issuer": "https://auth.x.ai"
        });
        assert!(auth_candidate(&xai).is_some());

        let other = serde_json::json!({
            "key": "token",
            "auth_mode": "external",
            "user_id": "user",
            "oidc_issuer": "https://sso.example.com"
        });
        assert!(auth_candidate(&other).is_none());
    }
}
