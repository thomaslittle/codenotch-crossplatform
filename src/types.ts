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
}
