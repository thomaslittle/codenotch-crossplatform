export type Edge = "right" | "left" | "top" | "bottom";
export type Fidelity = "official" | "derived" | "manual";
export type SnapshotStatus =
  | "ok"
  | "stale"
  | "needsAuth"
  | "unsupported"
  | "error";

export type ActivityState = "idle" | "working" | "waiting";

export interface ActivitySummary {
  state: ActivityState;
  label?: string | null;
}

export interface LimitWindow {
  id: string;
  label: string;
  usedFraction: number;
  resetsAt?: string | null;
}

export interface ProviderAccount {
  label?: string | null;
  plan?: string | null;
  source?: string | null;
}

export interface ProviderSnapshot {
  id: string;
  displayName: string;
  glyph: string;
  fidelity: Fidelity;
  status: SnapshotStatus;
  windows: LimitWindow[];
  headlineId?: string | null;
  fetchedAt: string;
  message?: string | null;
  account?: ProviderAccount | null;
  manageUrl?: string | null;
  displayValue?: string | null;
  activity?: ActivitySummary | null;
}

export interface ClientSettings {
  edge: Edge;
  enabledProviders: string[];
  /** Named color/surface preset; `custom` preserves the freeform appearance controls. */
  theme: ThemePresetId;
  /** Outer notch body/silhouette treatment. */
  shellStyle: ShellStyle;
  /** Provider gauge/layout treatment. */
  gaugeStyle: GaugeStyle;
  /** Color scheme source. `system` follows the OS and uses default surfaces. */
  mode: ThemeMode;
  /** Custom notch/card surface (`#rrggbb`). Used in dark/light mode. */
  surface: string;
  /** Notch + card surface opacity, 0–1. */
  opacity: number;
  /** Notch scale factor, 0.7–1.3. */
  scale: number;
  /** Notch nudge in logical px, -200–200. */
  offsetX: number;
  offsetY: number;
  /** `"primary"` or a monitor index from `list_monitors`. */
  monitor: string;
  /** Retract the notch under its docked edge after an idle delay. */
  autoHide: boolean;
  /** Idle delay before auto-hide engages, in seconds (1–60). */
  autoHideDelaySec: number;
}

export type ThemeMode = "dark" | "light" | "system";
export type ThemePresetId = "custom" | "midnight" | "graphite" | "abyss" | "forest" | "plum";
export type ShellStyle = "tab" | "bubble" | "sharp" | "trapezoid" | "pill" | "rail" | "dock" | "ghost";
export type GaugeStyle = "classic" | "slim" | "halo" | "stacked" | "columns" | "micro";

export interface MonitorInfo {
  id: string;
  name: string | null;
  x: number;
  y: number;
  width: number;
  height: number;
  scaleFactor: number;
  primary: boolean;
}
