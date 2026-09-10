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
        ? minutes < 60 ? `${Math.trunc(minutes)}m limit` : minutes < 1440 ? `${Math.trunc(minutes / 60)}h limit` : Math.round(minutes / 1440) === 30 ? "Monthly limit" : Math.round(minutes / 1440) === 7 ? "Weekly limit" : `${Math.round(minutes / 1440)}d limit`
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
  const plan = body.individualUsage?.plan ?? {};
  const usage = body.individualUsage ?? {};
  const team = body.teamUsage ?? {};
  const spend = (bucket) => {
    if (!bucket || bucket.enabled !== true) return null;
    if (typeof bucket.limit !== "number" || bucket.limit <= 0) return null;
    if (typeof bucket.used !== "number") return null;
    return bucket.used / bucket.limit;
  };
  if (typeof plan.autoPercentUsed === "number") {
    windows.push({ id: "auto", label: "Auto usage", usedFraction: plan.autoPercentUsed / 100, resetsAt });
  } else if (typeof plan.totalPercentUsed === "number") {
    windows.push({ id: "included", label: "Included usage", usedFraction: plan.totalPercentUsed / 100, resetsAt });
  }
  if (typeof plan.apiPercentUsed === "number" && plan.apiPercentUsed > 0) {
    windows.push({ id: "api", label: "API usage", usedFraction: plan.apiPercentUsed / 100, resetsAt });
  }
  const onDemand = spend(usage.onDemand);
  if (onDemand != null) windows.push({ id: "on_demand", label: "On demand", usedFraction: onDemand, resetsAt });
  if (windows.length === 0) {
    const overall = spend(usage.overall);
    if (overall != null) windows.push({ id: "included", label: "Included usage", usedFraction: overall, resetsAt });
  }
  const teamOnDemand = spend(team.onDemand);
  if (teamOnDemand != null && teamOnDemand > 0) {
    windows.push({ id: "team_on_demand", label: "Team on demand", usedFraction: teamOnDemand, resetsAt });
  }
  if (windows.length === 0) {
    const membership = body.membershipType ?? "this";
    const message = body.isUnlimited === true
      ? `Unlimited on the ${membership} plan — nothing to meter`
      : `The ${membership} plan has nothing for Cursor to meter yet`;
    return unavailable(ID, "Cursor", "⌾", "error", message, MANAGE, account);
  }
  const headline = ["auto", "included", "api"].find((id) => windows.some((w) => w.id === id)) ?? windows[0].id;
  return {
    id: ID, displayName: "Cursor", glyph: "⌾", fidelity: "official", status: "ok",
    windows, headlineId: headline, fetchedAt: new Date().toISOString(), message: null,
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

// --- GitHub Copilot (mirrors providers/copilot.rs) ---------------------------

function parseGhHosts(text) {
  const lines = String(text ?? "").split("\n");
  const start = lines.findIndex((line) => line.trim() === "github.com:");
  if (start < 0) return {};
  let username;
  let token;
  for (const line of lines.slice(start + 1)) {
    if (!line.startsWith(" ") && !line.startsWith("\t")) break;
    const trimmed = line.trim();
    for (const key of ["user", "oauth_token"]) {
      if (!trimmed.startsWith(`${key}:`)) continue;
      const value = trimmed.slice(key.length + 1).trim().replace(/^["']|["']$/g, "").trim();
      if (!value) continue;
      if (key === "user") username = value;
      else token = value;
    }
  }
  return { username, token };
}

async function copilotSnapshot() {
  const ID = "copilot";
  const MANAGE = "https://github.com/settings/copilot";
  const envToken = (process.env.GH_TOKEN || process.env.GITHUB_TOKEN || "").trim();
  let token = envToken || null;
  let source = envToken ? "GitHub" : null;
  let username;
  const hostsFile = process.platform === "win32"
    ? path.join(process.env.APPDATA || path.join(HOME, "AppData", "Roaming"), "GitHub CLI", "hosts.yml")
    : path.join(HOME, ".config", "gh", "hosts.yml");
  try {
    const parsed = parseGhHosts(await fs.readFile(hostsFile, "utf8"));
    username = parsed.username;
    if (!token && parsed.token) {
      token = parsed.token;
      source = "GitHub CLI";
    }
  } catch { /* no hosts file */ }
  const account = username || source ? { label: username ?? null, plan: null, source: source ?? "GitHub" } : null;
  if (!token) {
    return unavailable(ID, "GitHub Copilot", "⛁", "needsAuth",
      "Sign in with GitHub CLI using `gh auth login`, then enable GitHub Copilot.", MANAGE, account);
  }
  let body;
  try {
    body = await fetchJson("https://api.github.com/copilot_internal/user", {
      Authorization: `Bearer ${token}`,
      Accept: "application/json",
      "X-GitHub-Api-Version": "2022-11-28",
    });
  } catch (error) {
    const status = error.status === 401 || error.status === 403 ? "needsAuth" : "error";
    return unavailable(ID, "GitHub Copilot", "⛁", status, `GitHub Copilot usage request failed (HTTP ${error.status ?? "network error"}).`, MANAGE, account);
  }
  const quotas = body.quota_snapshots ?? {};
  const order = ["premium_interactions", "chat", "completions"];
  const keys = [...order, ...Object.keys(quotas).filter((k) => !order.includes(k)).sort()];
  const labelFor = (id) => id === "premium_interactions" ? "Premium requests" : id === "chat" ? "Chat requests" : id === "completions" ? "Completions" : id.replace(/_/g, " ");
  const windows = [];
  for (const key of keys) {
    const quota = quotas[key];
    if (!quota || quota.unlimited === true || quota.entitlement === 0) continue;
    const entitlement = quota.entitlement;
    const remaining = quota.remaining;
    const used = quota.used;
    const resetsAt = parseRfc3339(quota.reset_date ?? quota.reset_at ?? quota.resets_at ?? body.quota_reset_date);
    if (typeof entitlement === "number" && entitlement > 0) {
      const consumed = typeof used === "number" ? used : Math.max(0, entitlement - (remaining ?? entitlement));
      windows.push({ id: key, label: labelFor(key), usedFraction: Math.max(0, consumed / entitlement), resetsAt });
    }
  }
  if (!windows.length) {
    return unavailable(ID, "GitHub Copilot", "⛁", "error", "GitHub Copilot reported no metered quotas.", MANAGE, account);
  }
  const headline = windows.some((w) => w.id === "premium_interactions") ? "premium_interactions" : windows[0].id;
  return {
    id: ID, displayName: "GitHub Copilot", glyph: "⛁", fidelity: "official", status: "ok",
    windows, headlineId: headline, fetchedAt: new Date().toISOString(), message: null,
    account, manageUrl: MANAGE, displayValue: null, activity: null,
  };
}

// --- GLM (mirrors providers/glm.rs) ------------------------------------------

async function readJsonFile(file) {
  try {
    return JSON.parse(await fs.readFile(file, "utf8"));
  } catch {
    return null;
  }
}

function glmCredential() {
  const claudeSettings = path.join(HOME, ".claude", "settings.json");
  return readJsonFile(claudeSettings).then((root) => {
    const env = root?.env ?? {};
    const token = env.ANTHROPIC_AUTH_TOKEN || env.ANTHROPIC_API_KEY;
    const base = env.ANTHROPIC_BASE_URL || "";
    const host = base.split("://")[1]?.split("/")[0] ?? "";
    if (token && (host === "api.z.ai" || host.endsWith(".z.ai") || host.endsWith(".bigmodel.cn"))) {
      return { token, baseUrl: host.endsWith("bigmodel.cn") ? "https://open.bigmodel.cn" : "https://api.z.ai", source: "Claude Code" };
    }
    return null;
  });
}

async function glmSnapshot() {
  const ID = "glm";
  const MANAGE_GLOBAL = "https://z.ai/manage-apikey/apikey-list";
  let credential = await glmCredential();
  if (!credential) {
    const zcodeConfig = await readJsonFile(path.join(HOME, ".zcode", "v2", "config.json"));
    const providers = zcodeConfig?.provider ?? {};
    for (const pid of Object.keys(providers).sort()) {
      if (!pid.includes("coding-plan")) continue;
      const provider = providers[pid] ?? {};
      if (provider.enabled === false) continue;
      const key = provider.options?.apiKey;
      if (!key) continue;
      const host = provider.options?.baseURL?.split("://")[1]?.split("/")[0] ?? "";
      credential = { token: key, baseUrl: host.endsWith("bigmodel.cn") ? "https://open.bigmodel.cn" : "https://api.z.ai", source: "ZCode" };
      break;
    }
  }
  if (!credential) {
    const zcodeCreds = await readJsonFile(path.join(HOME, ".zcode", "v2", "credentials.json"));
    const token = zcodeCreds?.["oauth:zai:access_token"];
    if (token && !token.startsWith("enc:v1:")) {
      credential = { token, baseUrl: "https://api.z.ai", source: "ZCode" };
    }
  }
  if (!credential) {
    const auth = await readJsonFile(path.join(HOME, ".local", "share", "opencode", "auth.json"));
    for (const pid of ["zai-coding-plan", "zai", "z-ai", "z.ai", "glm", "zhipu", "zhipuai"]) {
      const entry = auth?.[pid];
      const key = typeof entry === "string" ? entry : entry == null ? null
        : ["apiKey", "api_key", "token", "key", "accessToken", "auth_token"].map((f) => entry[f]).find((v) => typeof v === "string" && v);
      if (key) {
        credential = { token: key, baseUrl: pid.startsWith("zhipu") ? "https://open.bigmodel.cn" : "https://api.z.ai", source: "OpenCode" };
        break;
      }
    }
  }
  if (!credential) {
    return unavailable(ID, "GLM", "△", "needsAuth",
      "Usage rides on a Z.ai GLM Coding Plan key held by a coding tool — Claude Code's settings.json, ZCode or OpenCode. Set one up there and the notch reads it.",
      MANAGE_GLOBAL, null);
  }
  const manage = credential.baseUrl.includes("bigmodel.cn") ? "https://open.bigmodel.cn/usage" : MANAGE_GLOBAL;
  const account = { label: null, plan: null, source: credential.source };
  let body;
  try {
    const res = await fetch(`${credential.baseUrl}/api/monitor/usage/quota/limit`, {
      headers: { Authorization: credential.token, "Content-Type": "application/json" },
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
    if (res.status === 401 || res.status === 403) {
      return unavailable(ID, "GLM", "△", "needsAuth", "Z.ai rejected the borrowed plan key. Refresh it in the tool that holds it.", manage, account);
    }
    if (!res.ok) {
      return unavailable(ID, "GLM", "△", "error", `Z.ai usage endpoint returned HTTP ${res.status}`, manage, account);
    }
    body = await res.json();
  } catch (error) {
    return unavailable(ID, "GLM", "△", "error", `GLM usage request failed (${error.status ? `HTTP ${error.status}` : "network error"}).`, manage, account);
  }
  if (body && typeof body.code === "number" && (body.success !== true || body.code !== 200)) {
    const status = body.code === 401 || body.code === 403 ? "needsAuth" : "error";
    return unavailable(ID, "GLM", "△", status, "Z.ai rejected the borrowed plan key.", manage, account);
  }
  const data = body.data ?? body;
  const limits = data.limits ?? [];
  const idFor = (limit) => {
    if (limit.type === "TIME_LIMIT") return ["mcp", "MCP (1 month)"];
    if (limit.unit === 3 && limit.number === 5) return ["session", "Current session"];
    if (limit.unit === 6 && limit.number === 1) return ["weekly", "Weekly"];
    return [String(limit.type ?? "usage").toLowerCase(), "Usage"];
  };
  const rank = (id) => id === "session" ? 0 : id === "weekly" ? 1 : id === "mcp" ? 2 : 3;
  const windows = limits
    .filter((limit) => typeof limit.percentage === "number")
    .map((limit) => {
      const [id, label] = idFor(limit);
      const resetsAt = typeof limit.nextResetTime === "number" ? new Date(limit.nextResetTime).toISOString() : null;
      return { id, label, usedFraction: limit.percentage / 100, resetsAt };
    })
    .sort((a, b) => rank(a.id) - rank(b.id));
  if (!windows.length) {
    return unavailable(ID, "GLM", "△", "error", "Z.ai returned no usage windows.", manage, account);
  }
  return {
    id: ID, displayName: "GLM", glyph: "△", fidelity: "official", status: "ok",
    windows, headlineId: "session", fetchedAt: new Date().toISOString(), message: null,
    account: { label: null, plan: data.level ?? null, source: credential.source },
    manageUrl: manage, displayValue: null, activity: null,
  };
}

// --- Grok (mirrors providers/grok.rs) -----------------------------------------

async function grokSnapshot() {
  const ID = "grok";
  const MANAGE = "https://grok.com/?_s=usage";
  const TRUSTED = "https://auth.x.ai";
  let root;
  try {
    root = JSON.parse(await fs.readFile(path.join(HOME, ".grok", "auth.json"), "utf8"));
  } catch {
    return unavailable(ID, "Grok", "⛭", "needsAuth", "Run `grok login` — it signs in and refreshes the token this reads.", MANAGE, null);
  }
  const entries = Object.entries(root ?? {}).filter(([key, entry]) =>
    entry && typeof entry === "object" && (key.startsWith(TRUSTED) || entry.oidc_issuer === TRUSTED));
  if (!entries.length) {
    return unavailable(ID, "Grok", "⛭", "needsAuth", "Grok CLI session was not found in auth.json.", MANAGE, null);
  }
  const now = Date.now();
  entries.sort(([ , a], [ , b]) => {
    const live = (e) => !e.expires_at || Date.parse(e.expires_at) > now ? 0 : 1;
    return live(a) - live(b);
  });
  const entry = entries[0][1];
  if (!entry.key) {
    return unavailable(ID, "Grok", "⛭", "needsAuth", "Grok CLI access token is missing.", MANAGE, null);
  }
  const account = { label: entry.email ?? null, plan: null, source: "Grok" };
  if (entry.expires_at && Date.parse(entry.expires_at) <= now) {
    return unavailable(ID, "Grok", "⛭", "stale", "Grok's saved CLI session is expired. Run `grok login` so it can refresh its own token.", MANAGE, account);
  }
  let body;
  try {
    const res = await fetch("https://cli-chat-proxy.grok.com/v1/billing?format=credits", {
      headers: { Authorization: `Bearer ${entry.key}`, "X-XAI-Token-Auth": "xai-grok-cli", Accept: "application/json" },
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
    if (res.status === 401 || res.status === 403) {
      return unavailable(ID, "Grok", "⛭", "needsAuth", "Grok rejected the saved CLI session. Run `grok login` and sign in again.", MANAGE, account);
    }
    if (!res.ok) {
      return unavailable(ID, "Grok", "⛭", "error", `Grok usage endpoint returned HTTP ${res.status}`, MANAGE, account);
    }
    body = await res.json();
  } catch (error) {
    return unavailable(ID, "Grok", "⛭", "error", `Grok usage request failed (${error.status ? `HTTP ${error.status}` : "network error"}).`, MANAGE, account);
  }
  const config = body.config;
  if (!config) {
    return unavailable(ID, "Grok", "⛭", "error", "Grok returned no usage config.", MANAGE, account);
  }
  const resetsAt = parseRfc3339(config.currentPeriod?.end ?? config.billingPeriodEnd);
  const humanize = (name) => String(name ?? "Usage").replace(/([a-z])([A-Z])/g, "$1 $2");
  const windows = [];
  if (typeof config.creditUsagePercent === "number") {
    const label = humanize(config.productUsage?.[0]?.product ?? "GrokBuild");
    windows.push({ id: "credits", label, usedFraction: config.creditUsagePercent / 100, resetsAt });
  } else if (Array.isArray(config.productUsage)) {
    config.productUsage.forEach((product, index) => {
      if (typeof product.usagePercent !== "number") return;
      windows.push({
        id: index === 0 ? "credits" : String(product.product ?? humanize(product.product)),
        label: humanize(product.product),
        usedFraction: product.usagePercent / 100,
        resetsAt,
      });
    });
  }
  if (!windows.length && String(config.currentPeriod?.type ?? "").includes("WEEKLY")) {
    windows.push({ id: "credits", label: "Weekly limit", usedFraction: 0, resetsAt });
  }
  if (!windows.length) {
    return unavailable(ID, "Grok", "⛭", "error", "Grok has nothing metered on this account yet.", MANAGE, account);
  }
  return {
    id: ID, displayName: "Grok", glyph: "⛭", fidelity: "official", status: "ok",
    windows, headlineId: "credits", fetchedAt: new Date().toISOString(), message: null,
    account, manageUrl: MANAGE, displayValue: null, activity: null,
  };
}

// --- Ollama (mirrors providers/ollama.rs) --------------------------------------

async function ollamaSnapshot() {
  const ID = "ollama";
  const MANAGE = "https://ollama.com";
  const DEFAULT = "http://127.0.0.1:11434";
  const raw = (process.env.OLLAMA_HOST || DEFAULT).trim().replace(/\/+$/, "");
  let base = DEFAULT;
  try {
    const url = new URL(raw);
    if ((url.protocol === "http:" || url.protocol === "https:") && ["localhost", "127.0.0.1", "::1", "[::1]"].includes(url.hostname)) {
      base = raw;
    }
  } catch { /* keep default */ }
  let body;
  try {
    const res = await fetch(`${base}/api/ps`, {
      headers: { Accept: "application/json" },
      signal: AbortSignal.timeout(4000),
    });
    if (!res.ok) {
      return unavailable(ID, "Ollama", "◉", "unsupported", `Ollama returned HTTP ${res.status}. Check the server address and configuration.`, MANAGE, null);
    }
    body = await res.json();
  } catch {
    return unavailable(ID, "Ollama", "◉", "unsupported", "Ollama server unavailable. Open Ollama and check the server address.", MANAGE, null);
  }
  const models = Array.isArray(body.models) ? body.models.filter((m) => m && typeof m.name === "string" && m.name.trim()) : null;
  if (!models) {
    return unavailable(ID, "Ollama", "◉", "unsupported", "This server did not return an Ollama model listing.", MANAGE, null);
  }
  const account = { label: base, plan: "Local", source: "Ollama" };
  if (!models.length) {
    return {
      id: ID, displayName: "Ollama", glyph: "◉", fidelity: "official", status: "ok",
      windows: [], headlineId: null, fetchedAt: new Date().toISOString(),
      message: "Ollama is running with no models loaded.",
      account, manageUrl: MANAGE, displayValue: "idle", activity: null,
    };
  }
  const windows = [...new Set(models.map((m) => m.name.trim()))].sort().map((name) => ({ id: name, label: name, usedFraction: 0, resetsAt: null }));
  return {
    id: ID, displayName: "Ollama", glyph: "◉", fidelity: "official", status: "ok",
    windows, headlineId: null, fetchedAt: new Date().toISOString(),
    message: models.map((m) => m.name.trim()).sort().join("\n"),
    account, manageUrl: MANAGE,
    displayValue: `${models.length} model${models.length === 1 ? "" : "s"}`,
    activity: null,
  };
}

export async function devSnapshots() {
  const [claude, cursor, codex, antigravity, opencode, copilot, glm, grok, ollama] = await Promise.all([
    claudeSnapshot().catch((e) => unavailable("claude", "Claude", "✳", "error", String(e), "https://claude.ai/settings/usage", null)),
    cursorSnapshot().catch((e) => unavailable("cursor", "Cursor", "⌾", "error", String(e), "https://cursor.com/dashboard", null)),
    codexSnapshot().catch((e) => unavailable("codex", "Codex", "✦", "error", String(e), "https://chatgpt.com/#settings/Account", null)),
    antigravitySnapshot().catch((e) => unavailable("gemini", "Antigravity", "◆", "error", String(e), "https://antigravity.google/", null)),
    opencodeSnapshot().catch((e) => unavailable("opencode", "OpenCode", "▣", "error", String(e), "https://opencode.ai/docs/zen/", null)),
    copilotSnapshot().catch((e) => unavailable("copilot", "GitHub Copilot", "⛁", "error", String(e), "https://github.com/settings/copilot", null)),
    glmSnapshot().catch((e) => unavailable("glm", "GLM", "△", "error", String(e), "https://z.ai/manage-apikey/apikey-list", null)),
    grokSnapshot().catch((e) => unavailable("grok", "Grok", "⛭", "error", String(e), "https://grok.com/?_s=usage", null)),
    ollamaSnapshot().catch((e) => unavailable("ollama", "Ollama", "◉", "unsupported", String(e), "https://ollama.com", null)),
  ]);
  return [claude, cursor, codex, antigravity, opencode, copilot, glm, grok, ollama];
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
