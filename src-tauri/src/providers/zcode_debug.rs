use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Once;
use walkdir::WalkDir;

static ONCE: Once = Once::new();
const MAX_FILE_BYTES: u64 = 1_000_000;
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
    eprintln!("[zcode-debug] redacted diagnostics enabled; no API keys, tokens, cookies, or credential values are printed");
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

        for entry in WalkDir::new(&home)
            .max_depth(5)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
        {
            let path = entry.path();
            let relative = path.strip_prefix(&home).unwrap_or(path);
            let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            let lower_name = relative.to_string_lossy().to_ascii_lowercase();
            let interesting_name = lower_name.ends_with(".json")
                || lower_name.contains("config")
                || lower_name.contains("setting")
                || lower_name.contains("provider")
                || lower_name.contains("plan")
                || lower_name.contains("usage")
                || lower_name.contains("quota")
                || lower_name.contains("billing")
                || lower_name.contains("balance")
                || lower_name.contains("cache");
            if !interesting_name || size > MAX_FILE_BYTES {
                continue;
            }

            let Ok(text) = fs::read_to_string(path) else {
                continue;
            };
            let lower = text.to_ascii_lowercase();
            let hits = KEYWORDS
                .iter()
                .copied()
                .filter(|needle| lower.contains(needle))
                .collect::<Vec<_>>();
            let parsed = serde_json::from_str::<Value>(&text).ok();
            let root_keys = parsed
                .as_ref()
                .map(root_key_summary)
                .unwrap_or_default();

            if hits.is_empty() && root_keys.is_empty() {
                continue;
            }
            eprintln!(
                "[zcode-debug] file={} bytes={} keywords=[{}] root_keys=[{}]",
                relative.display(),
                size,
                hits.join(","),
                root_keys.join(","),
            );

            if is_target_plan_file(relative) {
                if let Some(value) = parsed.as_ref() {
                    let mut summaries = Vec::new();
                    collect_safe_plan_summary(value, "", 0, &mut summaries);
                    for summary in summaries.into_iter().take(80) {
                        eprintln!("[zcode-debug] detail file={} {}", relative.display(), summary);
                    }
                }
            }
        }
    }
}

fn is_target_plan_file(path: &Path) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    normalized == "v2/coding-plan-cache.json" || normalized == "v2/config.json"
}

fn collect_safe_plan_summary(value: &Value, prefix: &str, depth: usize, output: &mut Vec<String>) {
    if depth > 8 || output.len() >= 80 {
        return;
    }
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if sensitive_key(key) {
                    continue;
                }
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                match child {
                    Value::Object(_) | Value::Array(_) => {
                        output.push(format!("{path}=<{}>", value_kind(child)));
                        collect_safe_plan_summary(child, &path, depth + 1, output);
                    }
                    _ => {
                        if let Some(rendered) = safe_scalar(&path, child) {
                            output.push(format!("{path}={rendered}"));
                        }
                    }
                }
                if output.len() >= 80 {
                    break;
                }
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().take(12).enumerate() {
                let path = format!("{prefix}[{index}]");
                match child {
                    Value::Object(_) | Value::Array(_) => {
                        output.push(format!("{path}=<{}>", value_kind(child)));
                        collect_safe_plan_summary(child, &path, depth + 1, output);
                    }
                    _ => {
                        if let Some(rendered) = safe_scalar(&path, child) {
                            output.push(format!("{path}={rendered}"));
                        }
                    }
                }
                if output.len() >= 80 {
                    break;
                }
            }
        }
        _ => {}
    }
}

fn safe_scalar(path: &str, value: &Value) -> Option<String> {
    let lower_path = path.to_ascii_lowercase();
    let safe_path = [
        "plan",
        "status",
        "tier",
        "level",
        "type",
        "enabled",
        "reset",
        "limit",
        "usage",
        "remaining",
        "percentage",
        "balance",
        "trial",
        "daily",
        "version",
        "baseurl",
        "base_url",
    ]
    .iter()
    .any(|needle| lower_path.contains(needle));

    match value {
        Value::Bool(value) if safe_path => Some(value.to_string()),
        Value::Number(value) if safe_path => Some(value.to_string()),
        Value::String(value) => {
            let lower = value.to_ascii_lowercase();
            let keyword_value = KEYWORDS.iter().any(|needle| lower.contains(needle));
            if !safe_path && !keyword_value {
                return None;
            }
            let cleaned = value.replace(['\r', '\n', '\t'], " ");
            let truncated = cleaned.chars().take(120).collect::<String>();
            Some(format!("{:?}", truncated))
        }
        Value::Null if safe_path => Some("null".into()),
        _ => None,
    }
}

fn value_kind(value: &Value) -> &'static str {
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
