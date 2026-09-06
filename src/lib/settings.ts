import type { ClientSettings, Edge } from "../types";
import { isSurface, isThemeMode } from "./theme";

const STORAGE_KEY = "codenotch-crossplatform.settings.v1";

/** Pre-color-picker theme names → their surfaces. */
const LEGACY_THEMES: Record<string, string> = {
  midnight: "#000000",
  graphite: "#16181d",
  abyss: "#0b1526",
  forest: "#0c1710",
  plum: "#170f1c",
};

export const DEFAULT_SETTINGS: ClientSettings = {
  edge: "right",
  enabledProviders: ["claude", "cursor", "codex", "gemini", "opencode"],
  mode: "system",
  surface: "#000000",
  opacity: 1,
  scale: 1,
  offsetX: 0,
  offsetY: 0,
  monitor: "primary",
  autoHide: false,
  autoHideDelaySec: 5,
};

function clampNumber(value: unknown, min: number, max: number, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value)
    ? Math.min(max, Math.max(min, value))
    : fallback;
}

export function loadSettings(): ClientSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_SETTINGS;
    const parsed = JSON.parse(raw) as Partial<ClientSettings> & { theme?: unknown };
    const edge = isEdge(parsed.edge) ? parsed.edge : DEFAULT_SETTINGS.edge;
    const enabledProviders = Array.isArray(parsed.enabledProviders)
      ? parsed.enabledProviders.filter((value): value is string => typeof value === "string")
      : DEFAULT_SETTINGS.enabledProviders;
    const mode = isThemeMode(parsed.mode) ? parsed.mode : DEFAULT_SETTINGS.mode;
    let surface = isSurface(parsed.surface) ? parsed.surface : DEFAULT_SETTINGS.surface;
    if (typeof parsed.theme === "string" && LEGACY_THEMES[parsed.theme]) {
      surface = LEGACY_THEMES[parsed.theme];
    }
    const opacity = clampNumber(parsed.opacity, 0, 1, DEFAULT_SETTINGS.opacity);
    const scale = clampNumber(parsed.scale, 0.7, 1.3, DEFAULT_SETTINGS.scale);
    const offsetX = clampNumber(parsed.offsetX, -200, 200, DEFAULT_SETTINGS.offsetX);
    const offsetY = clampNumber(parsed.offsetY, -200, 200, DEFAULT_SETTINGS.offsetY);
    const monitor = typeof parsed.monitor === "string" && parsed.monitor ? parsed.monitor : DEFAULT_SETTINGS.monitor;
    const autoHide = typeof parsed.autoHide === "boolean" ? parsed.autoHide : DEFAULT_SETTINGS.autoHide;
    const autoHideDelaySec = clampNumber(parsed.autoHideDelaySec, 1, 60, DEFAULT_SETTINGS.autoHideDelaySec);
    return {
      edge,
      enabledProviders,
      mode,
      surface,
      opacity,
      scale,
      offsetX,
      offsetY,
      monitor,
      autoHide,
      autoHideDelaySec,
    };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

export function saveSettings(settings: ClientSettings): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
}

function isEdge(value: unknown): value is Edge {
  return value === "right" || value === "left" || value === "top" || value === "bottom";
}
