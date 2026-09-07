use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Once;

static ONCE: Once = Once::new();
const KEYWORDS: [&str; 9] = [
    "start-plan",
    "startplan",
    "trial",
    "daily",
    "quota",
    "balance",
    "billing",
    "plan",
    "usage",
];

pub fn emit_if_enabled() {
    if std::env::var("CODENOTCH_ZCODE_DEBUG").ok().as_deref() != Some("1") {
        return;
    }
    ONCE.call_once(run);
}

fn run() {
    eprintln!("[zcode-debug] redacted diagnostics enabled; no API keys, tokens, cookies, account ids, or credential values are printed");
    let homes = home_candidates();
    if homes.is_empty() {
        eprintln!("[zcode-debug] no ZCode home candidate could be resolved");
        return;
    }

    for home in homes {
        eprintln!("[zcode-debug] home={} exists={}", home.display(), home.exists());
        if !home.exists() {
            continue;
        }

        let v2 = home.join("v2");
        inspect_plan_cache(&v2.join("coding-plan-cache.json"));
        inspect_config(&v2.join("config.json"));
        inspect_credentials(&v2.join("credentials.json"));
        inspect_settings(&v2.join("setting.json"));
    }
}

fn inspect_plan_cache(path: &Path) {
    let Some(root) = read_json(path, "coding-plan-cache.json") else {
        return;
    };
    let Some(items) = root
        .get("entryStatus")
        .and_then(|value| value.get("items"))
        .and_then(Value::as_object)
    else {
        eprintln!("[zcode-debug] cache has no entryStatus.items object");
        return;
    };

    for (id, entry) in items {
        let status = entry.get("status").and_then(Value::as_str).unwrap_or("<missing>");
        let reason = entry.get("reason").and_then(Value::as_str).unwrap_or("<missing>");
        eprintln!("[zcode-debug] cache provider={} status={} reason={}", id, status, reason);
    }
}

fn inspect_config(path: &Path) {
    let Some(root) = read_json(path, "config.json") else {
        return;
    };
    let Some(providers) = root
        .get("provider")
        .or_else(|| root.get("providers"))
        .and_then(Value::as_object)
    else {
        eprintln!("[zcode-debug] config has no provider/providers object");
        return;
    };

    eprintln!("[zcode-debug] config provider_count={}", providers.len());
    for (id, provider) in providers {
        let enabled = provider
            .get("enabled")
            .and_then(Value::as_bool)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "<unset>".into());
        let kind = provider
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("<unset>");
        let options = provider.get("options").and_then(Value::as_object);
        let base_url = options
            .and_then(|object| {
                object
                    .get("baseURL")
                    .or_else(|| object.get("baseUrl"))
                    .or_else(|| object.get("base_url"))
            })
            .and_then(Value::as_str)
            .map(redact_url)
            .unwrap_or_else(|| "<none>".into());
        let has_api_key = options
            .is_some_and(|object| ["apiKey", "api_key", "access_token", "token"]
                .iter()
                .any(|name| object.get(*name).is_some()));
        let model_count = provider
            .get("models")
            .and_then(Value::as_object)
            .map(|models| models.len())
            .unwrap_or(0);
        let zcode_keys = provider
            .get("zcode")
            .and_then(Value::as_object)
            .map(|object| {
                object
                    .keys()
                    .filter(|key| !sensitive_key(key))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();

        eprintln!(
            "[zcode-debug] provider id={} enabled={} kind={} baseURL={} has_api_key={} model_count={} zcode_keys=[{}]",
            id,
            enabled,
            kind,
            base_url,
            has_api_key,
            model_count,
            zcode_keys,
        );
    }
}

fn inspect_credentials(path: &Path) {
    let Some(root) = read_json(path, "credentials.json") else {
        return;
    };
    let Some(object) = root.as_object() else {
        return;
    };

    let active_provider_present = object.contains_key("oauth:active_provider");
    let zai_user_info_present = object.contains_key("oauth:zai:user_info");
    let zai_access_token_present = object.contains_key("oauth:zai:access_token");
    let legacy_jwt_present = object.contains_key("zcodejwttoken");
    eprintln!(
        "[zcode-debug] credentials active_provider={} zai_user_info={} zai_access_token={} legacy_zcode_jwt={} (presence only)",
        active_provider_present,
        zai_user_info_present,
        zai_access_token_present,
        legacy_jwt_present,
    );
}

fn inspect_settings(path: &Path) {
    let Some(root) = read_json(path, "setting.json") else {
        return;
    };
    let enabled_cli = root
        .get("enabledBuiltinAgentCliProviders")
        .map(value_shape)
        .unwrap_or("<missing>");
    let family_modes = root
        .get("modelProviderFamilyModes")
        .map(value_shape)
        .unwrap_or("<missing>");
    eprintln!(
        "[zcode-debug] settings enabledBuiltinAgentCliProviders={} modelProviderFamilyModes={}",
        enabled_cli,
        family_modes,
    );
}

fn read_json(path: &Path, label: &str) -> Option<Value> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => {
            eprintln!("[zcode-debug] file={} exists=false", label);
            return None;
        }
    };
    let hits = KEYWORDS
        .iter()
        .copied()
        .filter(|needle| text.to_ascii_lowercase().contains(needle))
        .collect::<Vec<_>>();
    let root = match serde_json::from_str::<Value>(&text) {
        Ok(root) => root,
        Err(_) => {
            eprintln!("[zcode-debug] file={} bytes={} parse=false", label, text.len());
            return None;
        }
    };
    eprintln!(
        "[zcode-debug] file={} bytes={} keywords=[{}] root_keys=[{}]",
        label,
        text.len(),
        hits.join(","),
        root_key_summary(&root).join(","),
    );
    Some(root)
}

fn redact_url(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        // Base URLs are provider configuration, not credentials. Drop query and
        // fragment anyway so a malformed/custom URL cannot leak parameters.
        trimmed
            .split(['?', '#'])
            .next()
            .unwrap_or(trimmed)
            .chars()
            .take(180)
            .collect()
    } else {
        "<non-http>".into()
    }
}

fn value_shape(value: &Value) -> &'static str {
    match value {
        Value::Object(_) => "object",
        Value::Array(_) => "array",
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "bool",
        Value::Null => "null",
    }
}

fn root_key_summary(value: &Value) -> Vec<String> {
    let Some(object) = value.as_object() else {
        return Vec::new();
    };
    let mut keys = object
        .keys()
        .filter(|key| !sensitive_key(key))
        .take(24)
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    keys
}

fn sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    [
        "key",
        "token",
        "secret",
        "password",
        "credential",
        "cookie",
        "authorization",
        "email",
        "userid",
        "user_id",
        "accountid",
        "account_id",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn home_candidates() -> Vec<PathBuf> {
    let mut homes = Vec::new();
    for name in ["ZCODE_HOME", "ZCODE_CONFIG_DIR"] {
        if let Some(value) = std::env::var_os(name) {
            push_unique(&mut homes, PathBuf::from(value));
        }
    }
    if let Some(config) = std::env::var_os("ZCODE_V2_CONFIG") {
        if let Some(parent) = Path::new(&config).parent().and_then(Path::parent) {
            push_unique(&mut homes, parent.to_path_buf());
        }
    }
    if let Some(home) = dirs::home_dir() {
        push_unique(&mut homes, home.join(".zcode"));
    }
    homes
}

fn push_unique(values: &mut Vec<PathBuf>, value: PathBuf) {
    if !value.as_os_str().is_empty() && !values.contains(&value) {
        values.push(value);
    }
}
