use crate::model::{ActivitySummary, LimitWindow, ProviderAccount, ProviderSnapshot};
use base64::Engine;
use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

const MANAGE_URL: &str = "https://chatgpt.com/#settings/Account";

pub async fn snapshot() -> ProviderSnapshot {
    let root = codex_home();
    let account = account(&root.join("auth.json"));
    let Some(file) = newest_rollout(&root.join("sessions")) else {
        return ProviderSnapshot::unavailable(
            "codex", "Codex", "✦", "needsAuth",
            "No Codex rollout log found yet. Run Codex once so it can record a rate-limit snapshot.",
            MANAGE_URL, account,
        );
    };

    match fs::read_to_string(&file) {
        Ok(text) => match parse_rollout(&text) {
            Ok((windows, recorded_at)) => {
                let modified = fs::metadata(&file).and_then(|m| m.modified()).ok();
                let activity = modified.and_then(activity_from_modified);
                ProviderSnapshot {
                    id: "codex".into(), display_name: "Codex".into(), glyph: "✦".into(),
                    fidelity: "official".into(), status: "ok".into(), windows,
                    headline_id: Some("primary".into()), fetched_at: recorded_at.unwrap_or_else(Utc::now),
                    message: None, account, manage_url: Some(MANAGE_URL.into()), display_value: None, activity,
                }
            }
            Err(message) => ProviderSnapshot::unavailable(
                "codex", "Codex", "✦", "error", message, MANAGE_URL, account,
            ),
        },
        Err(error) => ProviderSnapshot::unavailable(
            "codex", "Codex", "✦", "error", format!("Could not read Codex rollout: {error}"), MANAGE_URL, account,
        ),
    }
}

fn codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME").map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

fn newest_rollout(root: &Path) -> Option<PathBuf> {
    WalkDir::new(root).into_iter().filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("rollout-") && entry.path().extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.into_path()))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

fn parse_rollout(text: &str) -> Result<(Vec<LimitWindow>, Option<DateTime<Utc>>), String> {
    for line in text.lines().rev().filter(|line| line.contains("rate_limits")) {
        let Ok(root) = serde_json::from_str::<Value>(line) else { continue };
        let limits = root.get("rate_limits").or_else(|| root.get("payload")?.get("rate_limits"));
        let Some(limits) = limits else { continue };
        let now = Utc::now();
        let mut windows = Vec::new();
        for (id, fallback) in [("primary", "Current session"), ("secondary", "Longer window")] {
            let Some(bucket) = limits.get(id) else { continue };
            let Some(percent) = bucket.get("used_percent").and_then(Value::as_f64) else { continue };
            let minutes = bucket.get("window_minutes").and_then(Value::as_f64);
            let resets_at = bucket.get("resets_at").and_then(Value::as_f64)
                .and_then(|epoch| Utc.timestamp_opt(epoch as i64, 0).single())
                .or_else(|| bucket.get("resets_in_seconds").and_then(Value::as_f64).map(|seconds| now + chrono::Duration::seconds(seconds as i64)));
            windows.push(LimitWindow {
                id: id.into(), label: window_label(minutes, fallback), used_fraction: percent / 100.0, resets_at,
            });
        }
        if windows.is_empty() { continue; }
        let recorded_at = root.get("timestamp").and_then(Value::as_str)
            .and_then(|stamp| DateTime::parse_from_rfc3339(stamp).ok()).map(|stamp| stamp.with_timezone(&Utc));
        return Ok((windows, recorded_at));
    }
    Err("Codex has not recorded a usage snapshot in its latest rollout yet.".into())
}

fn window_label(minutes: Option<f64>, fallback: &str) -> String {
    let Some(minutes) = minutes.filter(|v| *v > 0.0) else { return fallback.into(); };
    if minutes < 60.0 { return format!("{}m limit", minutes as i64); }
    if minutes < 1440.0 { return format!("{}h limit", (minutes / 60.0) as i64); }
    let days = (minutes / 1440.0).round() as i64;
    match days { 7 => "Weekly limit".into(), 30 => "Monthly limit".into(), _ => format!("{days}d limit") }
}

fn account(path: &Path) -> Option<ProviderAccount> {
    let root: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    let token = root.get("tokens")?.get("id_token")?.as_str()?;
    let payload = token.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).ok()?;
    let claims: Value = serde_json::from_slice(&decoded).ok()?;
    Some(ProviderAccount {
        label: claims.get("email").and_then(Value::as_str).map(str::to_owned),
        plan: claims.pointer("/https:~1~1api.openai.com~1auth/chatgpt_plan_type").and_then(Value::as_str).map(str::to_owned),
        source: Some("Codex".into()),
    })
}

fn activity_from_modified(modified: SystemTime) -> Option<ActivitySummary> {
    let age = SystemTime::now().duration_since(modified).ok()?.as_secs();
    (age <= 8).then(|| ActivitySummary { state: "working".into(), label: Some("Working now".into()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_latest_rate_limit_line() {
        let input = r#"{"timestamp":"2026-09-05T12:00:00Z","type":"event_msg","payload":{"rate_limits":{"primary":{"used_percent":52.0,"window_minutes":300,"resets_at":1790000000},"secondary":{"used_percent":13.0,"window_minutes":10080,"resets_at":1790500000}}}}"#;
        let (windows, _) = parse_rollout(input).unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].label, "5h limit");
        assert!((windows[0].used_fraction - 0.52).abs() < 0.001);
    }
}
