import { describe, expect, it } from "vitest";
import { compareVersions } from "./updates";

describe("compareVersions", () => {
  it("orders plain versions", () => {
    expect(compareVersions("0.1.0", "0.2.0")).toBe(-1);
    expect(compareVersions("0.2.0", "0.1.0")).toBe(1);
    expect(compareVersions("1.10.0", "1.9.9")).toBe(1);
  });

  it("tolerates v prefixes", () => {
    expect(compareVersions("0.1.0", "v0.1.0")).toBe(0);
    expect(compareVersions("v0.1.0", "v0.2.0")).toBe(-1);
  });

  it("treats equal versions as equal", () => {
    expect(compareVersions("v0.2.0", "0.2.0")).toBe(0);
  });
});
