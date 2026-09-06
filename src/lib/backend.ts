import { invoke } from "@tauri-apps/api/core";
import type { Edge, MonitorInfo, ProviderSnapshot } from "../types";

export function runningInTauri(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export async function getSnapshots(): Promise<ProviderSnapshot[]> {
  if (runningInTauri()) return invoke<ProviderSnapshot[]>("get_snapshots");
  // Browser dev mode: same real local readings as the Rust backend, served by
  // the Vite `/api/snapshots` bridge (see scripts/dev-snapshots.mjs). No demo
  // numbers — anything unreadable comes back with an honest status.
  const res = await fetch("/api/snapshots", { signal: AbortSignal.timeout(20_000) });
  if (!res.ok) throw new Error(`Snapshot bridge returned HTTP ${res.status}`);
  return (await res.json()) as ProviderSnapshot[];
}

export async function setEdge(
  edge: Edge,
  providerCount: number,
  scale: number,
  monitor: string,
  offsetX: number,
  offsetY: number,
): Promise<void> {
  if (!runningInTauri()) return;
  await invoke("set_edge", { edge, providerCount, scale, monitor, offsetX, offsetY });
}

export async function setBlur(enabled: boolean): Promise<void> {
  if (!runningInTauri()) return;
  await invoke("set_blur", { enabled }).catch(() => undefined);
}

export async function listMonitors(): Promise<MonitorInfo[]> {
  if (!runningInTauri()) {
    return [{ id: "primary", name: "Primary display", x: 0, y: 0, width: 1920, height: 1080, scaleFactor: 1, primary: true }];
  }
  return invoke<MonitorInfo[]>("list_monitors");
}

/** Resize the settings window to fit its measured content height. */
export async function fitSettings(height: number): Promise<void> {
  if (!runningInTauri()) return;
  await invoke("fit_settings", { height }).catch(() => undefined);
}

export async function showTooltip(edge: Edge, index: number, scale: number): Promise<void> {
  if (!runningInTauri()) return;
  try {
    await invoke("show_tooltip", { edge, index, scale });
  } catch (error) {
    console.error("[codenotch] show_tooltip failed:", error);
    throw error;
  }
}

export async function hideTooltip(): Promise<void> {
  if (!runningInTauri()) return;
  await invoke("hide_window", { label: "tooltip" });
}

/**
 * Is the OS cursor inside the notch or tooltip window? `null` means unknown
 * (caller keeps the plain timeout behavior).
 */
export async function cursorOverTooltipArea(): Promise<boolean | null> {
  if (!runningInTauri()) return null;
  try {
    return await invoke<boolean | null>("cursor_over_tooltip_area");
  } catch {
    return null;
  }
}

export async function openSettings(): Promise<void> {
  if (!runningInTauri()) return;
  await invoke("open_settings");
}

export async function showContextMenu(edge: Edge, scale: number): Promise<void> {
  if (!runningInTauri()) return;
  await invoke("show_context_menu", { edge, scale });
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
