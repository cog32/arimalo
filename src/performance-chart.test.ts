import { describe, it, expect } from "vitest";
import { rebaseToPercent } from "./performance-chart";

// The growth chart's percentage transform — pure, so it's unit-tested directly
// without loading ApexCharts or touching the DOM.
describe("rebaseToPercent", () => {
  it("rebases a series present at open to 0% at index 0", () => {
    const out = rebaseToPercent([5000, 4000, 4050]);
    expect(out[0]).toBe(0);
    expect(out[1]).toBeCloseTo(-20, 6); // (4000 − 5000) / 5000
    expect(out[2]).toBeCloseTo(-19, 6); // (4050 − 5000) / 5000
  });

  it("starts a mid-window entrant at 0% on its first non-zero point, with gaps before", () => {
    const out = rebaseToPercent([0, 1200, 2800]);
    expect(out[0]).toBeNull(); // didn't exist yet
    expect(out[1]).toBe(0); // effective open
    expect(out[2]).toBeCloseTo(133.333, 2); // (2800 − 1200) / 1200
  });

  it("returns all-null for an all-zero series", () => {
    expect(rebaseToPercent([0, 0, 0])).toEqual([null, null, null]);
  });

  it("treats an interior drop to zero (fully disposed) as −100%", () => {
    expect(rebaseToPercent([100, 0, 50])).toEqual([0, -100, -50]);
  });

  it("rebases a negative (liability/equity) series on its opening magnitude", () => {
    // A balance going −10k → −12k shrank by 20% of its opening size; the line
    // dips, consistent with assets (up = value grew, down = value fell).
    const out = rebaseToPercent([-10000, -12000]);
    expect(out[0]).toBe(0);
    expect(out[1]).toBeCloseTo(-20, 6);
  });

  it("clamps explosive growth so one line can't wreck the shared axis", () => {
    const out = rebaseToPercent([0.001, 1e10]);
    expect(out[0]).toBe(0);
    expect(out[1]).toBe(100000); // CAP
  });

  it("ignores sub-epsilon dust when choosing the anchor", () => {
    const out = rebaseToPercent([1e-9, 0, 100]);
    expect(out[0]).toBeNull();
    expect(out[1]).toBeNull();
    expect(out[2]).toBe(0);
  });

  it("preserves length and index alignment", () => {
    const values = [0, 0, 500, 750];
    expect(rebaseToPercent(values)).toHaveLength(values.length);
  });
});
