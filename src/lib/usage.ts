import type { LimitWindow, ProviderSnapshot } from "../types";

export type UsageBand = "ample" | "watch" | "critical" | "exhausted";

export const PALETTE = {
  notch: "#000000",
  card: "#000000",
  ringTrack: "#303030",
  barTrack: "#2d2d2d",
  ample: "#00ff88",
  watch: "#f2ff00",
  critical: "#ff3f00",
  textPrimary: "#ffffff",
  textSecondary: "#808080",
} as const;

export function clamp01(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.min(1, Math.max(0, value));
}

export function usageBand(value: number): UsageBand {
  const used = clamp01(value);
  if (used >= 1) return "exhausted";
  if (used >= 0.8) return "critical";
  if (used >= 0.5) return "watch";
  return "ample";
}

/** Convert provider-native used quota into the user-facing amount left. */
export function remainingFraction(usedFraction: number): number {
  return clamp01(1 - clamp01(usedFraction));
}

/** Availability semantics: full/healthy is green; depletion moves yellow -> red. */
export function availabilityBand(value: number): UsageBand {
  const remaining = clamp01(value);
  if (remaining <= 0) return "exhausted";
  if (remaining <= 0.2) return "critical";
  if (remaining <= 0.5) return "watch";
  return "ample";
}

export function bandColor(value: number): string {
  switch (usageBand(value)) {
    case "ample":
      return PALETTE.ample;
    case "watch":
      return PALETTE.watch;
    case "critical":
    case "exhausted":
      return PALETTE.critical;
  }
}

export function headlineWindow(snapshot: ProviderSnapshot): LimitWindow | undefined {
  if (snapshot.headlineId) {
    const explicit = snapshot.windows.find((window) => window.id === snapshot.headlineId);
    if (explicit) return explicit;
  }
  return [...snapshot.windows].sort((a, b) => b.usedFraction - a.usedFraction)[0];
}

export function formatPercent(value: number): string {
  return `${Math.round(clamp01(value) * 100)}%`;
}

export function formatReset(resetsAt?: string | null, now = new Date()): string {
  if (!resetsAt) return "Reset time unavailable";
  const reset = new Date(resetsAt);
  if (Number.isNaN(reset.getTime())) return "Reset time unavailable";

  const diffMs = reset.getTime() - now.getTime();
  if (diffMs <= 0) return "Resetting now";

  const minutes = Math.ceil(diffMs / 60_000);
  if (minutes < 60) return `Resets in ${minutes} min`;

  if (minutes < 12 * 60) {
    const hours = Math.floor(minutes / 60);
    const rem = minutes % 60;
    return rem ? `Resets in ${hours}h ${rem}m` : `Resets in ${hours}h`;
  }

  return `Resets ${new Intl.DateTimeFormat(undefined, {
    weekday: "short",
    hour: "numeric",
    minute: "2-digit",
  }).format(reset)}`;
}
