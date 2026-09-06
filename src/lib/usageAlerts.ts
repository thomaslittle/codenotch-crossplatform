import type { ProviderSnapshot } from "../types";

const BASELINE_KEY = "codenotch-crossplatform.usage-reset-baseline.v1";
const ALERTS_KEY = "codenotch-crossplatform.usage-reset-alerts.v1";
const MIN_DROP = 0.05;

type BaselineWindow = {
  usedFraction: number;
  resetsAt: string | null;
};

type Baseline = Record<string, BaselineWindow>;

export interface UsageResetAlert {
  id: string;
  providerId: string;
  providerName: string;
  windowId: string;
  windowLabel: string;
  resetAt: string;
  detectedAt: string;
  previousUsedFraction: number;
  currentUsedFraction: number;
}

function readJson<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? JSON.parse(raw) as T : fallback;
  } catch {
    return fallback;
  }
}

function writeJson(key: string, value: unknown): void {
  localStorage.setItem(key, JSON.stringify(value));
}

function baselineKey(providerId: string, windowId: string): string {
  return `${providerId}:${windowId}`;
}

function resetAdvanced(previous: string | null, current: string | null): current is string {
  if (!previous || !current) return false;
  const previousMs = Date.parse(previous);
  const currentMs = Date.parse(current);
  return Number.isFinite(previousMs) && Number.isFinite(currentMs) && currentMs > previousMs;
}

export function getUsageResetAlerts(): UsageResetAlert[] {
  const value = readJson<unknown>(ALERTS_KEY, []);
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is UsageResetAlert => {
    if (!item || typeof item !== "object") return false;
    const alert = item as Partial<UsageResetAlert>;
    return typeof alert.id === "string"
      && typeof alert.providerId === "string"
      && typeof alert.providerName === "string"
      && typeof alert.windowId === "string"
      && typeof alert.windowLabel === "string"
      && typeof alert.resetAt === "string"
      && typeof alert.detectedAt === "string"
      && typeof alert.previousUsedFraction === "number"
      && typeof alert.currentUsedFraction === "number";
  });
}

export function clearUsageResetAlerts(providerId?: string): UsageResetAlert[] {
  const next = providerId
    ? getUsageResetAlerts().filter((alert) => alert.providerId !== providerId)
    : [];
  writeJson(ALERTS_KEY, next);
  return next;
}

/**
 * Compare fresh official snapshots to the last good baseline.
 * A reset requires both a later reset timestamp and a material usage drop,
 * so stale-data repair or tiny accounting corrections do not notify.
 */
export function processUsageResetSnapshots(snapshots: ProviderSnapshot[]): UsageResetAlert[] {
  const baseline = readJson<Baseline>(BASELINE_KEY, {});
  const alerts = getUsageResetAlerts();
  const knownAlertIds = new Set(alerts.map((alert) => alert.id));
  const nextBaseline: Baseline = { ...baseline };

  for (const snapshot of snapshots) {
    if (snapshot.status !== "ok") continue;

    for (const window of snapshot.windows) {
      if (!Number.isFinite(window.usedFraction)) continue;
      const key = baselineKey(snapshot.id, window.id);
      const previous = baseline[key];
      const currentReset = window.resetsAt ?? null;

      if (
        previous
        && resetAdvanced(previous.resetsAt, currentReset)
        && previous.usedFraction - window.usedFraction >= MIN_DROP
      ) {
        const alertId = `${key}:${currentReset}`;
        if (!knownAlertIds.has(alertId)) {
          alerts.push({
            id: alertId,
            providerId: snapshot.id,
            providerName: snapshot.displayName,
            windowId: window.id,
            windowLabel: window.label,
            resetAt: currentReset,
            detectedAt: new Date().toISOString(),
            previousUsedFraction: previous.usedFraction,
            currentUsedFraction: window.usedFraction,
          });
          knownAlertIds.add(alertId);
        }
      }

      nextBaseline[key] = {
        usedFraction: window.usedFraction,
        resetsAt: currentReset,
      };
    }
  }

  writeJson(BASELINE_KEY, nextBaseline);
  writeJson(ALERTS_KEY, alerts);
  return alerts;
}
