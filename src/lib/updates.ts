import { getVersion } from "@tauri-apps/api/app";
import { runningInTauri } from "./backend";

const REPO = "thomaslittle/codenotch-crossplatform";

export interface UpdateInfo {
  available: boolean;
  current: string;
  latest: string;
  url: string;
  checkedAt: number;
}

function splitVersion(value: string): (number | string)[] {
  return value
    .trim()
    .replace(/^v/i, "")
    .split(/[.+-]/)
    .filter((part) => part.length > 0)
    .map((part) => (/^\d+$/.test(part) ? Number(part) : part));
}

/** -1 if a < b, 0 if equal, 1 if a > b. Tolerates `v` prefixes. */
export function compareVersions(a: string, b: string): number {
  const left = splitVersion(a);
  const right = splitVersion(b);
  const length = Math.max(left.length, right.length);
  for (let i = 0; i < length; i++) {
    const x = left[i] ?? 0;
    const y = right[i] ?? 0;
    if (typeof x === "number" && typeof y === "number") {
      if (x !== y) return x < y ? -1 : 1;
    } else if (String(x) !== String(y)) {
      return String(x) < String(y) ? -1 : 1;
    }
  }
  return 0;
}

let cached: Promise<UpdateInfo | null> | null = null;

/**
 * Compares the running app against the latest GitHub release. Result is
 * cached per session; pass `force` for the manual "check again" button.
 * Returns null outside Tauri or when the check can't complete — the UI
 * treats that as "unknown", never as an update.
 */
export function checkForUpdates(force = false): Promise<UpdateInfo | null> {
  if (!runningInTauri()) return Promise.resolve(null);
  if (!force && cached) return cached;
  cached = (async (): Promise<UpdateInfo | null> => {
    const current = await getVersion().catch(() => "0.0.0");
    const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, {
      headers: { Accept: "application/vnd.github+json" },
      signal: AbortSignal.timeout(15_000),
    });
    if (!res.ok) return null;
    const data = (await res.json()) as { tag_name?: unknown; html_url?: unknown };
    const latest = typeof data.tag_name === "string" && data.tag_name ? data.tag_name : null;
    if (!latest) return null;
    return {
      available: compareVersions(current, latest) < 0,
      current,
      latest,
      url: typeof data.html_url === "string" && data.html_url
        ? data.html_url
        : `https://github.com/${REPO}/releases`,
      checkedAt: Date.now(),
    };
  })().catch(() => null);
  return cached;
}
