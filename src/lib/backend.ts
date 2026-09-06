import { invoke } from "@tauri-apps/api/core";
import type { Edge, MonitorInfo, ProviderSnapshot, ShellStyle } from "../types";

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
  compact = false,
  shellStyle: ShellStyle = "tab",
): Promise<void> {
  if (!runningInTauri()) return;
  await throttledInvoke("set_edge", { edge, providerCount, scale, monitor, offsetX, offsetY, compact, shellStyle });
}

// Window placement invokes are throttled (leading + trailing): slider drags
// fire dozens per second and every window move costs a full repaint.
let edgeTimer: ReturnType<typeof setTimeout> | null = null;
let edgeLeadingAt = 0;
let edgePending: Record<string, unknown> | null = null;

function throttledInvoke(command: string, args: Record<string, unknown>): Promise<void> {
  const now = Date.now();
  edgePending = args;
  if (now - edgeLeadingAt > 64) {
    edgeLeadingAt = now;
    if (edgeTimer != null) {
      clearTimeout(edgeTimer);
      edgeTimer = null;
    }
    return invoke(command, edgePending).then(() => undefined);
  }
  if (edgeTimer == null) {
    edgeTimer = setTimeout(() => {
      edgeTimer = null;
      edgeLeadingAt = Date.now();
      const latest = edgePending;
      if (latest) void invoke(command, latest).catch(() => undefined);
    }, 64);
  }
  return Promise.resolve();
}

export async function listMonitors(): Promise<MonitorInfo[]> {
  if (!runningInTauri()) {
    return [{ id: "primary", name: "Primary display", x: 0, y: 0, width: 1920, height: 1080, scaleFactor: 1, primary: true }];
  }
  return invoke<MonitorInfo[]>("list_monitors");
}

/** Legacy no-op caller surface retained for compatibility with older views. */
export async function fitSettings(height: number): Promise<void> {
  if (!runningInTauri()) return;
  await invoke("fit_settings", { height }).catch(() => undefined);
}

export async function showTooltip(
  edge: Edge,
  index: number,
  scale: number,
  compact = false,
  shellStyle: ShellStyle = "tab",
): Promise<void> {
  if (!runningInTauri()) return;
  try {
    await invoke("show_tooltip", { edge, index, scale, compact, shellStyle });
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

export async function setNotchRetracted(retracted: boolean, edge: Edge): Promise<void> {
  if (!runningInTauri()) return;
  try {
    await invoke("set_notch_retracted", { retracted, edge });
  } catch (error) {
    console.error("[codenotch] set_notch_retracted failed:", error);
  }
}

/** Is the cursor over any auxiliary overlay that should keep the notch visible? */
export async function cursorOverOverlay(): Promise<boolean | null> {
  if (!runningInTauri()) return null;
  try {
    return await invoke<boolean | null>("cursor_over_overlay");
  } catch {
    return null;
  }
}

/** Whether this platform can reliably find the cursor for auto-hide peeking. */
export async function autohideSupported(): Promise<boolean> {
  if (!runningInTauri()) return false;
  try {
    return await invoke<boolean>("autohide_supported");
  } catch {
    return false;
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

export const REPO_URL = "https://github.com/thomaslittle/codenotch-crossplatform";

/** Temporary hover diagnostic (removed once the no-popover fault is found). */
export async function trace(msg: string): Promise<void> {
  if (!runningInTauri()) return;
  await invoke("trace", { msg }).catch(() => undefined);
}

export async function openExternal(url: string): Promise<void> {
  if (!runningInTauri()) {
    window.open(url, "_blank", "noopener,noreferrer");
    return;
  }
  await invoke("open_url", { url });
}

export async function openProvider(snapshot: ProviderSnapshot): Promise<void> {
  if (!snapshot.manageUrl) return;
  if (!runningInTauri()) {
    window.open(snapshot.manageUrl, "_blank", "noopener,noreferrer");
    return;
  }
  await invoke("open_url", { url: snapshot.manageUrl });
}
