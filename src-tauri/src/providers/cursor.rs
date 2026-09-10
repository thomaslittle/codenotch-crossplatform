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
        Ok(windows) => {
            // The ring follows Auto (Cursor Models) when reported — never the
            // blended total, never API — and on enterprise/team plans, which
            // report neither, the hard `included` ceiling.
            let headline_id = ["auto", "included", "api"]
                .iter()
                .find(|id| windows.iter().any(|window| window.id == **id))
                .map(|id| id.to_string())
                .unwrap_or_else(|| windows[0].id.clone());
            ProviderSnapshot {
                id: "cursor".into(), display_name: "Cursor".into(), glyph: "⌾".into(), fidelity: "official".into(),
                status: "ok".into(), windows, headline_id: Some(headline_id), fetched_at: Utc::now(), message: None,
                account: account(&path), manage_url: Some(MANAGE_URL.into()), display_value: None, activity: None,
            }
        }
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
    let usage = root.get("individualUsage").unwrap_or(&Value::Null);
    let team = root.get("teamUsage").unwrap_or(&Value::Null);
    let mut windows = Vec::new();
    // Auto (Cursor Models) is the headline bar. Zero is a reading, not an
    // absence — a fresh month is 0% on this bar.
    if let Some(auto) = plan.get("autoPercentUsed").and_then(Value::as_f64) {
        windows.push(LimitWindow { id: "auto".into(), label: "Auto usage".into(), used_fraction: auto / 100.0, resets_at: resets_at.clone() });
    } else if let Some(total) = plan.get("totalPercentUsed").and_then(Value::as_f64) {
        // Older shape without the Auto split: keep the blended total so the
        // ring still reads something honest.
        windows.push(LimitWindow { id: "included".into(), label: "Included usage".into(), used_fraction: total / 100.0, resets_at: resets_at.clone() });
    }
    if let Some(api) = plan.get("apiPercentUsed").and_then(Value::as_f64).filter(|v| *v > 0.0) {
        windows.push(LimitWindow { id: "api".into(), label: "API usage".into(), used_fraction: api / 100.0, resets_at: resets_at.clone() });
    }
    if let Some(on_demand) = spend_window(usage.get("onDemand"), "On demand", resets_at.clone()) {
        windows.push(LimitWindow { id: "on_demand".into(), label: "On demand".into(), used_fraction: on_demand, resets_at });
    }
    // Enterprise / team plans omit `plan` percentages and meter a hard
    // `overall` ceiling instead. Keep the window id as `included` so the
    // headline preference still resolves.
    if windows.is_empty() {
        if let Some(overall) = spend_window(usage.get("overall"), "Included usage", resets_at.clone()) {
            windows.push(LimitWindow { id: "included".into(), label: "Included usage".into(), used_fraction: overall, resets_at });
        }
    }
    if let Some(team_on_demand) = spend_window(team.get("onDemand"), "Team on demand", resets_at.clone()).filter(|fraction| *fraction > 0.0) {
        windows.push(LimitWindow { id: "team_on_demand".into(), label: "Team on demand".into(), used_fraction: team_on_demand, resets_at });
    }
    if windows.is_empty() {
        let membership = root.get("membershipType").and_then(Value::as_str).unwrap_or("this");
        if root.get("isUnlimited").and_then(Value::as_bool) == Some(true) {
            return Err(format!("Unlimited on the {membership} plan — nothing to meter"));
        }
        return Err(format!("The {membership} plan has nothing for Cursor to meter yet"));
    }
    Ok(windows)
}

/// A dollar-denominated bucket stating a real ceiling (`enabled` + limit).
fn spend_window(bucket: Option<&Value>, _label: &str, _resets: Option<DateTime<Utc>>) -> Option<f64> {
    let bucket = bucket?;
    if bucket.get("enabled").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let limit = bucket.get("limit").and_then(Value::as_f64).filter(|value| *value > 0.0)?;
    let used = bucket.get("used").and_then(Value::as_f64)?;
    Some(used / limit)
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

    #[test]
    fn prefers_auto_over_blended_total() {
        let body: Value = serde_json::json!({"billingCycleEnd":"2026-09-24T03:32:15.933Z","individualUsage":{"plan":{"autoPercentUsed":33.0,"apiPercentUsed":19.0,"totalPercentUsed":9.5}}});
        let windows = parse_usage(&body).unwrap();
        assert_eq!(windows[0].id, "auto");
        assert!((windows[0].used_fraction - 0.33).abs() < 0.001);
        assert_eq!(windows[1].id, "api");
    }

    #[test]
    fn parses_enterprise_overall_ceiling() {
        let body: Value = serde_json::json!({"membershipType":"enterprise","limitType":"team","individualUsage":{"overall":{"enabled":true,"used":6907,"limit":45000,"remaining":38093}},"teamUsage":{"onDemand":{"enabled":true,"used":0,"limit":1000000}}});
        let windows = parse_usage(&body).unwrap();
        assert_eq!(windows[0].id, "included");
        assert!((windows[0].used_fraction - 6907.0 / 45000.0).abs() < 0.001);
    }
}
