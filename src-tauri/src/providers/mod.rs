mod antigravity;
mod claude;
mod codex;
mod copilot;
mod cursor;
mod glm;
mod grok;
mod ollama;
mod opencode;

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
        // Claude and Codex support multiple local profiles (`~/.claude-<slug>`,
        // `~/.codex-<slug>`); every extra profile becomes its own ring with a
        // stable id, default first then alphabetical, so rings never swap
        // places. All other providers yield a single snapshot.
        let (claude_profiles, codex_profiles, cursor, antigravity, opencode, copilot, glm, grok, ollama) =
            tokio::join!(
                claude::snapshots(),
                codex::snapshots(),
                cursor::snapshot(),
                antigravity::snapshot(),
                opencode::snapshot(),
                copilot::snapshot(),
                glm::snapshot(),
                grok::snapshot(),
                ollama::snapshot(),
            );
        // The default Claude ring keeps the refresh gate (3-minute cadence +
        // persisted 429 back-off); extra profiles are independent accounts with
        // their own tokens and fetch directly.
        let mut all = Vec::new();
        for snapshot in claude_profiles {
            if snapshot.id == "claude" {
                all.push(self.gated_claude_snapshot(snapshot).await);
            } else {
                all.push(snapshot);
            }
        }
        all.push(cursor);
        all.extend(codex_profiles);
        all.push(antigravity);
        all.push(opencode);
        all.push(copilot);
        all.push(glm);
        all.push(grok);
        all.push(ollama);
        // Deduplicate defensively: profile discovery must never emit the same
        // ring twice.
        let mut seen = std::collections::HashSet::new();
        all.retain(|snapshot| seen.insert(snapshot.id.clone()));
        all.into_iter().map(|snapshot| self.with_stale_fallback(snapshot)).collect()
    }

    /// Applies the shared OAuth refresh gate to a pre-fetched default-profile
    /// Claude snapshot: serves the last good reading when within the quiet
    /// interval instead of hitting the network.
    async fn gated_claude_snapshot(&self, fresh: ProviderSnapshot) -> ProviderSnapshot {
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
            return fresh;
        }

        let rate_limited = fresh.status == "stale"
            && fresh.message.as_deref().is_some_and(|message| message.contains("rate limited"));
        {
            let mut gate = self.claude_gate.lock().expect("claude refresh gate poisoned");
            if rate_limited {
                gate.rate_limited = true;
                gate.next_attempt = SystemTime::now() + CLAUDE_RATE_LIMIT_BACKOFF;
            } else if fresh.status == "ok" {
                gate.rate_limited = false;
                gate.next_attempt = SystemTime::now() + CLAUDE_MIN_REFRESH;
            } else {
                // Authentication/transport errors should not hot-loop either,
                // but they can recover sooner than an explicit 429 bucket.
                gate.next_attempt = SystemTime::now() + Duration::from_secs(60);
            }
        }
        fresh
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
            // Activity is ephemeral local state, not part of the durable usage
            // reading. Never carry a cached `working` spinner forward just
            // because the live quota refresh failed. Claude's local activity
            // remains available even when its usage endpoint is unavailable.
            stale.activity = if stale.id == "claude" || stale.id.starts_with("claude-") {
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
