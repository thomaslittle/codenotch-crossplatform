import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderSnapshot } from "../types";
import { clearUsageResetAlerts, getUsageResetAlerts, processUsageResetSnapshots } from "./usageAlerts";

class MemoryStorage implements Storage {
  private readonly values = new Map<string, string>();
  get length(): number { return this.values.size; }
  clear(): void { this.values.clear(); }
  getItem(key: string): string | null { return this.values.get(key) ?? null; }
  key(index: number): string | null { return Array.from(this.values.keys())[index] ?? null; }
  removeItem(key: string): void { this.values.delete(key); }
  setItem(key: string, value: string): void { this.values.set(key, value); }
}

Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: new MemoryStorage(),
});

function snapshot(usedFraction: number, resetsAt: string): ProviderSnapshot {
  return {
    id: "codex",
    displayName: "Codex",
    glyph: "C",
    fidelity: "official",
    status: "ok",
    windows: [{ id: "five-hour", label: "5h limit", usedFraction, resetsAt }],
    fetchedAt: "2026-09-06T12:00:00Z",
  };
}

beforeEach(() => {
  localStorage.clear();
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-09-06T12:00:00Z"));
});

afterEach(() => vi.useRealTimers());

describe("usage reset alerts", () => {
  it("does not alert on the first snapshot", () => {
    expect(processUsageResetSnapshots([snapshot(0.82, "2026-09-06T18:00:00Z")])).toEqual([]);
  });

  it("alerts when reset time advances and used usage materially drops", () => {
    processUsageResetSnapshots([snapshot(0.82, "2026-09-06T18:00:00Z")]);
    const alerts = processUsageResetSnapshots([snapshot(0.03, "2026-09-06T23:00:00Z")]);
    expect(alerts).toHaveLength(1);
    expect(alerts[0]).toMatchObject({
      providerId: "codex",
      windowId: "five-hour",
      previousUsedFraction: 0.82,
      currentUsedFraction: 0.03,
    });
  });

  it("does not alert when only usage drops", () => {
    processUsageResetSnapshots([snapshot(0.82, "2026-09-06T18:00:00Z")]);
    expect(processUsageResetSnapshots([snapshot(0.03, "2026-09-06T18:00:00Z")])).toEqual([]);
  });

  it("does not alert for tiny changes even if reset time advances", () => {
    processUsageResetSnapshots([snapshot(0.42, "2026-09-06T18:00:00Z")]);
    expect(processUsageResetSnapshots([snapshot(0.39, "2026-09-06T23:00:00Z")])).toEqual([]);
  });

  it("does not duplicate the same reset event", () => {
    processUsageResetSnapshots([snapshot(0.82, "2026-09-06T18:00:00Z")]);
    processUsageResetSnapshots([snapshot(0.03, "2026-09-06T23:00:00Z")]);
    processUsageResetSnapshots([snapshot(0.03, "2026-09-06T23:00:00Z")]);
    expect(getUsageResetAlerts()).toHaveLength(1);
  });

  it("keeps a recent reset notification while refreshed quota is still meaningfully available", () => {
    processUsageResetSnapshots([snapshot(0.82, "2026-09-06T18:00:00Z")]);
    processUsageResetSnapshots([snapshot(0.03, "2026-09-06T23:00:00Z")]);
    expect(processUsageResetSnapshots([snapshot(0.50, "2026-09-06T23:00:00Z")])).toHaveLength(1);
  });

  it("expires a reset notification once the refreshed quota has been consumed again", () => {
    processUsageResetSnapshots([snapshot(0.82, "2026-09-06T18:00:00Z")]);
    processUsageResetSnapshots([snapshot(0.03, "2026-09-06T23:00:00Z")]);
    expect(processUsageResetSnapshots([snapshot(0.80, "2026-09-06T23:00:00Z")])).toEqual([]);
    expect(getUsageResetAlerts()).toEqual([]);
  });

  it("expires an unacknowledged reset notification after fifteen minutes", () => {
    processUsageResetSnapshots([snapshot(0.82, "2026-09-06T18:00:00Z")]);
    processUsageResetSnapshots([snapshot(0.03, "2026-09-06T23:00:00Z")]);
    expect(getUsageResetAlerts()).toHaveLength(1);

    vi.advanceTimersByTime(15 * 60_000 + 1);

    expect(getUsageResetAlerts()).toEqual([]);
  });

  it("clears alerts for one provider", () => {
    processUsageResetSnapshots([snapshot(0.82, "2026-09-06T18:00:00Z")]);
    processUsageResetSnapshots([snapshot(0.03, "2026-09-06T23:00:00Z")]);
    expect(clearUsageResetAlerts("codex")).toEqual([]);
    expect(getUsageResetAlerts()).toEqual([]);
  });
});
