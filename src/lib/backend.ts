import { invoke } from "@tauri-apps/api/core";
import type { Edge, ProviderSnapshot } from "../types";

const demoSnapshots: ProviderSnapshot[] = [
  {
    id: "claude",
    displayName: "Claude",
    glyph: "✳",
    fidelity: "official",
    status: "ok",
    headlineId: "session",
    fetchedAt: new Date().toISOString(),
    manageUrl: "https://claude.ai/settings/usage",
    account: { plan: "Max", source: "Claude Code" },
    activity: { state: "working", label: "Working" },
    windows: [
      {
        id: "session",
        label: "Current session",
        usedFraction: 0.73,
        resetsAt: new Date(Date.now() + 51 * 60_000).toISOString(),
      },
      {
        id: "weekly_all",
        label: "All models",
        usedFraction: 0.07,
        resetsAt: new Date(Date.now() + 3 * 24 * 60 * 60_000).toISOString(),
      },
    ],
  },
  {
    id: "cursor",
    displayName: "Cursor",
    glyph: "⌾",
    fidelity: "official",
    status: "ok",
    headlineId: "included",
    fetchedAt: new Date().toISOString(),
    manageUrl: "https://cursor.com/dashboard",
    windows: [
      {
        id: "included",
        label: "Included usage",
        usedFraction: 0.21,
        resetsAt: new Date(Date.now() + 11 * 24 * 60 * 60_000).toISOString(),
      },
    ],
  },
  {
    id: "gemini",
    displayName: "Antigravity",
    glyph: "◆",
    fidelity: "derived",
    status: "ok",
    fetchedAt: new Date().toISOString(),
    manageUrl: "https://antigravity.google/",
    displayValue: "~18",
    message: "~18 requests today · no limit published",
    account: { plan: "Personal", source: "Antigravity" },
    windows: [],
  },
  {
    id: "codex",
    displayName: "Codex",
    glyph: "✦",
    fidelity: "official",
    status: "ok",
    headlineId: "primary",
    fetchedAt: new Date().toISOString(),
    manageUrl: "https://chatgpt.com/#settings/Account",
    windows: [
      {
        id: "primary",
        label: "5h limit",
        usedFraction: 0.52,
        resetsAt: new Date(Date.now() + 2 * 60 * 60_000).toISOString(),
      },
      {
        id: "secondary",
        label: "Weekly limit",
        usedFraction: 0.13,
        resetsAt: new Date(Date.now() + 5 * 24 * 60 * 60_000).toISOString(),
      },
    ],
  },
];

export function runningInTauri(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export async function getSnapshots(): Promise<ProviderSnapshot[]> {
  if (!runningInTauri()) return structuredClone(demoSnapshots);
  return invoke<ProviderSnapshot[]>("get_snapshots");
}

export async function setEdge(edge: Edge, providerCount: number): Promise<void> {
  if (!runningInTauri()) return;
  await invoke("set_edge", { edge, providerCount });
}

export async function showTooltip(edge: Edge, index: number): Promise<void> {
  if (!runningInTauri()) return;
  await invoke("show_tooltip", { edge, index });
}

export async function hideTooltip(): Promise<void> {
  if (!runningInTauri()) return;
  await invoke("hide_window", { label: "tooltip" });
}

export async function openSettings(): Promise<void> {
  if (!runningInTauri()) return;
  await invoke("open_settings");
}

export async function showContextMenu(edge: Edge): Promise<void> {
  if (!runningInTauri()) return;
  await invoke("show_context_menu", { edge });
}

export async function hideWindow(label: string): Promise<void> {
  if (!runningInTauri()) return;
  await invoke("hide_window", { label });
}

export async function appAction(
  action: "refresh" | "hide-hour" | "quit" | "settings",
): Promise<void> {
  if (!runningInTauri()) return;
  await invoke("app_action", { action });
}

export async function openProvider(snapshot: ProviderSnapshot): Promise<void> {
  if (!snapshot.manageUrl) return;
  if (!runningInTauri()) {
    window.open(snapshot.manageUrl, "_blank", "noopener,noreferrer");
    return;
  }
  await invoke("open_url", { url: snapshot.manageUrl });
}
