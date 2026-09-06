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
      autoHide: false,
      autoHideDelaySec: 5,
    });
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

  it("does not coerce non-boolean autoHide values", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ autoHide: "true" }));
    expect(loadSettings().autoHide).toBe(false);
  });
});
