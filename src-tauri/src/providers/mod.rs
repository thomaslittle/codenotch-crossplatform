mod antigravity;
mod claude;
mod codex;
mod cursor;
mod opencode;

use crate::model::{ActivitySummary, ProviderSnapshot};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;
use walkdir::WalkDir;

pub struct ProviderStore {
    last_good: Mutex<HashMap<String, ProviderSnapshot>>,
    cache_path: PathBuf,
}

impl Default for ProviderStore {
    fn default() -> Self {
        let cache_path = dirs::cache_dir()
            .or_else(dirs::data_local_dir)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("codenotch-crossplatform")
            .join("snapshots.json");
        let last_good = fs::read_to_string(&cache_path)
            .ok()
            .and_then(|text| serde_json::from_str::<HashMap<String, ProviderSnapshot>>(&text).ok())
            .unwrap_or_default();
        Self { last_good: Mutex::new(last_good), cache_path }
    }
}

impl ProviderStore {
    pub async fn snapshots(&self) -> Vec<ProviderSnapshot> {
        let (claude, cursor, codex, antigravity, opencode) = tokio::join!(
            claude::snapshot(),
            cursor::snapshot(),
            codex::snapshot(),
            antigravity::snapshot(),
            opencode::snapshot(),
        );
        vec![claude, cursor, codex, antigravity, opencode]
            .into_iter()
            .map(|snapshot| self.with_stale_fallback(snapshot))
            .collect()
    }

    fn with_stale_fallback(&self, snapshot: ProviderSnapshot) -> ProviderSnapshot {
        let mut cache = self.last_good.lock().expect("provider cache poisoned");
        if snapshot.status == "ok" {
            cache.insert(snapshot.id.clone(), snapshot.clone());
            self.persist(&cache);
            return snapshot;
        }
        if let Some(previous) = cache.get(&snapshot.id) {
            let mut stale = previous.clone();
            stale.status = "stale".into();
            stale.message = snapshot.message;
            // Activity is ephemeral local state, not part of the durable usage
            // reading. Never carry a cached `working` spinner forward just
            // because the live quota refresh failed. Claude's local activity
            // remains available even when its usage endpoint is unavailable.
            stale.activity = if stale.id == "claude" {
                claude_activity()
            } else {
                snapshot.activity
            };
            return stale;
        }
        snapshot
    }

    fn persist(&self, cache: &HashMap<String, ProviderSnapshot>) {
        let Some(parent) = self.cache_path.parent() else { return };
        if fs::create_dir_all(parent).is_err() { return; }
        let Ok(text) = serde_json::to_string(cache) else { return };
        let _ = fs::write(&self.cache_path, text);
    }
}

fn claude_config_dir() -> PathBuf {
    std::env::var_os("CLAUDE_SECURESTORAGE_CONFIG_DIR").map(PathBuf::from)
        .or_else(|| std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from))
        .or_else(|| dirs::home_dir().map(|home| home.join(".claude")))
        .unwrap_or_else(|| PathBuf::from(".claude"))
}

/// Lightweight local activity signal used to update the spinner and trigger a
/// usage refresh at the beginning/end of a Claude turn without polling the
/// Anthropic usage endpoint every few seconds.
pub(crate) fn claude_activity() -> Option<ActivitySummary> {
    let projects = claude_config_dir().join("projects");
    let newest = WalkDir::new(projects).max_depth(5).into_iter().filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file() && entry.path().extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|entry| entry.metadata().ok()?.modified().ok()).max()?;
    let age = SystemTime::now().duration_since(newest).ok()?.as_secs();
    (age <= 8).then(|| ActivitySummary { state: "working".into(), label: Some("Working now".into()) })
}
