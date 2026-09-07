mod antigravity;
mod claude;
mod codex;
mod cursor;
mod grok;
mod opencode;
mod openrouter;
mod zcode;

use crate::model::{ActivitySummary, ProviderSnapshot};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

const CLAUDE_MIN_REFRESH: Duration = Duration::from_secs(180);
const CLAUDE_RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(600);

struct ClaudeRefreshGate {
    next_attempt: SystemTime,
    rate_limited: bool,
}

impl Default for ClaudeRefreshGate {
    fn default() -> Self {
        Self { next_attempt: SystemTime::UNIX_EPOCH, rate_limited: false }
    }
}

pub struct ProviderStore {
    last_good: Mutex<HashMap<String, ProviderSnapshot>>,
    cache_path: PathBuf,
    claude_gate: Mutex<ClaudeRefreshGate>,
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
        Self {
            last_good: Mutex::new(last_good),
            cache_path,
            claude_gate: Mutex::new(ClaudeRefreshGate::default()),
        }
    }
}

impl ProviderStore {
    pub async fn snapshots(&self) -> Vec<ProviderSnapshot> {
        let (claude, cursor, codex, antigravity, opencode, openrouter, grok, zcode) = tokio::join!(
            self.claude_snapshot(),
            cursor::snapshot(),
            codex::snapshot(),
            antigravity::snapshot(),
            opencode::snapshot(),
            openrouter::snapshot(),
            grok::snapshot(),
            zcode::snapshot(),
        );
        vec![claude, cursor, codex, antigravity, opencode, openrouter, grok, zcode]
            .into_iter()
            .map(|snapshot| self.with_stale_fallback(snapshot))
            .collect()
    }

    async fn claude_snapshot(&self) -> ProviderSnapshot {
        let now = SystemTime::now();
        let (should_fetch, backing_off) = {
            let mut gate = self.claude_gate.lock().expect("claude refresh gate poisoned");
            if now >= gate.next_attempt {
                gate.next_attempt = now + CLAUDE_MIN_REFRESH;
                (true, gate.rate_limited)
            } else {
                (false, gate.rate_limited)
            }
        };

        if !should_fetch {
            if let Some(mut cached) = self.cached("claude") {
                cached.activity = claude_activity();
                if backing_off {
                    cached.status = "stale".into();
                    cached.message = Some("Claude usage is temporarily rate limited. Codenotch is backing off and will retry automatically.".into());
                }
                return cached;
            }
        }

        let snapshot = claude::snapshot().await;
        let rate_limited = snapshot.status == "stale"
            && snapshot.message.as_deref().is_some_and(|message| message.contains("rate limited"));
        {
            let mut gate = self.claude_gate.lock().expect("claude refresh gate poisoned");
            if rate_limited {
                gate.rate_limited = true;
                gate.next_attempt = SystemTime::now() + CLAUDE_RATE_LIMIT_BACKOFF;
            } else if snapshot.status == "ok" {
                gate.rate_limited = false;
                gate.next_attempt = SystemTime::now() + CLAUDE_MIN_REFRESH;
            } else {
                gate.next_attempt = SystemTime::now() + Duration::from_secs(60);
            }
        }
        snapshot
    }

    fn cached(&self, id: &str) -> Option<ProviderSnapshot> {
        self.last_good.lock().expect("provider cache poisoned").get(id).cloned()
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

pub(crate) fn claude_activity() -> Option<ActivitySummary> {
    let projects = claude_config_dir().join("projects");
    let newest = WalkDir::new(projects).max_depth(5).into_iter().filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file() && entry.path().extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|entry| entry.metadata().ok()?.modified().ok()).max()?;
    let age = SystemTime::now().duration_since(newest).ok()?.as_secs();
    (age <= 8).then(|| ActivitySummary { state: "working".into(), label: Some("Working now".into()) })
}
