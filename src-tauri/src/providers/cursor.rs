use crate::model::{LimitWindow, ProviderAccount, ProviderSnapshot};
use chrono::{DateTime, Utc};
use reqwest::header::{ACCEPT, COOKIE};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::PathBuf;

const MANAGE_URL: &str = "https://cursor.com/dashboard";
const ENDPOINT: &str = "https://cursor.com/api/usage-summary";

pub async fn snapshot() -> ProviderSnapshot {
    let path = store_path();
    let credentials = match credentials(&path) {
        Ok(value) => value,
        Err(message) => return ProviderSnapshot::unavailable(
            "cursor", "Cursor", "⌾", "needsAuth", message, MANAGE_URL, account(&path),
        ),
    };
    let client = match reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build() {
        Ok(client) => client,
        Err(error) => return ProviderSnapshot::unavailable("cursor", "Cursor", "⌾", "error", error.to_string(), MANAGE_URL, account(&path)),
    };
    let cookie = format!("WorkosCursorSessionToken={}::{}", credentials.0, credentials.1);
    let response = match client.get(ENDPOINT).header(ACCEPT, "application/json").header(COOKIE, cookie).send().await {
        Ok(response) => response,
        Err(error) => return ProviderSnapshot::unavailable("cursor", "Cursor", "⌾", "error", format!("Cursor usage request failed: {error}"), MANAGE_URL, account(&path)),
    };
    if matches!(response.status().as_u16(), 401 | 403) {
        return ProviderSnapshot::unavailable("cursor", "Cursor", "⌾", "needsAuth", "Cursor rejected the editor session. Open Cursor and sign in again.", MANAGE_URL, account(&path));
    }
    if !response.status().is_success() {
        let status = response.status();
        return ProviderSnapshot::unavailable("cursor", "Cursor", "⌾", "error", format!("Cursor usage endpoint returned {status}"), MANAGE_URL, account(&path));
    }
    let body = match response.json::<Value>().await {
        Ok(body) => body,
        Err(error) => return ProviderSnapshot::unavailable("cursor", "Cursor", "⌾", "error", format!("Could not decode Cursor usage: {error}"), MANAGE_URL, account(&path)),
    };
    match parse_usage(&body) {
        Ok(windows) => ProviderSnapshot {
            id: "cursor".into(), display_name: "Cursor".into(), glyph: "⌾".into(), fidelity: "official".into(),
            status: "ok".into(), windows, headline_id: Some("included".into()), fetched_at: Utc::now(), message: None,
            account: account(&path), manage_url: Some(MANAGE_URL.into()), display_value: None, activity: None,
        },
        Err(error) => ProviderSnapshot::unavailable("cursor", "Cursor", "⌾", "error", format!("Could not parse Cursor usage: {error}"), MANAGE_URL, account(&path)),
    }
}

fn store_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("Cursor/User/globalStorage/state.vscdb")
    }
    #[cfg(not(target_os = "windows"))]
    {
        dirs::config_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config")).join("Cursor/User/globalStorage/state.vscdb")
    }
}

fn open_store(path: &PathBuf) -> Result<Connection, String> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI)
        .map_err(|error| format!("Cursor state database is unavailable at {}: {error}", path.display()))
}

fn item(connection: &Connection, key: &str) -> Option<String> {
    connection.query_row("SELECT value FROM ItemTable WHERE key = ?1", [key], |row| row.get(0)).ok()
}

fn credentials(path: &PathBuf) -> Result<(String, String), String> {
    let connection = open_store(path)?;
    let access = item(&connection, "cursorAuth/accessToken").filter(|v| !v.is_empty()).ok_or("Cursor access token is missing. Open Cursor and sign in.")?;
    let account = item(&connection, "cursorAuth/stripeMembershipAuthId").filter(|v| !v.is_empty()).ok_or("Cursor account id is missing. Open Cursor and sign in.")?;
    Ok((account, access))
}

fn account(path: &PathBuf) -> Option<ProviderAccount> {
    let connection = open_store(path).ok()?;
    Some(ProviderAccount {
        label: item(&connection, "cursorAuth/cachedEmail"),
        plan: item(&connection, "cursorAuth/stripeMembershipType"),
        source: Some("Cursor".into()),
    })
}

fn parse_usage(root: &Value) -> Result<Vec<LimitWindow>, String> {
    let resets_at = root.get("billingCycleEnd").and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok()).map(|date| date.with_timezone(&Utc));
    let plan = root.pointer("/individualUsage/plan").unwrap_or(&Value::Null);
    let mut windows = Vec::new();
    if let Some(total) = plan.get("totalPercentUsed").and_then(Value::as_f64) {
        windows.push(LimitWindow { id: "included".into(), label: "Included usage".into(), used_fraction: total / 100.0, resets_at: resets_at.clone() });
    }
    if let Some(api) = plan.get("apiPercentUsed").and_then(Value::as_f64).filter(|v| *v > 0.0) {
        windows.push(LimitWindow { id: "api".into(), label: "API usage".into(), used_fraction: api / 100.0, resets_at: resets_at.clone() });
    }
    if let Some(on_demand) = root.pointer("/individualUsage/onDemand") {
        if on_demand.get("enabled").and_then(Value::as_bool) == Some(true) {
            if let (Some(used), Some(limit)) = (on_demand.get("used").and_then(Value::as_f64), on_demand.get("limit").and_then(Value::as_f64).filter(|v| *v > 0.0)) {
                windows.push(LimitWindow { id: "on_demand".into(), label: "On demand".into(), used_fraction: used / limit, resets_at });
            }
        }
    }
    if windows.is_empty() { Err("Cursor returned no metered usage windows.".into()) } else { Ok(windows) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_included_usage() {
        let body: Value = serde_json::json!({"billingCycleEnd":"2026-09-24T03:32:15.933Z","individualUsage":{"plan":{"totalPercentUsed":21.0,"apiPercentUsed":0.0}}});
        let windows = parse_usage(&body).unwrap();
        assert_eq!(windows[0].id, "included");
        assert!((windows[0].used_fraction - 0.21).abs() < 0.001);
    }
}
