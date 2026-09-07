import type { ClientSettings, Edge, GaugeStyle, ShellStyle } from "../types";
import { getThemePreset, isSurface, isThemeMode, isThemePreset } from "./theme";

const STORAGE_KEY = "codenotch-crossplatform.settings.v1";

export const DEFAULT_SETTINGS: ClientSettings = {
  edge: "right",
  enabledProviders: ["claude", "cursor", "codex", "gemini", "opencode"],
  theme: "custom",
  shellStyle: "tab",
  gaugeStyle: "classic",
  mode: "system",
  surface: "#000000",
  accent: "#4da3ff",
  shellBackgroundOpacity: 0.82,
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

function applyAppearance(
  shellStyle: ShellStyle,
  gaugeStyle: GaugeStyle,
  accent: string,
  shellBackgroundOpacity: number,
): void {
  if (typeof document !== "undefined") {
    document.documentElement.dataset.shell = shellStyle;
    document.documentElement.dataset.gauge = gaugeStyle;
    document.documentElement.style.setProperty("--accent", accent);
    document.documentElement.style.setProperty("--shell-bg-opacity", String(shellBackgroundOpacity));
  }
}

export function loadSettings(): ClientSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) {
      applyAppearance(
        DEFAULT_SETTINGS.shellStyle,
        DEFAULT_SETTINGS.gaugeStyle,
        DEFAULT_SETTINGS.accent,
        DEFAULT_SETTINGS.shellBackgroundOpacity,
      );
      return DEFAULT_SETTINGS;
    }
    const parsed = JSON.parse(raw) as Partial<ClientSettings>;
    const edge = isEdge(parsed.edge) ? parsed.edge : DEFAULT_SETTINGS.edge;
    const enabledProviders = Array.isArray(parsed.enabledProviders)
      ? parsed.enabledProviders.filter((value): value is string => typeof value === "string")
      : DEFAULT_SETTINGS.enabledProviders;
    const theme = isThemePreset(parsed.theme) ? parsed.theme : DEFAULT_SETTINGS.theme;
    // Rail never developed into a strong visual design. Keep it readable as a
    // legacy stored value, but migrate it to the supported Glass Dock shell.
    const shellStyle = parsed.shellStyle === "rail"
      ? "dock"
      : isShellStyle(parsed.shellStyle)
        ? parsed.shellStyle
        : DEFAULT_SETTINGS.shellStyle;
    const gaugeStyle = isGaugeStyle(parsed.gaugeStyle) ? parsed.gaugeStyle : DEFAULT_SETTINGS.gaugeStyle;
    const mode = isThemeMode(parsed.mode) ? parsed.mode : DEFAULT_SETTINGS.mode;
    let surface = isSurface(parsed.surface) ? parsed.surface : DEFAULT_SETTINGS.surface;
    if (theme !== "custom") {
      surface = getThemePreset(theme).surface;
    }
    const accent = isSurface(parsed.accent) ? parsed.accent : DEFAULT_SETTINGS.accent;
    const shellBackgroundOpacity = clampNumber(
      parsed.shellBackgroundOpacity,
      0,
      1,
      DEFAULT_SETTINGS.shellBackgroundOpacity,
    );
    const opacity = clampNumber(parsed.opacity, 0, 1, DEFAULT_SETTINGS.opacity);
    const scale = clampNumber(parsed.scale, 0.7, 1.3, DEFAULT_SETTINGS.scale);
    const offsetX = clampNumber(parsed.offsetX, -200, 200, DEFAULT_SETTINGS.offsetX);
    const offsetY = clampNumber(parsed.offsetY, -200, 200, DEFAULT_SETTINGS.offsetY);
    const monitor = typeof parsed.monitor === "string" && parsed.monitor ? parsed.monitor : DEFAULT_SETTINGS.monitor;
    const autoHide = typeof parsed.autoHide === "boolean" ? parsed.autoHide : DEFAULT_SETTINGS.autoHide;
    const autoHideDelaySec = clampNumber(parsed.autoHideDelaySec, 1, 60, DEFAULT_SETTINGS.autoHideDelaySec);
    const settings: ClientSettings = {
      edge,
      enabledProviders,
      theme,
      shellStyle,
      gaugeStyle,
      mode,
      surface,
      accent,
      shellBackgroundOpacity,
      opacity,
      scale,
      offsetX,
      offsetY,
      monitor,
      autoHide,
      autoHideDelaySec,
    };
    applyAppearance(shellStyle, gaugeStyle, accent, shellBackgroundOpacity);
    return settings;
  } catch {
    applyAppearance(
      DEFAULT_SETTINGS.shellStyle,
      DEFAULT_SETTINGS.gaugeStyle,
      DEFAULT_SETTINGS.accent,
      DEFAULT_SETTINGS.shellBackgroundOpacity,
    );
    return DEFAULT_SETTINGS;
  }
}

export function saveSettings(settings: ClientSettings): void {
  applyAppearance(settings.shellStyle, settings.gaugeStyle, settings.accent, settings.shellBackgroundOpacity);
  localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
}

function isEdge(value: unknown): value is Edge {
  return value === "right" || value === "left" || value === "top" || value === "bottom";
}

function isShellStyle(value: unknown): value is ShellStyle {
  const styles: readonly ShellStyle[] = ["tab", "bubble", "sharp", "trapezoid", "pill", "dock", "dock3d", "ghost"];
  return typeof value === "string" && styles.includes(value as ShellStyle);
}

function isGaugeStyle(value: unknown): value is GaugeStyle {
  return value === "classic"
    || value === "slim"
    || value === "halo"
    || value === "stacked"
    || value === "columns"
    || value === "micro";
}
