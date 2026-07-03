// Imperative ApexCharts lifecycle for the Performance report, isolated here so
// the chart library is (a) code-split via dynamic import — only loaded when the
// Performance view first opens — and (b) never imported by the vitest render
// surface (render.ts stays lib-free, so jsdom never instantiates SVG-heavy
// ApexCharts).
//
// The app's render loop applies HTML through morphdom, which preserves chart
// mounts across re-renders (they carry `data-morph-preserve`). So instances
// survive: each is created once, updated in place on data changes, and
// destroyed when the Performance view is left or there's nothing to plot. Two
// charts share this module: the value/cost-basis area chart
// (`syncPerformanceChart`) and the rebased growth-by-category line chart
// (`syncPerformanceGrowthChart`). Both are single entry points, called once at
// the tail of every render(); a shared `createChartController` factory owns the
// per-mount lifecycle so the bookkeeping isn't duplicated.

import type ApexChartsClass from "apexcharts";
import type { AccountValueSeries, PerformancePoint, PerformanceReport } from "./types";
import { formatMoney } from "./format";

// ApexCharts v5 ships `export = ApexCharts` with `ApexOptions` as a namespace
// member, so it's reached as `ApexChartsClass.ApexOptions` (not a named export).
type ApexOptions = ApexChartsClass.ApexOptions;

// Structural view of the instance/ctor — avoids the `export =` default-import
// friction with dynamic import() while keeping the methods we call typed.
interface ApexChartInstance {
  render(): Promise<void>;
  updateOptions(options: ApexOptions, redrawPaths?: boolean, animate?: boolean): Promise<void>;
  destroy(): void;
}
type ApexChartCtor = new (el: HTMLElement, options: ApexOptions) => ApexChartInstance;

// Shared library loader — ApexCharts is imported once, lazily, and reused by
// every chart controller.
let ctorCache: ApexChartCtor | null = null;
let loaderPromise: Promise<ApexChartCtor> | null = null;

function loadApex(): Promise<ApexChartCtor> {
  if (ctorCache) return Promise.resolve(ctorCache);
  if (!loaderPromise) {
    loaderPromise = import("apexcharts").then((mod) => {
      const m = mod as unknown as { default?: ApexChartCtor };
      const ctor = m.default ?? (mod as unknown as ApexChartCtor);
      ctorCache = ctor;
      return ctor;
    });
  }
  return loaderPromise;
}

type ChartSync = (report: PerformanceReport | null | undefined) => void;

/**
 * Build a single-mount chart lifecycle. Returns a `sync(report)` that is safe to
 * call on every render: it no-ops when the report object is unchanged, updates
 * in place when it changes, and tears the chart down when `report` is
 * null/undefined, the mount is gone (navigated away), or `hasData(report)` is
 * false. Fire-and-forget — never awaited.
 */
function createChartController(
  mountId: string,
  buildOptions: (report: PerformanceReport) => ApexOptions,
  hasData: (report: PerformanceReport) => boolean,
): ChartSync {
  let chart: ApexChartInstance | null = null;
  // The report the chart currently reflects — identity-compared to skip
  // redundant work, and used as a "teardown intent" flag (null) while the lib loads.
  let lastReport: PerformanceReport | null = null;

  return (report) => {
    if (typeof window === "undefined" || typeof document === "undefined") return;
    const el = document.getElementById(mountId);
    const existing = chart;

    // Teardown: navigated away (mount gone), no report, or nothing to plot.
    if (!report || !el || !hasData(report)) {
      if (existing) existing.destroy();
      chart = null;
      lastReport = null;
      return;
    }

    // Object identity — the report is only replaced when a fetch produces new data.
    if (existing && report === lastReport) return;

    if (existing) {
      // In-place update; trailing `false, false` suppress redraw animation so
      // refreshes don't re-animate.
      void existing.updateOptions(buildOptions(report), false, false);
      lastReport = report;
      return;
    }

    // First mount — load the lib (cached after first time), then create, using
    // the latest `lastReport` at resolve time. Recording it now stops concurrent
    // calls from double-creating; the resolver bails if we've since torn down.
    lastReport = report;
    void loadApex()
      .then((Ctor) => {
        const mount = document.getElementById(mountId);
        const intended = lastReport;
        if (!mount || intended === null || chart) return;
        const created = new Ctor(mount, buildOptions(intended));
        chart = created;
        void created.render();
      })
      .catch((err: unknown) => {
        lastReport = null;
        console.error(`Failed to load chart library for #${mountId}:`, err);
      });
  };
}

/** Reconcile the value/cost-basis area chart. See {@link createChartController}. */
export const syncPerformanceChart: ChartSync = createChartController(
  "performanceChart",
  buildOptions,
  (report) => report.points.length > 0,
);

/** Reconcile the rebased growth-by-category line chart. */
export const syncPerformanceGrowthChart: ChartSync = createChartController(
  "performanceGrowthChart",
  buildGrowthOptions,
  (report) => report.account_breakdown.length > 0,
);

function buildOptions(report: PerformanceReport): ApexOptions {
  const points = report.points;
  const value = points.map((p) => ({ x: p.date, y: round2(p.portfolio_value) }));
  const cost = points.map((p) => ({ x: p.date, y: round2(p.cost_basis) }));
  return {
    chart: {
      type: "area",
      height: 300,
      fontFamily:
        "ui-sans-serif, system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif",
      toolbar: { show: false },
      zoom: { enabled: false },
      animations: { enabled: true, speed: 260 },
      background: "transparent",
    },
    series: [
      { name: `Invested value (${report.base_currency})`, data: value },
      { name: "Cost basis", data: cost },
    ],
    colors: ["#4f46e5", "#9ca3af"],
    stroke: { curve: "smooth", width: [2.5, 1.5], dashArray: [0, 5] },
    fill: {
      type: ["gradient", "solid"],
      gradient: { shadeIntensity: 1, opacityFrom: 0.18, opacityTo: 0, stops: [0, 100] },
      opacity: [1, 0],
    },
    dataLabels: { enabled: false },
    grid: { borderColor: "#eef2f7", strokeDashArray: 3, padding: { left: 8, right: 8 } },
    xaxis: {
      type: "datetime",
      labels: { style: { colors: "#6b7280", fontSize: "12px" } },
      axisBorder: { show: false },
      axisTicks: { show: false },
      crosshairs: { stroke: { color: "#c7d2fe", width: 1, dashArray: 0 } },
      tooltip: { enabled: false },
    },
    yaxis: {
      labels: {
        style: { colors: "#6b7280", fontSize: "12px" },
        formatter: (v: number) => abbreviate(v),
      },
    },
    legend: { show: true, position: "bottom", labels: { colors: "#6b7280" } },
    tooltip: {
      shared: true,
      custom: ({ dataPointIndex }: { dataPointIndex: number }) =>
        tooltipHtml(points[dataPointIndex]),
    },
  };
}

// Deliberate categorical palette, anchored on the area chart's indigo so the two
// charts read as siblings. Assigned by the backend's stable sort order (largest
// current category first), so colours stay put across re-renders.
const GROWTH_COLORS = [
  "#4f46e5",
  "#0d9488",
  "#d97706",
  "#e11d48",
  "#7c3aed",
  "#059669",
  "#0284c7",
  "#64748b",
];
// Below EPS a value is treated as "no position" (can't rebase off dust); CAP
// keeps one explosive line from wrecking the shared percentage axis.
const REBASE_EPS = 1e-6;
const REBASE_CAP = 1e5;

/**
 * Rebase a raw per-snapshot value series to cumulative % change from its first
 * non-zero point (the line's effective open): every line starts at 0% so
 * differently-sized categories compare on one axis.
 *
 * `null` is emitted for snapshots before the first non-zero value (a gap — the
 * category didn't exist yet), so a mid-window entrant joins at 0% when it
 * appears. An interior drop to zero after the anchor yields −100% ("lost all
 * value"). Negative-base series (liabilities/equity) rebase on magnitude and are
 * well-defined while the sign is stable.
 */
export function rebaseToPercent(values: number[]): (number | null)[] {
  const anchor = values.findIndex((v) => Math.abs(v) >= REBASE_EPS);
  if (anchor < 0) return values.map(() => null);
  const base = Math.abs(values[anchor]);
  return values.map((v, i) => {
    if (i < anchor) return null;
    const pct = ((v - values[anchor]) / base) * 100;
    return Math.max(-REBASE_CAP, Math.min(REBASE_CAP, pct));
  });
}

function buildGrowthOptions(report: PerformanceReport): ApexOptions {
  const dates = report.points.map((p) => p.date);
  const series = report.account_breakdown.map((s: AccountValueSeries) => ({
    name: shortLabel(s.account),
    data: rebaseToPercent(s.values).map((y, i) => ({ x: dates[i], y })),
  }));
  return {
    chart: {
      type: "line",
      height: 320,
      fontFamily:
        "ui-sans-serif, system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif",
      toolbar: { show: false },
      zoom: { enabled: false },
      animations: { enabled: !prefersReducedMotion(), speed: 260 },
      background: "transparent",
    },
    series,
    colors: GROWTH_COLORS,
    stroke: { curve: "smooth", width: 2 },
    markers: { size: 0, hover: { size: 4 } },
    dataLabels: { enabled: false },
    grid: { borderColor: "#eef2f7", strokeDashArray: 3, padding: { left: 8, right: 8 } },
    xaxis: {
      type: "datetime",
      labels: { style: { colors: "#6b7280", fontSize: "12px" } },
      axisBorder: { show: false },
      axisTicks: { show: false },
      crosshairs: { stroke: { color: "#c7d2fe", width: 1, dashArray: 0 } },
      tooltip: { enabled: false },
    },
    yaxis: {
      labels: {
        style: { colors: "#6b7280", fontSize: "12px" },
        formatter: (v: number) => `${v >= 0 ? "+" : ""}${v.toFixed(0)}%`,
      },
    },
    legend: { show: true, position: "bottom", labels: { colors: "#6b7280" } },
    // The signature element: a shared 0% datum every line departs from, making
    // "who grew faster" legible at a glance.
    annotations: {
      yaxis: [
        {
          y: 0,
          strokeDashArray: 4,
          borderColor: "#9ca3af",
          label: {
            text: "start",
            position: "left",
            style: { color: "#9ca3af", background: "transparent", fontSize: "11px" },
          },
        },
      ],
    },
    tooltip: {
      shared: true,
      custom: ({ dataPointIndex }: { dataPointIndex: number }) =>
        growthTooltipHtml(report, dataPointIndex),
    },
  };
}

function tooltipHtml(p: PerformancePoint | undefined): string {
  if (!p) return "";
  const gl = (v: number) => (v >= 0 ? "gain" : "loss");
  const row = (label: string, v: number, cls = "") =>
    `<div class="perfTip__row"><span class="perfTip__label">${label}</span><span class="perfTip__val ${cls}">${money(v)}</span></div>`;
  return `<div class="perfTip">
    <div class="perfTip__title">${escapeHtml(p.label)}</div>
    ${row("Value", p.portfolio_value)}
    ${row("Cost basis", p.cost_basis)}
    ${row("Unrealised", p.unrealised_gain, gl(p.unrealised_gain))}
    ${row("Realised", p.realised_gain, gl(p.realised_gain))}
    ${row("Income", p.income)}
  </div>`;
}

// Tooltip for the growth chart: each category's % change and its raw value at
// the hovered month (full account name as the label, so the short legend stays
// readable while the tooltip is unambiguous).
function growthTooltipHtml(report: PerformanceReport, idx: number): string {
  const p = report.points[idx];
  if (!p) return "";
  const rows = report.account_breakdown
    .map((s: AccountValueSeries) => {
      const pct = rebaseToPercent(s.values)[idx];
      const pctText = pct == null ? "—" : `${pct >= 0 ? "+" : ""}${pct.toFixed(1)}%`;
      const cls = pct == null ? "" : pct >= 0 ? "gain" : "loss";
      return `<div class="perfTip__row"><span class="perfTip__label">${escapeHtml(s.account)}</span><span class="perfTip__val ${cls}">${pctText} · ${money(s.values[idx])}</span></div>`;
    })
    .join("");
  return `<div class="perfTip"><div class="perfTip__title">${escapeHtml(p.label)}</div>${rows}</div>`;
}

// Leaf segment of an account path — the legend label (e.g. "crypto" from
// "assets:crypto"). Direct children at one scope level have distinct leaves.
function shortLabel(account: string): string {
  const segments = account.split(":");
  return segments[segments.length - 1] || account;
}

function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

function round2(v: number): number {
  return Math.round(v * 100) / 100;
}

function money(v: number): string {
  return formatMoney(v, 2);
}

function abbreviate(v: number): string {
  const abs = Math.abs(v);
  if (abs >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
  if (abs >= 1_000) return `${(v / 1_000).toFixed(1)}k`;
  return v.toLocaleString(undefined, { maximumFractionDigits: 0 });
}

function escapeHtml(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}
