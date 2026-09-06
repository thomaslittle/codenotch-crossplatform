import { useEffect, useState } from "react";
import type { CSSProperties } from "react";
import type { ThemeMode } from "../types";
import { usageBand } from "./usage";

export const DARK_SURFACE = "#000000";
export const LIGHT_SURFACE = "#eef0f4";

const HEX_RE = /^#[0-9a-fA-F]{6}$/;

export function isSurface(value: unknown): value is string {
  return typeof value === "string" && HEX_RE.test(value);
}

export function isThemeMode(value: unknown): value is ThemeMode {
  return value === "dark" || value === "light" || value === "system";
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

/** Usage band color that stays readable on the given surface. */
export function bandFor(surface: string, fraction: number): string {
  const bands = surfaceBands(surface);
  switch (usageBand(fraction)) {
    case "ample": return bands.ok;
    case "watch": return bands.warn;
    case "critical":
    case "exhausted": return bands.crit;
  }
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

/** Effective surface: system mode uses OS defaults, otherwise the custom color. */
export function resolveSurface(mode: ThemeMode, surface: string, systemLight: boolean): string {
  if (mode === "system") return systemLight ? LIGHT_SURFACE : DARK_SURFACE;
  return isSurface(surface) ? surface : DARK_SURFACE;
}

/**
 * CSS variables consumed by styles.css. Surfaces are always solid here —
 * translucency comes from the element-level `opacity` style so the whole
 * notch (rings, glyphs, labels) fades together down to fully invisible.
 * Text and bands auto-contrast against the surface so any picker color
 * stays readable.
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
