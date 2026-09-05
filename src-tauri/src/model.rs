use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitWindow {
    pub id: String,
    pub label: String,
    pub used_fraction: f64,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAccount {
    pub label: Option<String>,
    pub plan: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySummary {
    pub state: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSnapshot {
    pub id: String,
    pub display_name: String,
    pub glyph: String,
    pub fidelity: String,
    pub status: String,
    pub windows: Vec<LimitWindow>,
    pub headline_id: Option<String>,
    pub fetched_at: DateTime<Utc>,
    pub message: Option<String>,
    pub account: Option<ProviderAccount>,
    pub manage_url: Option<String>,
    pub display_value: Option<String>,
    pub activity: Option<ActivitySummary>,
}

impl ProviderSnapshot {
    pub fn unavailable(
        id: &str,
        display_name: &str,
        glyph: &str,
        status: &str,
        message: impl Into<String>,
        manage_url: &str,
        account: Option<ProviderAccount>,
    ) -> Self {
        Self {
            id: id.into(),
            display_name: display_name.into(),
            glyph: glyph.into(),
            fidelity: "official".into(),
            status: status.into(),
            windows: Vec::new(),
            headline_id: None,
            fetched_at: Utc::now(),
            message: Some(message.into()),
            account,
            manage_url: Some(manage_url.into()),
            display_value: None,
            activity: None,
        }
    }
}
