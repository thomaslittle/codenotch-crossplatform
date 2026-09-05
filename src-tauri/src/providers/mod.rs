mod antigravity;
mod claude;
mod codex;
mod cursor;

use crate::model::ProviderSnapshot;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

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
        let (claude, cursor, codex, antigravity) = tokio::join!(
            claude::snapshot(),
            cursor::snapshot(),
            codex::snapshot(),
            antigravity::snapshot(),
        );
        vec![claude, cursor, codex, antigravity]
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
