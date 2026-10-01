use crate::model::{LimitWindow, ProviderAccount, ProviderSnapshot};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::time::Duration;

/// Local Ollama runtime: automatically detected loaded models (`GET /api/ps`)
/// with RAM/VRAM, unload time, context and quantization. No credentials, no
/// inference, no prompt capture — model detection only. Mirrors upstream
/// `OllamaLocalProvider` + `OllamaLocalUsage` (listing shape, loopback-only).
const MANAGE_URL: &str = "https://ollama.com";

pub async fn snapshot() -> ProviderSnapshot {
    let base = endpoint_base();
    let url = format!("{base}/api/ps");
    let client = match reqwest::Client::builder().timeout(Duration::from_secs(4)).build() {
        Ok(client) => client,
        Err(error) => {
            return ProviderSnapshot::unavailable(
                "ollama",
                "Ollama",
                "◉",
                "unsupported",
                error.to_string(),
                MANAGE_URL,
                None,
            )
        }
    };
    let response = match client.get(&url).header("Accept", "application/json").send().await {
        Ok(response) => response,
        Err(_) => {
            return ProviderSnapshot::unavailable(
                "ollama",
                "Ollama",
                "◉",
                "unsupported",
                "Ollama server unavailable. Open Ollama and check the server address.",
                MANAGE_URL,
                None,
            )
        }
    };
    if !response.status().is_success() {
        let status = response.status();
        return ProviderSnapshot::unavailable(
            "ollama",
            "Ollama",
            "◉",
            "unsupported",
            format!("Ollama returned HTTP {status}. Check the server address and configuration."),
            MANAGE_URL,
            None,
        );
    }
    let body = match response.json::<Value>().await {
        Ok(body) => body,
        Err(_) => {
            return ProviderSnapshot::unavailable(
                "ollama",
                "Ollama",
                "◉",
                "unsupported",
                "This server did not return an Ollama model listing.",
                MANAGE_URL,
                None,
            )
        }
    };
    match parse_models(&body) {
        Ok(models) => {
            if models.is_empty() {
                return ProviderSnapshot {
                    id: "ollama".into(),
                    display_name: "Ollama".into(),
                    glyph: "◉".into(),
                    fidelity: "official".into(),
                    status: "ok".into(),
                    windows: Vec::new(),
                    headline_id: None,
                    fetched_at: Utc::now(),
                    message: Some("Ollama is running with no models loaded.".into()),
                    account: Some(ProviderAccount {
                        label: Some(base.clone()),
                        plan: Some("Local".into()),
                        source: Some("Ollama".into()),
                    }),
                    manage_url: Some(MANAGE_URL.into()),
                    display_value: Some("idle".into()),
                    activity: None,
                };
            }
            let detail = models
                .iter()
                .map(|model| {
                    let mut parts = vec![model.name.clone()];
                    if let Some(memory) = model.memory_bytes {
                        parts.push(format_size(memory));
                    }
                    if let Some(quant) = &model.quantization {
                        parts.push(quant.clone());
                    }
                    parts.join(" · ")
                })
                .collect::<Vec<_>>()
                .join("\n");
            // One window per loaded model so stacked/columns gauges show each
            // cell; used_fraction 0 keeps the ring at full (local = available).
            let windows = models
                .iter()
                .map(|model| LimitWindow {
                    id: model.name.clone(),
                    label: model.name.clone(),
                    used_fraction: 0.0,
                    resets_at: model.expires_at,
                })
                .collect::<Vec<_>>();
            ProviderSnapshot {
                id: "ollama".into(),
                display_name: "Ollama".into(),
                glyph: "◉".into(),
                fidelity: "official".into(),
                status: "ok".into(),
                windows,
                headline_id: None,
                fetched_at: Utc::now(),
                message: Some(detail),
                account: Some(ProviderAccount {
                    label: Some(base),
                    plan: Some("Local".into()),
                    source: Some("Ollama".into()),
                }),
                manage_url: Some(MANAGE_URL.into()),
                display_value: Some(format!("{} model{}", models.len(), if models.len() == 1 { "" } else { "s" })),
                activity: None,
            }
        }
        Err(message) => ProviderSnapshot::unavailable("ollama", "Ollama", "◉", "unsupported", message, MANAGE_URL, None),
    }
}

struct Model {
    name: String,
    memory_bytes: Option<i64>,
    quantization: Option<String>,
    expires_at: Option<DateTime<Utc>>,
}

/// `OLLAMA_HOST` override honoured only for loopback hosts; otherwise the
/// default local address. A model listing has no redirect workflow and must
/// never silently move monitoring to another host.
fn endpoint_base() -> String {
    const DEFAULT: &str = "http://127.0.0.1:11434";
    let raw = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| DEFAULT.into());
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return DEFAULT.into();
    }
    let lower = trimmed.to_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return DEFAULT.into();
    }
    let without_scheme = lower.split("://").nth(1).unwrap_or("");
    let host = without_scheme.split('/').next().unwrap_or("").split(':').next().unwrap_or("");
    if !["localhost", "127.0.0.1", "::1", "[::1]"].contains(&host) {
        return DEFAULT.into();
    }
    trimmed.to_owned()
}

fn parse_models(root: &Value) -> Result<Vec<Model>, String> {
    let models = root
        .get("models")
        .and_then(Value::as_array)
        .ok_or("This server did not return an Ollama model listing.")?;
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for model in models {
        let name = model.get("name").and_then(Value::as_str).unwrap_or("").trim();
        if name.is_empty() || !seen.insert(name.to_owned()) {
            if name.is_empty() {
                return Err("This server did not return an Ollama model listing.".into());
            }
            continue;
        }
        if let Some(size) = model.get("size").and_then(Value::as_i64) {
            if size < 0 {
                return Err("This server did not return an Ollama model listing.".into());
            }
        }
        let quantization = model
            .get("details")
            .and_then(|details| details.get("quantization_level"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let expires_at = model
            .get("expires_at")
            .and_then(Value::as_str)
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .map(|date| date.with_timezone(&Utc));
        out.push(Model {
            name: name.to_owned(),
            memory_bytes: model.get("size").and_then(Value::as_i64),
            quantization,
            expires_at,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn format_size(bytes: i64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else {
        format!("{:.0} MB", bytes / MB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_model_listing() {
        let body = json!({"models": [
            {"name": "gemma3:4b", "size": 3220944992i64, "details": {"quantization_level": "Q4_K_M"}, "expires_at": "2026-09-10T12:00:00Z"},
            {"name": "nomic-embed:latest", "size": 274i64}
        ]});
        let models = parse_models(&body).unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "gemma3:4b");
        assert_eq!(models[0].quantization.as_deref(), Some("Q4_K_M"));
        assert!(models[0].expires_at.is_some());
    }

    #[test]
    fn rejects_non_loopback_override() {
        std::env::set_var("OLLAMA_HOST", "http://192.168.1.10:11434");
        assert_eq!(endpoint_base(), "http://127.0.0.1:11434");
        std::env::remove_var("OLLAMA_HOST");
    }
}
