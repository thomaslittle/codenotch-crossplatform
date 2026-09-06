import { beforeEach, describe, expect, it } from "vitest";
import { DEFAULT_SETTINGS, loadSettings } from "./settings";

const STORAGE_KEY = "codenotch-crossplatform.settings.v1";

class MemoryStorage implements Storage {
  private readonly values = new Map<string, string>();

  get length(): number {
    return this.values.size;
  }

  clear(): void {
    this.values.clear();
  }

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  key(index: number): string | null {
    return Array.from(this.values.keys())[index] ?? null;
  }

  removeItem(key: string): void {
    this.values.delete(key);
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }
}

Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: new MemoryStorage(),
});

beforeEach(() => {
  localStorage.clear();
});

describe("settings", () => {
  it("uses defaults when storage is empty", () => {
    expect(loadSettings()).toMatchObject({
      theme: DEFAULT_SETTINGS.theme,
      shellStyle: DEFAULT_SETTINGS.shellStyle,
      gaugeStyle: DEFAULT_SETTINGS.gaugeStyle,
      accent: DEFAULT_SETTINGS.accent,
      autoHide: DEFAULT_SETTINGS.autoHide,
      autoHideDelaySec: DEFAULT_SETTINGS.autoHideDelaySec,
    });
  });

  it("clamps the idle delay to 1–60 seconds", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ autoHideDelaySec: 0 }));
    expect(loadSettings().autoHideDelaySec).toBe(1);

    localStorage.setItem(STORAGE_KEY, JSON.stringify({ autoHideDelaySec: 120 }));
    expect(loadSettings().autoHideDelaySec).toBe(60);
  });

  it("fills new defaults for legacy payloads", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ edge: "left", scale: 0.9 }));
    expect(loadSettings()).toMatchObject({
      edge: "left",
      scale: 0.9,
      theme: "custom",
      shellStyle: "tab",
      gaugeStyle: "classic",
      accent: DEFAULT_SETTINGS.accent,
      autoHide: false,
      autoHideDelaySec: 5,
    });
  });

  it("persists a valid accent and rejects malformed values", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ accent: "#ff66cc" }));
    expect(loadSettings().accent).toBe("#ff66cc");

    localStorage.setItem(STORAGE_KEY, JSON.stringify({ accent: "hotpink" }));
    expect(loadSettings().accent).toBe(DEFAULT_SETTINGS.accent);
  });

  it("restores legacy named theme surfaces", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ theme: "abyss", surface: "#ffffff" }));
    expect(loadSettings()).toMatchObject({
      theme: "abyss",
      surface: "#0b1526",
    });
  });

  it("falls back from unknown theme ids", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ theme: "unknown" }));
    expect(loadSettings().theme).toBe("custom");
  });

  it("persists every supported shell style and rejects unknown ones", () => {
    const shellStyles = ["tab", "bubble", "sharp", "trapezoid", "pill", "dock", "dock3d", "ghost"] as const;
    for (const shellStyle of shellStyles) {
      localStorage.setItem(STORAGE_KEY, JSON.stringify({ shellStyle }));
      expect(loadSettings().shellStyle).toBe(shellStyle);
    }

    localStorage.setItem(STORAGE_KEY, JSON.stringify({ shellStyle: "glass" }));
    expect(loadSettings().shellStyle).toBe("tab");
  });

  it("migrates the retired rail shell to glass dock", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ shellStyle: "rail" }));
    expect(loadSettings().shellStyle).toBe("dock");
  });

  it("persists every supported gauge style and rejects unknown ones", () => {
    for (const gaugeStyle of ["classic", "slim", "halo", "stacked", "columns", "micro"] as const) {
      localStorage.setItem(STORAGE_KEY, JSON.stringify({ gaugeStyle }));
      expect(loadSettings().gaugeStyle).toBe(gaugeStyle);
    }

    localStorage.setItem(STORAGE_KEY, JSON.stringify({ gaugeStyle: "neon" }));
    expect(loadSettings().gaugeStyle).toBe("classic");
  });

  it("does not coerce non-boolean autoHide values", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ autoHide: "true" }));
    expect(loadSettings().autoHide).toBe(false);
  });
});
