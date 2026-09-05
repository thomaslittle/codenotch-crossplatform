import { describe, expect, it } from "vitest";
import { formatPercent, headlineWindow, usageBand } from "./usage";
import type { ProviderSnapshot } from "../types";

describe("usage bands", () => {
  it("matches the notch thresholds", () => {
    expect(usageBand(0.21)).toBe("ample");
    expect(usageBand(0.52)).toBe("watch");
    expect(usageBand(0.73)).toBe("watch");
    expect(usageBand(0.8)).toBe("critical");
    expect(usageBand(1)).toBe("exhausted");
  });
});

describe("headline selection", () => {
  const base: ProviderSnapshot = {
    id: "x",
    displayName: "X",
    glyph: "✦",
    fidelity: "official",
    status: "ok",
    fetchedAt: new Date(0).toISOString(),
    windows: [
      { id: "short", label: "Short", usedFraction: 0.2 },
      { id: "long", label: "Long", usedFraction: 0.7 },
    ],
  };

  it("prefers the provider-declared headline", () => {
    expect(headlineWindow({ ...base, headlineId: "short" })?.id).toBe("short");
  });

  it("falls back to the most constrained window", () => {
    expect(headlineWindow(base)?.id).toBe("long");
  });
});

it("rounds percentages for compact display", () => {
  expect(formatPercent(0.734)).toBe("73%");
});
