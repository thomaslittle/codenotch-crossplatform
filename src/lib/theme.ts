import { useEffect, useState } from "react";
import type { CSSProperties } from "react";
import type { ThemeMode, ThemePresetId } from "../types";
import { usageBand } from "./usage";

export const DARK_SURFACE = "#000000";
export const LIGHT_SURFACE = "#eef0f4";

export interface ThemePreset {
  id: ThemePresetId;
  label: string;
  surface: string;
  description: string;
}

export const THEME_PRESETS: readonly ThemePreset[] = [
  { id: "custom", label: "Custom", surface: DARK_SURFACE, description: "Use the appearance controls below." },
  { id: "midnight", label: "Midnight", surface: "#000000", description: "Pure black and high contrast." },
  { id: "graphite", label: "Graphite", surface: "#16181d", description: "Neutral charcoal." },
  { id: "abyss", label: "Abyss", surface: "#0b1526", description: "Deep navy." },
  { id: "forest", label: "Forest", surface: "#0c1710", description: "Dark evergreen." },
  { id: "plum", label: "Plum", surface: "#170f1c", description: "Muted violet-black." },
] as const;

const HEX_RE = /^#[0-9a-fA-F]{6}$/;

export function isSurface(value: unknown): value is string {
  return typeof value === "string" && HEX_RE.test(value);
}

export function isThemeMode(value: unknown): value is ThemeMode {
  return value === "dark" || value === "light" || value === "system";
}

export function isThemePreset(value: unknown): value is ThemePresetId {
  return THEME_PRESETS.some((preset) => preset.id === value);
}

export function getThemePreset(id: ThemePresetId): ThemePreset {
  return THEME_PRESETS.find((preset) => preset.id === id) ?? THEME_PRESETS[0];
}

function channels(hex: string): [number, number, number] {
  const num = parseInt(hex.slice(1), 16);
  return [(num >> 16) & 255, (num >> 8) & 255, num & 255];
}

/** Relative luminance, 0 (black) – 1 (white). */
export function surfaceLuminance(hex: string): number {
  const [r, g, b] = channels(hex).map((c) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function mix(a: string, b: string, t: number): string {
  const ca = channels(a);
  const cb = channels(b);
  const cc = ca.map((v, i) => Math.round(v + (cb[i] - v) * t));
  return `#${cc.map((v) => v.toString(16).padStart(2, "0")).join("")}`;
}

/** Text color guaranteed readable on the surface. */
export function surfaceText(surface: string): string {
  return surfaceLuminance(surface) > 0.5 ? "#141518" : "#ffffff";
}

function surfaceBands(surface: string): { ok: string; warn: string; crit: string } {
  return surfaceText(surface) === "#ffffff"
    ? { ok: "#00ff88", warn: "#f2ff00", crit: "#ff3f00" }
    : { ok: "#00a35c", warn: "#9a8c00", crit: "#e63e00" };
}

/** Legacy usage-band color helper retained for status/diagnostic visuals. */
export function bandFor(surface: string, fraction: number): string {
  const bands = surfaceBands(surface);
  switch (usageBand(fraction)) {
    case "ample": return bands.ok;
    case "watch": return bands.warn;
    case "critical":
    case "exhausted": return bands.crit;
  }
}

/**
 * Quota gauges use one user-selected accent. Remaining quota is communicated
 * by fill length; the neutral track communicates the depleted portion.
 */
export function bandForRemaining(_surface: string, _fraction: number): string {
  return "var(--accent)";
}

/** Follows the OS light/dark preference; updates live on change. */
export function useSystemLight(): boolean {
  const [light, setLight] = useState(
    () => typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: light)").matches,
  );
  useEffect(() => {
    const query = window.matchMedia("(prefers-color-scheme: light)");
    const onChange = (event: MediaQueryListEvent) => setLight(event.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);
  return light;
}

/** Effective surface: named presets own their surface; custom keeps the existing mode behavior. */
export function resolveSurface(
  mode: ThemeMode,
  surface: string,
  systemLight: boolean,
  theme: ThemePresetId = "custom",
): string {
  if (theme !== "custom") return getThemePreset(theme).surface;
  if (mode === "system") return systemLight ? LIGHT_SURFACE : DARK_SURFACE;
  return isSurface(surface) ? surface : DARK_SURFACE;
}

/**
 * CSS variables consumed by styles.css. Surfaces are always solid here —
 * translucency comes from the element-level `opacity` style so the whole
 * notch (rings, glyphs, labels) fades together down to fully invisible.
 * Text auto-contrasts against the surface; the gauge accent is applied by
 * the persisted settings loader on each webview root.
 */
export function themeVars(surface: string): CSSProperties {
  const base = isSurface(surface) ? surface : DARK_SURFACE;
  const text = surfaceText(base);
  return {
    "--black": base,
    "--card": base,
    "--track": mix(base, text, 0.2),
    "--bar-track": mix(base, text, 0.18),
    "--settings-bg": base,
    "--text": text,
    "--subtle": mix(base, text, 0.55),
    "--faint": mix(base, text, 0.42),
    "--row-bg": mix(base, text, 0.055),
    "--row-border": mix(base, text, 0.13),
    "--avatar-bg": mix(base, text, 0.1),
    "--chip-bg": mix(base, text, 0.06),
    "--chip-border": mix(base, text, 0.15),
    "--chip-sel": mix(base, text, 0.12),
    "--icon-fg": mix(base, text, 0.62),
    "--icon-bg": mix(base, text, 0.06),
    "--icon-border": mix(base, text, 0.17),
  } as unknown as CSSProperties;
}
