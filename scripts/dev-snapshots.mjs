/**
 * Dev-only snapshot bridge: `GET /api/snapshots`.
 *
 * The browser cannot run the Rust backend, so `npm run dev` used to render
 * hardcoded demo numbers. This module reads the SAME local sources as
 * `src-tauri/src/providers/*` with plain Node (no new dependencies) and
 * returns the same `ProviderSnapshot` JSON shape, so every number shown in
 * browser dev mode is a real local reading. Anything that cannot be read
 * honestly comes back as `needsAuth`/`error` with a message — never invented.
 *
 * Only wired into `vite.config.ts` (`configureServer`, dev only). Production
 * builds and the Tauri app always use the Rust backend.
 */
import { promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";

const HOME = os.homedir();
const TIMEOUT_MS = 15_000;

function unavailable(id, displayName, glyph, status, message, manageUrl, account) {
  return {
    id,
    displayName,
    glyph,
    fidelity: "official",
    status,
    windows: [],
    headlineId: null,
    fetchedAt: new Date().toISOString(),
    message,
    account: account ?? null,
    manageUrl,
    displayValue: null,
    activity: null,
  };
}

async function fetchJson(url, headers) {
  const res = await fetch(url, { headers, signal: AbortSignal.timeout(TIMEOUT_MS) });
  if (!res.ok) {
    const error = new Error(`HTTP ${res.status}`);
    error.status = res.status;
    throw error;
  }
  return res.json();
}

function parseRfc3339(value) {
  if (!value || typeof value !== "string") return null;
  const ms = Date.parse(value);
  return Number.isNaN(ms) ? null : new Date(ms).toISOString();
}

function expiryMs(value) {
  const num = typeof value === "string" ? Number(value.trim()) : value;
  if (typeof num !== "number" || !Number.isFinite(num) || num <= 0) return null;
  return num < 1e12 ? num * 1000 : num;
}

// --- Claude (mirrors providers/claude.rs) ---------------------------------

async function claudeSnapshot() {
  const ID = "claude";
  const MANAGE = "https://claude.ai/settings/usage";
  const dir =
    process.env.CLAUDE_SECURESTORAGE_CONFIG_DIR ||
    process.env.CLAUDE_CONFIG_DIR ||
    path.join(HOME, ".claude");
  let oauth;
  try {
    const root = JSON.parse(await fs.readFile(path.join(dir, ".credentials.json"), "utf8"));
    oauth = root.claudeAiOauth;
    if (!oauth?.accessToken) throw new Error("missing token");
  } catch {
    return unavailable(ID, "Claude", "✳", "needsAuth",
      `Claude Code credentials were not found at ${path.join(dir, ".credentials.json")}. Run Claude Code and sign in first.`, MANAGE, null);
  }
  const expires = expiryMs(oauth.expiresAt);
  if (expires == null || expires <= Date.now()) {
    return unavailable(ID, "Claude", "✳", "stale",
      "Claude Code's OAuth credential is expired. Run Claude Code once so it can refresh its own login.", MANAGE,
      { label: null, plan: oauth.subscriptionType ?? null, source: "Claude Code" });
  }
  let body;
  try {
    body = await fetchJson("https://api.anthropic.com/api/oauth/usage", {
      Authorization: `Bearer ${oauth.accessToken}`,
      "anthropic-beta": "oauth-2025-04-20",
    });
  } catch (error) {
    const status = error.status === 401 || error.status === 403 ? "needsAuth" : "error";
    return unavailable(ID, "Claude", "✳", status, `Claude usage request failed (HTTP ${error.status ?? "network error"}).`, MANAGE, null);
  }
  const windows = [];
  for (const item of body.limits ?? []) {
    const percent = item.percent ?? item.utilization;
    if (typeof item.kind !== "string" || typeof percent !== "number") continue;
    windows.push({
      id: item.kind,
      label: item.kind === "session" ? "Current session" : item.kind === "weekly_all" ? "All models" : item.kind,
      usedFraction: percent / 100,
      resetsAt: parseRfc3339(item.resets_at ?? item.resetsAt),
    });
  }
  for (const [key, id, label] of [["five_hour", "session", "Current session"], ["seven_day", "weekly_all", "All models"]]) {
    const named = body[key] ?? body[key.replace(/_([a-z])/g, (_, c) => c.toUpperCase())];
    if (!named || windows.some((w) => w.id === id) || typeof named.utilization !== "number") continue;
    windows.push({ id, label, usedFraction: named.utilization / 100, resetsAt: parseRfc3339(named.resets_at ?? named.resetsAt) });
  }
  if (!windows.length) {
    return unavailable(ID, "Claude", "✳", "error", "Claude returned no usage windows.", MANAGE, null);
  }
  return {
    id: ID, displayName: "Claude", glyph: "✳", fidelity: "official", status: "ok",
    windows, headlineId: "session", fetchedAt: new Date().toISOString(), message: null,
    account: { label: null, plan: oauth.subscriptionType ?? null, source: "Claude Code" },
    manageUrl: MANAGE, displayValue: null, activity: null,
  };
}

// --- OpenCode Zen (mirrors providers/opencode.rs) --------------------------

async function opencodeSnapshot() {
  const ID = "opencode";
  const MANAGE = "https://opencode.ai/docs/zen/";
  const account = { label: null, plan: "Zen", source: "OpenCode" };
  const candidates = [
    path.join(HOME, ".local", "share", "opencode", "auth.json"),
  ];
  let apiKey = null;
  let found = false;
  for (const file of candidates) {
    try {
      const root = JSON.parse(await fs.readFile(file, "utf8"));
      found = true;
      if (typeof root.opencode?.key === "string" && root.opencode.key) apiKey = root.opencode.key;
      break;
    } catch { /* try next */ }
  }
  if (!apiKey) {
    return unavailable(ID, "OpenCode", "▣", "needsAuth",
      found
        ? "OpenCode is not connected to Zen (no `opencode` key in auth.json). Run `/connect` in OpenCode and choose OpenCode Zen."
        : "OpenCode auth was not found. Run `/connect` in OpenCode first.", MANAGE, null);
  }
  let body;
  try {
    body = await fetchJson("https://opencode.ai/zen/go/v1/usage", { Authorization: `Bearer ${apiKey}` });
  } catch (error) {
    const status = error.status === 401 || error.status === 403 ? "needsAuth" : "error";
    return unavailable(ID, "OpenCode", "▣", status, `OpenCode usage request failed (HTTP ${error.status ?? "network error"}).`, MANAGE, account);
  }
  const usage = body.usage ?? body;
  const windows = [];
  for (const [id, label] of [["rolling", "Current"], ["weekly", "Weekly"], ["monthly", "Monthly"]]) {
    const bucket = usage[id];
    if (!bucket || typeof bucket.percent !== "number") continue;
    windows.push({ id, label, usedFraction: bucket.percent / 100, resetsAt: parseRfc3339(bucket.resetsAt ?? bucket.resets_at) });
  }
  if (!windows.length) {
    return unavailable(ID, "OpenCode", "▣", "error", "OpenCode returned no usage windows.", MANAGE, account);
  }
  // Headline the most-constrained window, mirroring the Rust backend: a full
  // monthly quota blocks usage even when the rolling window looks fine.
  const headline = windows.reduce((a, b) => (b.usedFraction > a.usedFraction ? b : a), windows[0]);
  return {
    id: ID, displayName: "OpenCode", glyph: "▣", fidelity: "official", status: "ok",
    windows, headlineId: headline.id, fetchedAt: new Date().toISOString(), message: null,
    account, manageUrl: MANAGE, displayValue: null, activity: null,
  };
}

// --- Codex (mirrors providers/codex.rs) ------------------------------------

async function newestRollout(sessionsDir) {
  let newest = null;
  async function walk(dir) {
    let entries;
    try {
      entries = await fs.readdir(dir, { withFileTypes: true });
    } catch { return; }
    for (const entry of entries) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) { await walk(full); continue; }
      if (!entry.isFile() || !entry.name.startsWith("rollout-") || !entry.name.endsWith(".jsonl")) continue;
      const stat = await fs.stat(full).catch(() => null);
      if (!stat) continue;
      if (!newest || stat.mtimeMs > newest.mtimeMs) newest = { path: full, mtimeMs: stat.mtimeMs };
    }
  }
  await walk(sessionsDir);
  return newest;
}

async function codexSnapshot() {
  const ID = "codex";
  const MANAGE = "https://chatgpt.com/#settings/Account";
  const root = process.env.CODEX_HOME || path.join(HOME, ".codex");
  const newest = await newestRollout(path.join(root, "sessions"));
  if (!newest) {
    return unavailable(ID, "Codex", "✦", "needsAuth",
      "No Codex rollout log found yet. Run Codex once so it can record a rate-limit snapshot.", MANAGE, null);
  }
  const text = await fs.readFile(newest.path, "utf8").catch(() => null);
  if (text == null) {
    return unavailable(ID, "Codex", "✦", "error", "Could not read Codex rollout log.", MANAGE, null);
  }
  const lines = text.split("\n").filter((line) => line.includes("rate_limits")).reverse();
  for (const line of lines) {
    let parsed;
    try { parsed = JSON.parse(line); } catch { continue; }
    const limits = parsed.rate_limits ?? parsed.payload?.rate_limits;
    if (!limits) continue;
    const now = Date.now();
    const windows = [];
    for (const [id, fallback] of [["primary", "Current session"], ["secondary", "Longer window"]]) {
      const bucket = limits[id];
      if (!bucket || typeof bucket.used_percent !== "number") continue;
      let resetsAt = null;
      if (typeof bucket.resets_at === "number") resetsAt = new Date(bucket.resets_at * 1000).toISOString();
      else if (typeof bucket.resets_in_seconds === "number") resetsAt = new Date(now + bucket.resets_in_seconds * 1000).toISOString();
      const minutes = bucket.window_minutes;
      const label = typeof minutes === "number" && minutes > 0
        ? minutes < 60 ? `${Math.trunc(minutes)}m limit` : minutes < 1440 ? `${Math.trunc(minutes / 60)}h limit` : "Weekly limit"
        : fallback;
      windows.push({ id, label, usedFraction: bucket.used_percent / 100, resetsAt });
    }
    if (windows.length) {
      return {
        id: ID, displayName: "Codex", glyph: "✦", fidelity: "official", status: "ok",
        windows, headlineId: "primary", fetchedAt: new Date().toISOString(), message: null,
        account: { label: null, plan: null, source: "Codex" },
        manageUrl: MANAGE, displayValue: null, activity: null,
      };
    }
  }
  return unavailable(ID, "Codex", "✦", "error", "Codex has not recorded a usage snapshot in its latest rollout yet.", MANAGE, null);
}

// --- Cursor (mirrors providers/cursor.rs) -----------------------------------

async function cursorSnapshot() {
  const ID = "cursor";
  const MANAGE = "https://cursor.com/dashboard";
  const store = process.platform === "win32"
    ? path.join(process.env.APPDATA || path.join(HOME, "AppData", "Roaming"), "Cursor", "User", "globalStorage", "state.vscdb")
    : path.join(process.env.XDG_CONFIG_HOME || path.join(HOME, ".config"), "Cursor", "User", "globalStorage", "state.vscdb");
  let get;
  try {
    const { DatabaseSync } = await import("node:sqlite");
    const db = new DatabaseSync(store, { readOnly: true });
    get = (key) => {
      try { return db.prepare("SELECT value FROM ItemTable WHERE key = ?").get(key)?.value ?? null; }
      catch { return null; }
    };
    // Probe now so a locked/missing store fails fast with an honest status.
    get("cursorAuth/accessToken");
  } catch {
    return unavailable(ID, "Cursor", "⌾", "needsAuth",
      `Cursor state database is unavailable at ${store}. Open Cursor and sign in.`, MANAGE, null);
  }
  const access = get("cursorAuth/accessToken");
  const accountId = get("cursorAuth/stripeMembershipAuthId");
  const account = {
    label: get("cursorAuth/cachedEmail"),
    plan: get("cursorAuth/stripeMembershipType"),
    source: "Cursor",
  };
  if (!access || !accountId) {
    return unavailable(ID, "Cursor", "⌾", "needsAuth", "Cursor access token is missing. Open Cursor and sign in.", MANAGE, account);
  }
  let body;
  try {
    body = await fetchJson("https://cursor.com/api/usage-summary", {
      Accept: "application/json",
      Cookie: `WorkosCursorSessionToken=${accountId}::${access}`,
    });
  } catch (error) {
    const status = error.status === 401 || error.status === 403 ? "needsAuth" : "error";
    return unavailable(ID, "Cursor", "⌾", status, `Cursor usage request failed (HTTP ${error.status ?? "network error"}).`, MANAGE, account);
  }
  const resetsAt = parseRfc3339(body.billingCycleEnd);
  const windows = [];
  const total = body.individualUsage?.plan?.totalPercentUsed;
  if (typeof total === "number") windows.push({ id: "included", label: "Included usage", usedFraction: total / 100, resetsAt });
  if (windows.length === 0) {
    return unavailable(ID, "Cursor", "⌾", "error", "Cursor returned no metered usage windows.", MANAGE, account);
  }
  return {
    id: ID, displayName: "Cursor", glyph: "⌾", fidelity: "official", status: "ok",
    windows, headlineId: "included", fetchedAt: new Date().toISOString(), message: null,
    account, manageUrl: MANAGE, displayValue: null, activity: null,
  };
}

// --- Antigravity derived fallback (no OS keyring in plain Node) --------------

async function antigravitySnapshot() {
  const ID = "gemini";
  const MANAGE = "https://antigravity.google/";
  const account = { label: null, plan: "Personal", source: "Antigravity" };
  const brain = path.join(HOME, ".gemini", "antigravity", "brain");
  let count = 0;
  let seen = false;
  const today = new Date().toLocaleDateString("en-CA");
  let trajectories;
  try {
    trajectories = await fs.readdir(brain, { withFileTypes: true });
  } catch {
    return unavailable(ID, "Antigravity", "◆", "needsAuth",
      "Antigravity credential is unavailable in browser dev (no OS keyring). Open Antigravity so it can sign in, or run the Tauri app for the full reading.", MANAGE, null);
  }
  for (const entry of trajectories) {
    if (!entry.isDirectory()) continue;
    const file = path.join(brain, entry.name, ".system_generated", "logs", "transcript.jsonl");
    const text = await fs.readFile(file, "utf8").catch(() => null);
    if (text == null) continue;
    seen = true;
    for (const line of text.split("\n")) {
      if (!line.includes('"MODEL"')) continue;
      let value;
      try { value = JSON.parse(line); } catch { continue; }
      if (value.source !== "MODEL" || typeof value.created_at !== "string") continue;
      const at = new Date(value.created_at);
      if (Number.isNaN(at.getTime())) continue;
      const day = `${at.getFullYear()}-${String(at.getMonth() + 1).padStart(2, "0")}-${String(at.getDate()).padStart(2, "0")}`;
      if (day === today) count += 1;
    }
  }
  if (!seen) {
    return unavailable(ID, "Antigravity", "◆", "needsAuth",
      "No Antigravity transcripts found yet. Open Antigravity so it can sign in.", MANAGE, null);
  }
  return {
    id: ID, displayName: "Antigravity", glyph: "◆", fidelity: "derived", status: "ok",
    windows: [], headlineId: null, fetchedAt: new Date().toISOString(),
    message: `~${count} request${count === 1 ? "" : "s"} today · Google publishes no limit for this account`,
    account, manageUrl: MANAGE, displayValue: `~${count}`, activity: null,
  };
}

export async function devSnapshots() {
  const [claude, cursor, codex, antigravity, opencode] = await Promise.all([
    claudeSnapshot().catch((e) => unavailable("claude", "Claude", "✳", "error", String(e), "https://claude.ai/settings/usage", null)),
    cursorSnapshot().catch((e) => unavailable("cursor", "Cursor", "⌾", "error", String(e), "https://cursor.com/dashboard", null)),
    codexSnapshot().catch((e) => unavailable("codex", "Codex", "✦", "error", String(e), "https://chatgpt.com/#settings/Account", null)),
    antigravitySnapshot().catch((e) => unavailable("gemini", "Antigravity", "◆", "error", String(e), "https://antigravity.google/", null)),
    opencodeSnapshot().catch((e) => unavailable("opencode", "OpenCode", "▣", "error", String(e), "https://opencode.ai/docs/zen/", null)),
  ]);
  return [claude, cursor, codex, antigravity, opencode];
}

export function devSnapshotsPlugin() {
  let cache = null;
  let cachedAt = 0;
  return {
    name: "codenotch-snapshots",
    configureServer(server) {
      server.middlewares.use("/api/snapshots", async (_req, res) => {
        try {
          if (!cache || Date.now() - cachedAt > 5_000) {
            cache = await devSnapshots();
            cachedAt = Date.now();
          }
          res.setHeader("Content-Type", "application/json");
          res.end(JSON.stringify(cache));
        } catch (error) {
          res.statusCode = 500;
          res.setHeader("Content-Type", "application/json");
          res.end(JSON.stringify({ error: String(error) }));
        }
      });
    },
  };
}
