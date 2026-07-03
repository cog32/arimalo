/**
 * Pure date helpers extracted from src/main.ts so they can be unit-tested.
 * All functions read the wall clock via `new Date()` — tests use vitest fake timers.
 */

export type DateFilter = "week" | "month" | "year" | "all";

export function nowYYYYMM(now: Date = new Date()): string {
  return `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, "0")}`;
}

export function nowYYYYMMDD(now: Date = new Date()): string {
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(
    now.getDate(),
  ).padStart(2, "0")}`;
}

/**
 * Financial-year date range, mirroring `fy_date_range` in reports.rs. For
 * `endMonth = 6` (default, Australian FY), FY 2026 → 2025-07-01 … 2026-06-30.
 */
export function fyDateRange(
  fy: number,
  endMonth = 6,
  endDay = 30,
): { start: string; end: string } {
  const startMonth = endMonth + 1;
  const [startYear, sm] =
    startMonth > 12 ? [fy, 1] : [fy - 1, startMonth];
  const pad = (n: number) => String(n).padStart(2, "0");
  return {
    start: `${startYear}-${pad(sm)}-01`,
    end: `${fy}-${pad(endMonth)}-${pad(endDay)}`,
  };
}

/**
 * Subtract `months` calendar months from an ISO date (YYYY-MM-DD), clamping the
 * day to the target month's length (e.g. minus 1 month from 2026-03-31 →
 * 2026-02-28). Used for the performance report's default 12-month lookback.
 */
export function minusMonths(iso: string, months: number): string {
  const [y, m, d] = iso.split("-").map(Number);
  const base = new Date(y, m - 1 - months, 1);
  const ty = base.getFullYear();
  const tm = base.getMonth(); // 0-based
  const lastDay = new Date(ty, tm + 1, 0).getDate();
  const day = Math.min(d, lastDay);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${ty}-${pad(tm + 1)}-${pad(day)}`;
}

/** Compute the ISO-date start of a relative date filter, or null for "all". */
export function dateFilterStart(filter: DateFilter, now: Date = new Date()): string | null {
  if (filter === "all") return null;
  let start: Date;
  switch (filter) {
    case "week": {
      const day = now.getDay();
      start = new Date(now.getFullYear(), now.getMonth(), now.getDate() - day);
      break;
    }
    case "month":
      start = new Date(now.getFullYear(), now.getMonth(), 1);
      break;
    case "year":
      start = new Date(now.getFullYear(), 0, 1);
      break;
  }
  return `${start.getFullYear()}-${String(start.getMonth() + 1).padStart(2, "0")}-${String(
    start.getDate(),
  ).padStart(2, "0")}`;
}
