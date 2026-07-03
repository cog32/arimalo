import { describe, expect, it } from "vitest";
import { dateFilterStart, nowYYYYMM, nowYYYYMMDD } from "./date-helpers";

describe("nowYYYYMM", () => {
  it("formats year+month with zero-padding", () => {
    expect(nowYYYYMM(new Date(2026, 2, 13))).toBe("202603");
  });
  it("pads single-digit months", () => {
    expect(nowYYYYMM(new Date(2026, 0, 1))).toBe("202601");
  });
});

describe("nowYYYYMMDD", () => {
  it("formats ISO date with zero-padding", () => {
    expect(nowYYYYMMDD(new Date(2026, 0, 5))).toBe("2026-01-05");
  });
});

describe("dateFilterStart", () => {
  // 2026-05-13 was a Wednesday (getDay() === 3).
  const wed = new Date(2026, 4, 13);

  it("returns null for 'all'", () => {
    expect(dateFilterStart("all", wed)).toBeNull();
  });
  it("returns Sunday-of-this-week for 'week'", () => {
    // Wed 2026-05-13 → Sun 2026-05-10.
    expect(dateFilterStart("week", wed)).toBe("2026-05-10");
  });
  it("returns first-of-month for 'month'", () => {
    expect(dateFilterStart("month", wed)).toBe("2026-05-01");
  });
  it("returns Jan 1 of current year for 'year'", () => {
    expect(dateFilterStart("year", wed)).toBe("2026-01-01");
  });
});
