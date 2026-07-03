import { describe, it, expect } from "vitest";
import {
  calculateWindow,
  computeRenderWindow,
  shouldAttachVirtualScroll,
  spacerRow,
  TX_WINDOW,
  TX_BUFFER,
} from "./virtual-scroll";

describe("computeRenderWindow", () => {
  it("local mode: windows from txWindowStart", () => {
    // 2000 items, window starting at 300
    const result = computeRenderWindow(2000, 300, undefined, undefined);
    expect(result.sliceStart).toBe(300);
    expect(result.sliceEnd).toBe(300 + TX_WINDOW);
    expect(result.beforeCount).toBe(300);
    expect(result.afterCount).toBe(2000 - 300 - TX_WINDOW);
  });

  it("local mode: clamps window at end of items", () => {
    const result = computeRenderWindow(100, 0, undefined, undefined);
    expect(result.sliceStart).toBe(0);
    expect(result.sliceEnd).toBe(100);
    expect(result.beforeCount).toBe(0);
    expect(result.afterCount).toBe(0);
  });

  it("rust-paginated: renders all fetched items with global spacers", () => {
    // Rust returned 500 items starting at offset 1000, out of 5000 total
    const result = computeRenderWindow(500, 1150, 1000, 5000);
    expect(result.sliceStart).toBe(0);
    expect(result.sliceEnd).toBe(500);
    expect(result.beforeCount).toBe(1000);
    expect(result.afterCount).toBe(5000 - 1000 - 500);
  });

  it("rust-paginated: no after spacer at end of data", () => {
    // Last page: offset 4500, 500 items, total 5000
    const result = computeRenderWindow(500, 4600, 4500, 5000);
    expect(result.sliceStart).toBe(0);
    expect(result.sliceEnd).toBe(500);
    expect(result.beforeCount).toBe(4500);
    expect(result.afterCount).toBe(0);
  });

  it("rust-paginated: first page has no before spacer", () => {
    const result = computeRenderWindow(500, 0, 0, 5000);
    expect(result.sliceStart).toBe(0);
    expect(result.sliceEnd).toBe(500);
    expect(result.beforeCount).toBe(0);
    expect(result.afterCount).toBe(4500);
  });
});

describe("shouldAttachVirtualScroll", () => {
  it("attaches when rust total exceeds TX_WINDOW", () => {
    // vsItems.length is 500 (= TX_WINDOW), but rustTotal is 5000
    expect(shouldAttachVirtualScroll(TX_WINDOW, 5000)).toBe(true);
  });

  it("does not attach when rust total fits in one window", () => {
    expect(shouldAttachVirtualScroll(200, 200)).toBe(false);
  });

  it("falls back to item count when no rust total", () => {
    expect(shouldAttachVirtualScroll(TX_WINDOW + 1, undefined)).toBe(true);
    expect(shouldAttachVirtualScroll(TX_WINDOW, undefined)).toBe(false);
  });

  it("does not attach when rust-paginated items fit but total is small", () => {
    // 300 items, total 300 — no scroll needed
    expect(shouldAttachVirtualScroll(300, 300)).toBe(false);
  });
});

describe("spacerRow", () => {
  it("produces valid HTML for non-zero counts", () => {
    const html = spacerRow(100, 7);
    expect(html).toContain("height:");
    expect(html).toContain('colspan="7"');
    expect(html).toContain("txTable__spacer");
  });

  it("returns empty string for zero items", () => {
    expect(spacerRow(0, 7)).toBe("");
  });

  it("returns empty string for negative items", () => {
    expect(spacerRow(-5, 7)).toBe("");
  });
});

describe("calculateWindow", () => {
  it("positions window around scroll position", () => {
    // Scrolled 5000px past table, each row ~48px → visible start ~104
    const range = calculateWindow(5000, 0, 10000, 800);
    expect(range.start).toBeGreaterThanOrEqual(0);
    expect(range.end).toBeLessThanOrEqual(10000);
    expect(range.end - range.start).toBeLessThanOrEqual(TX_WINDOW);
  });
});
