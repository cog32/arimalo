// Render helper functions extracted for testability.
// These are pure functions that take state and return HTML strings.

import { shortAccountPath } from "./account-utils";
import { formatMoney, formatMoneyWhole } from "./format";
import type {
  BalancesReport,
  BalancesSortColumn,
  CgtEvent,
  CgtReport,
  CgtSortColumn,
  CoinBalance,
  GenericSortState,
  IncomeEvent,
  IncomeSortColumn,
  IncomeTaxReport,
  LossHarvestReport,
  PerformanceReport,
  TaxConfig,
} from "./types";

const CURRENCY_DECIMALS = 2;
const QUANTITY_DECIMALS = 4;
/** Money in detail/table cells: 2 decimals + thousand separators. Summary /
 *  header figures use formatMoneyWhole (no decimals). */
const money = (v: number) => formatMoney(v, CURRENCY_DECIMALS);

export function renderSortHeader<C extends string>(
  label: string, column: C, sort?: GenericSortState<C>,
  extraClass?: string, scope?: string,
): string {
  const active = sort?.column === column;
  const arrow = active
    ? sort.direction === "asc" ? " \u25B2" : " \u25BC"
    : "";
  const cls = [extraClass, "sortable", active ? "sortable--active" : ""].filter(Boolean).join(" ");
  const scopeAttr = scope ? ` data-sort-scope="${scope}"` : "";
  return `<th class="${cls}" data-sort-col="${column}"${scopeAttr}>${label}${arrow}</th>`;
}

export function renderNavButtons(state: ReportState): string {
  return `<button class="navBtn" id="navBack" ${state.navCanGoBack ? "" : "disabled"} title="Back (Alt+Left)">\u2039</button><button class="navBtn" id="navForward" ${state.navCanGoForward ? "" : "disabled"} title="Forward (Alt+Right)">\u203A</button>`;
}

export function renderRebuildStrip(pendingMutations: number): string {
  const active = pendingMutations > 0;
  const cls = active ? "rebuildStrip rebuildStrip--active" : "rebuildStrip";
  return `<div class="${cls}" data-testid="rebuild-strip" aria-hidden="${!active}"></div>`;
}

export function withFadeInClass(
  baseClasses: string,
  txnId: string,
  justAddedTxnIds?: Set<string>,
): string {
  return justAddedTxnIds?.has(txnId) ? `${baseClasses} txRow--entering` : baseClasses;
}

export function escapeText(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

const MONTH_ABBR = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];

export function fyLabel(fy: string, taxConfig?: TaxConfig): string {
  const endMonth = taxConfig?.financial_year_end_month ?? 6;
  const startMonth = endMonth >= 12 ? 1 : endMonth + 1;
  const fyNum = parseInt(fy);
  const startYear = endMonth >= 12 ? fyNum : fyNum - 1;
  return `FY ${fy} (${MONTH_ABBR[startMonth - 1]} ${startYear} - ${MONTH_ABBR[endMonth - 1]} ${fyNum})`;
}

export type ReportState = {
  selectedReport?: "cgt" | "income" | "balances" | "performance" | "loss_harvest";
  selectedReportYear?: number;
  reportMarkdown?: string;
  reportYears?: number[];
  cgtReport?: CgtReport;
  incomeReport?: IncomeTaxReport;
  balancesReport?: BalancesReport;
  performanceReport?: PerformanceReport;
  lossHarvestReport?: LossHarvestReport;
  taxConfig?: TaxConfig;
  taxSettingsOpen?: boolean;
  reportsBuilding?: boolean;
  reportDateMode?: "fy" | "custom";
  reportDateFrom?: string;
  reportDateTo?: string;
  reportBaseScope?: string;
  sidebarView: "accounts" | "categories" | "reports" | "rule-editor" | "plugins";
  reportPageSize?: number;
  cgtSort?: GenericSortState<CgtSortColumn>;
  cgtFilterText?: string;
  cgtExpandedGroups?: Set<string>;
  incomeSort?: GenericSortState<IncomeSortColumn>;
  incomeFilterText?: string;
  incomeExpandedGroups?: Set<string>;
  balancesSort?: GenericSortState<BalancesSortColumn>;
  balancesFilterText?: string;
  balancesExpanded?: Set<string>;
  lossHarvestExpanded?: Set<string>;
  lossHarvestView?: "position" | "parcel";
  navCanGoBack?: boolean;
  navCanGoForward?: boolean;
  availableReportAccounts?: { income: string[]; expenses: string[] };
  // Global Settings modal (extra primary-account prefixes editor).
  globalSettingsOpen?: boolean;
  globalSettingsDraft?: string[];
  extraPrimaryAccountPrefixes?: string[];
  allAccounts?: string[];
};

/**
 * The financial year (labelled by its end calendar year) that `now` falls in,
 * given the configured FY-end. With a 30 June end: 2026-06-21 → 2026,
 * 2026-09-01 → 2027. Defaults to a 30 June end when no tax config is supplied.
 */
export function currentFinancialYear(taxConfig?: TaxConfig, now: Date = new Date()): number {
  const endMonth = taxConfig?.financial_year_end_month ?? 6;
  const endDay = taxConfig?.financial_year_end_day ?? 30;
  const y = now.getFullYear();
  const m = now.getMonth() + 1;
  const d = now.getDate();
  return m > endMonth || (m === endMonth && d > endDay) ? y + 1 : y;
}

export function fyYearOptions(
  selectedYear?: number,
  availableYears?: number[],
  taxConfig?: TaxConfig,
): string {
  const currentFy = currentFinancialYear(taxConfig);
  const selected = selectedYear ?? currentFy;
  const base = availableYears && availableYears.length > 0
    ? availableYears
    : Array.from({ length: 11 }, (_, i) => currentFy - i);
  // Always offer the current FY (and any explicitly-selected year) even when no
  // cached report exists for it yet — otherwise the current year is unreachable.
  const years = [...new Set([...base, currentFy, ...(selectedYear ? [selectedYear] : [])])]
    .sort((a, b) => b - a);
  return years.map((y) =>
    `<option value="${y}" ${y === selected ? "selected" : ""}>FY ${y}</option>`
  ).join("");
}

/// Tiny download-arrow icon button that triggers a CSV export of the current
/// report. The click handler in main.ts reads `data-export-csv` to know which
/// report type to convert.
export function renderExportCsvButton(reportType: "cgt" | "income" | "balances"): string {
  return `<button class="reportHeader__export btn btn--icon" type="button" title="Export as CSV" data-export-csv="${reportType}" aria-label="Export as CSV">
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M8 2v9" />
      <path d="M4 7l4 4 4-4" />
      <path d="M3 13h10" />
    </svg>
  </button>`;
}

export function renderDateControls(state: ReportState, disabled?: boolean): string {
  const isCustom = state.reportDateMode === "custom";
  return `<div class="reportDateControls">
    <div class="reportDateControls__toggle">
      <button class="reportDateToggle ${!isCustom ? "reportDateToggle--active" : ""}" data-date-mode="fy" ${disabled ? "disabled" : ""}>FY</button>
      <button class="reportDateToggle ${isCustom ? "reportDateToggle--active" : ""}" data-date-mode="custom" ${disabled ? "disabled" : ""}>Custom</button>
    </div>
    ${isCustom
      ? `<input type="date" id="reportDateFrom" class="reportDateInput" value="${state.reportDateFrom ?? ""}" ${disabled ? "disabled" : ""} />
         <span class="reportDateSep">\u2013</span>
         <input type="date" id="reportDateTo" class="reportDateInput" value="${state.reportDateTo ?? ""}" ${disabled ? "disabled" : ""} />
         <button id="reportDateApply" class="btn btn--small" ${disabled ? "disabled" : ""}>Go</button>`
      : `<select id="reportYearSelect" class="reportYearSelect" ${disabled ? "disabled" : ""}>${fyYearOptions(state.selectedReportYear, state.reportYears, state.taxConfig)}</select>`}
    <button id="taxSettingsBtn" class="btn btn--secondary" type="button" title="Tax Settings" ${disabled ? "disabled" : ""}>Settings</button>
  </div>`;
}

export function renderMarkdownReport(state: ReportState, markdownToHtml: (md: string) => string): string {
  if (state.reportsBuilding) {
    return `<div class="reportView">
      <div class="reportHeader">
        ${renderDateControls(state, true)}
      </div>
      <div class="reportView__empty">Reports are being rebuilt\u2026</div>
    </div>`;
  }
  const md = state.reportMarkdown;
  return `<div class="reportView">
    <div class="reportHeader">
      ${renderDateControls(state)}
    </div>
    ${md
      ? `<div class="reportContent markdown-body">${markdownToHtml(md)}</div>`
      : `<div class="reportView__empty">No report data for this financial year.</div>`}
  </div>`;
}

function sortCgtEvents(events: CgtEvent[], sort: GenericSortState<CgtSortColumn>): CgtEvent[] {
  const dir = sort.direction === "asc" ? 1 : -1;
  return [...events].sort((a, b) => {
    let cmp = 0;
    switch (sort.column) {
      case "sell_date": cmp = a.sell_date.localeCompare(b.sell_date); break;
      case "buy_date": cmp = a.buy_date.localeCompare(b.buy_date); break;
      case "commodity": cmp = a.commodity.localeCompare(b.commodity); break;
      case "quantity": cmp = a.quantity - b.quantity; break;
      case "cost_basis": cmp = a.cost_basis - b.cost_basis; break;
      case "sale_proceeds": cmp = a.sale_proceeds - b.sale_proceeds; break;
      case "capital_gain": cmp = a.capital_gain - b.capital_gain; break;
      case "holding_days": cmp = a.holding_days - b.holding_days; break;
    }
    return cmp * dir;
  });
}

function filterCgtEvents(events: CgtEvent[], text: string): CgtEvent[] {
  const lower = text.toLowerCase();
  return events.filter((e) =>
    e.commodity.toLowerCase().includes(lower) ||
    e.sell_date.toLowerCase().includes(lower) ||
    e.buy_date.toLowerCase().includes(lower) ||
    e.sell_account.toLowerCase().includes(lower)
  );
}

type CgtSection = {
  id: "short_term" | "long_term" | "losses";
  title: string;
  events: CgtEvent[];
};

function partitionCgtEvents(events: CgtEvent[]): CgtSection[] {
  // Section assignment is determined entirely by the per-event facts the
  // generator already computed: gains <12mo are short-term, gains ≥12mo
  // (= discount_eligible) are long-term, anything <0 is a loss regardless
  // of holding period.
  const sections: CgtSection[] = [
    { id: "short_term", title: "Short-Term Capital Gains", events: [] },
    { id: "long_term",  title: "Long-Term Capital Gains",  events: [] },
    { id: "losses",     title: "Capital Losses",           events: [] },
  ];
  for (const e of events) {
    if (e.capital_gain < 0) sections[2].events.push(e);
    else if (e.discount_eligible) sections[1].events.push(e);
    else sections[0].events.push(e);
  }
  return sections;
}

type CgtCommodityGroup = {
  commodity: string;
  events: CgtEvent[];
  totalQty: number;
  totalCostBasis: number;
  totalProceeds: number;
  totalGain: number;
  avgHoldingDays: number;
};

function groupSectionByCommodity(events: CgtEvent[], sort?: GenericSortState<CgtSortColumn>): CgtCommodityGroup[] {
  const map = new Map<string, CgtEvent[]>();
  for (const e of events) {
    const list = map.get(e.commodity);
    if (list) list.push(e); else map.set(e.commodity, [e]);
  }
  const groups: CgtCommodityGroup[] = [];
  for (const [commodity, evts] of map) {
    const sorted = sort ? sortCgtEvents(evts, sort) : evts;
    groups.push({
      commodity,
      events: sorted,
      totalQty: evts.reduce((s, e) => s + e.quantity, 0),
      totalCostBasis: evts.reduce((s, e) => s + e.cost_basis, 0),
      totalProceeds: evts.reduce((s, e) => s + e.sale_proceeds, 0),
      totalGain: evts.reduce((s, e) => s + e.capital_gain, 0),
      avgHoldingDays: evts.length > 0 ? Math.round(evts.reduce((s, e) => s + e.holding_days, 0) / evts.length) : 0,
    });
  }
  if (sort) {
    const dir = sort.direction === "asc" ? 1 : -1;
    groups.sort((a, b) => {
      let cmp = 0;
      switch (sort.column) {
        case "commodity": cmp = a.commodity.localeCompare(b.commodity); break;
        case "quantity": cmp = a.totalQty - b.totalQty; break;
        case "cost_basis": cmp = a.totalCostBasis - b.totalCostBasis; break;
        case "sale_proceeds": cmp = a.totalProceeds - b.totalProceeds; break;
        case "capital_gain": cmp = a.totalGain - b.totalGain; break;
        case "holding_days": cmp = a.avgHoldingDays - b.avgHoldingDays; break;
        default: cmp = a.commodity.localeCompare(b.commodity); break;
      }
      return cmp * dir;
    });
  } else {
    groups.sort((a, b) => a.commodity.localeCompare(b.commodity));
  }
  return groups;
}

function renderCgtSection(
  section: CgtSection,
  sort: GenericSortState<CgtSortColumn> | undefined,
  scope: string,
  expanded: Set<string>,
): string {
  if (section.events.length === 0) return "";
  const groups = groupSectionByCommodity(section.events, sort);
  // Section totals are aggregated from the partitioned events directly so they
  // remain stable regardless of grouping or sort.
  const totalCost = section.events.reduce((s, e) => s + e.cost_basis, 0);
  const totalProceeds = section.events.reduce((s, e) => s + e.sale_proceeds, 0);
  const totalGain = section.events.reduce((s, e) => s + e.capital_gain, 0);
  const rows = groups.map((g) => {
    const groupKey = `${section.id}:${g.commodity}`;
    const open = expanded.has(groupKey);
    const arrow = open ? "▼" : "▶";
    const headerRow = `<tr class="cgtGroup__header" data-cgt-group="${escapeText(groupKey)}">
      <td colspan="3"><span class="cgtGroup__arrow">${arrow}</span> <strong>${escapeText(g.commodity)}</strong> <span class="cgtGroup__count">(${g.events.length})</span></td>
      <td class="num">${g.totalQty.toFixed(QUANTITY_DECIMALS)}</td>
      <td class="num">${money(g.totalCostBasis)}</td>
      <td class="num">${money(g.totalProceeds)}</td>
      <td class="num ${g.totalGain >= 0 ? "gain" : "loss"}">${money(g.totalGain)}</td>
      <td class="num">${g.avgHoldingDays}</td>
    </tr>`;
    if (!open) return headerRow;
    const detailRows = g.events.map((e) => `<tr class="cgtGroup__detail">
      <td>${e.sell_txn_id ? `<a class="reportLink" data-goto-account="${escapeText(e.sell_account)}" data-goto-txn="${escapeText(e.sell_txn_id)}">${escapeText(e.sell_date)}</a>` : escapeText(e.sell_date)}</td>
      <td>${e.trade_link_id ? `<a class="reportLink" data-goto-account="${escapeText(e.sell_account)}" data-goto-txn="${escapeText(e.trade_link_id)}">${escapeText(e.buy_date)}</a>` : escapeText(e.buy_date)}</td>
      <td>${escapeText(e.commodity)}</td>
      <td class="num">${e.quantity.toFixed(QUANTITY_DECIMALS)}</td>
      <td class="num">${money(e.cost_basis)}</td>
      <td class="num">${money(e.sale_proceeds)}</td>
      <td class="num ${e.capital_gain >= 0 ? "gain" : "loss"}">${money(e.capital_gain)}</td>
      <td class="num">${e.holding_days}</td>
    </tr>`).join("");
    return headerRow + detailRows;
  }).join("");
  return `<div class="cgtSection" data-cgt-section="${section.id}">
    <h3 class="cgtSection__title">${section.title} <span class="cgtSection__count">(${section.events.length})</span></h3>
    <div class="tableCard">
      <table class="txTable reportTable">
        <thead>
          <tr>
            ${renderSortHeader("Sell Date", "sell_date", sort, undefined, scope)}
            ${renderSortHeader("Buy Date", "buy_date", sort, undefined, scope)}
            ${renderSortHeader("Asset", "commodity", sort, undefined, scope)}
            ${renderSortHeader("Qty", "quantity", sort, "num", scope)}
            ${renderSortHeader("Cost Basis", "cost_basis", sort, "num", scope)}
            ${renderSortHeader("Proceeds", "sale_proceeds", sort, "num", scope)}
            ${renderSortHeader("Gain/Loss", "capital_gain", sort, "num", scope)}
            ${renderSortHeader("Days Held", "holding_days", sort, "num", scope)}
          </tr>
        </thead>
        <tbody>${rows}</tbody>
        <tfoot>
          <tr class="cgtSection__total">
            <td colspan="4"><strong>Total</strong></td>
            <td class="num"><strong>${money(totalCost)}</strong></td>
            <td class="num"><strong>${money(totalProceeds)}</strong></td>
            <td class="num ${totalGain >= 0 ? "gain" : "loss"}"><strong>${money(totalGain)}</strong></td>
            <td></td>
          </tr>
        </tfoot>
      </table>
    </div>
  </div>`;
}

function sortIncomeEvents(events: IncomeEvent[], sort: GenericSortState<IncomeSortColumn>): IncomeEvent[] {
  const dir = sort.direction === "asc" ? 1 : -1;
  return [...events].sort((a, b) => {
    let cmp = 0;
    switch (sort.column) {
      case "date": cmp = a.date.localeCompare(b.date); break;
      case "account": cmp = a.account.localeCompare(b.account); break;
      case "commodity": cmp = a.commodity.localeCompare(b.commodity); break;
      case "quantity": cmp = a.quantity - b.quantity; break;
      case "price": cmp = a.price - b.price; break;
      case "value": cmp = a.value - b.value; break;
    }
    return cmp * dir;
  });
}

function filterIncomeEvents(events: IncomeEvent[], text: string): IncomeEvent[] {
  const lower = text.toLowerCase();
  return events.filter((e) =>
    e.commodity.toLowerCase().includes(lower) ||
    e.account.toLowerCase().includes(lower) ||
    e.date.toLowerCase().includes(lower) ||
    e.asset_account.toLowerCase().includes(lower)
  );
}

type IncomeCommodityGroup = {
  commodity: string;
  events: IncomeEvent[];
  totalQty: number;
  totalValue: number;
};

function groupIncomeEventsByCommodity(
  events: IncomeEvent[],
  sort?: GenericSortState<IncomeSortColumn>,
): IncomeCommodityGroup[] {
  const map = new Map<string, IncomeEvent[]>();
  for (const e of events) {
    const list = map.get(e.commodity);
    if (list) list.push(e); else map.set(e.commodity, [e]);
  }
  const groups: IncomeCommodityGroup[] = [];
  for (const [commodity, evts] of map) {
    // Detail rows: default to date order (ascending) so the timeline reads
    // naturally; only override when the user explicitly picks a detail-level
    // sort column (date / account / quantity / price / value).
    const detailSort = sort && sort.column !== "commodity" ? sort : { column: "date" as IncomeSortColumn, direction: "asc" as const };
    const sorted = sortIncomeEvents(evts, detailSort);
    groups.push({
      commodity,
      events: sorted,
      totalQty: evts.reduce((s, e) => s + e.quantity, 0),
      totalValue: evts.reduce((s, e) => s + e.value, 0),
    });
  }
  // Group ordering: by the user's chosen column when meaningful for groups
  // (commodity, quantity, value); otherwise alphabetical.
  if (sort) {
    const dir = sort.direction === "asc" ? 1 : -1;
    groups.sort((a, b) => {
      let cmp = 0;
      switch (sort.column) {
        case "commodity": cmp = a.commodity.localeCompare(b.commodity); break;
        case "quantity": cmp = a.totalQty - b.totalQty; break;
        case "value": cmp = a.totalValue - b.totalValue; break;
        default: cmp = a.commodity.localeCompare(b.commodity); break;
      }
      return cmp * dir;
    });
  } else {
    groups.sort((a, b) => a.commodity.localeCompare(b.commodity));
  }
  return groups;
}

type IncomeSection = {
  id: "income" | "expenses";
  title: string;
  emptyMessage: string;
  events: IncomeEvent[];
};

function renderIncomeSection(
  section: IncomeSection,
  sort: GenericSortState<IncomeSortColumn> | undefined,
  scope: string,
  expanded: Set<string>,
): string {
  if (section.events.length === 0) {
    return `<div class="incomeSection" data-income-section="${section.id}">
      <h3 class="incomeSection__title">${section.title}</h3>
      <div class="reportView__empty">${escapeText(section.emptyMessage)}</div>
    </div>`;
  }
  const groups = groupIncomeEventsByCommodity(section.events, sort);
  const sectionTotal = section.events.reduce((s, e) => s + e.value, 0);
  // Style negative values like CGT losses so refunds / fee-on-income rows
  // read clearly as offsets to gross income.
  const numCls = (v: number) => v < 0 ? "num loss" : "num";
  const rows = groups.map((g) => {
    const groupKey = `${section.id}:${g.commodity}`;
    const open = expanded.has(groupKey);
    const arrow = open ? "▼" : "▶";
    const headerRow = `<tr class="incomeGroup__header" data-income-group="${escapeText(groupKey)}">
      <td colspan="3"><span class="incomeGroup__arrow">${arrow}</span> <strong>${escapeText(g.commodity)}</strong> <span class="incomeGroup__count">(${g.events.length})</span></td>
      <td class="${numCls(g.totalQty)}">${g.totalQty.toFixed(QUANTITY_DECIMALS)}</td>
      <td></td>
      <td class="${numCls(g.totalValue)}"><strong>${money(g.totalValue)}</strong></td>
    </tr>`;
    if (!open) return headerRow;
    const detailRows = g.events.map((e) => {
      const dateCell = e.txn_id && e.asset_account
        ? `<a class="reportLink" data-goto-account="${escapeText(e.asset_account)}" data-goto-txn="${escapeText(e.txn_id)}">${escapeText(e.date)}</a>`
        : escapeText(e.date);
      const whereCell = e.asset_account
        ? `<a class="reportLink" data-goto-account="${escapeText(e.asset_account)}" title="${escapeText(e.asset_account)}">${escapeText(shortAccountPath(e.asset_account, 24))}</a>`
        : "";
      return `<tr class="incomeGroup__detail">
        <td>${dateCell}</td>
        <td>${whereCell}</td>
        <td><a class="reportLink" data-goto-account="${escapeText(e.account)}">${escapeText(e.account)}</a></td>
        <td class="${numCls(e.quantity)}">${e.quantity.toFixed(QUANTITY_DECIMALS)}</td>
        <td class="num">${money(e.price)}</td>
        <td class="${numCls(e.value)}">${money(e.value)}</td>
      </tr>`;
    }).join("");
    return headerRow + detailRows;
  }).join("");
  return `<div class="incomeSection" data-income-section="${section.id}">
    <h3 class="incomeSection__title">${section.title} <span class="incomeSection__count">(${section.events.length})</span></h3>
    <div class="tableCard">
      <table class="txTable reportTable">
        <thead>
          <tr>
            ${renderSortHeader("Date", "date", sort, undefined, scope)}
            <th>Where</th>
            ${renderSortHeader("Category", "account", sort, undefined, scope)}
            ${renderSortHeader("Qty", "quantity", sort, "num", scope)}
            ${renderSortHeader("Price", "price", sort, "num", scope)}
            ${renderSortHeader("Value", "value", sort, "num", scope)}
          </tr>
        </thead>
        <tbody>${rows}</tbody>
        <tfoot>
          <tr class="incomeSection__total">
            <td colspan="5"><strong>Total</strong></td>
            <td class="${numCls(sectionTotal)}"><strong>${money(sectionTotal)}</strong></td>
          </tr>
        </tfoot>
      </table>
    </div>
  </div>`;
}

export function renderCgtReport(state: ReportState): string {
  const report = state.cgtReport;
  if (!report) return `<div class="reportView"><div class="reportView__empty">Select a financial year to generate the report.</div></div>`;
  const fy = report.financial_year;
  const fyText = fyLabel(fy, state.taxConfig);
  return `<div class="reportView">
    <div class="reportHeader">
      <h2 class="reportHeader__title">${renderNavButtons(state)}Capital Gains Tax - ${escapeText(fyText)}</h2>
      ${renderDateControls(state)}
      ${renderExportCsvButton("cgt")}
    </div>
    ${report.events.length > 0 ? `
    <div class="reportSummary">
      <div class="reportSummary__row"><span>Total Gains</span><span class="gain">${formatMoneyWhole(report.total_gains)}</span></div>
      <div class="reportSummary__row"><span>Total Losses</span><span class="loss">${formatMoneyWhole(report.total_losses)}</span></div>
      <div class="reportSummary__row reportSummary__row--total"><span>Net Capital Gain</span><span>${formatMoneyWhole(report.net_capital_gain)}</span></div>
      <div class="reportSummary__row reportSummary__row--total"><span>After CGT Discount</span><span>${formatMoneyWhole(report.total_discounted_gain)}</span></div>
    </div>
    <div class="reportFilter">
      <input type="text" id="cgtFilter" class="reportFilter__input" placeholder="Filter by asset, account, date\u2026" value="${escapeText(state.cgtFilterText ?? "")}" />
    </div>
    ${(() => {
      const events = state.cgtFilterText ? filterCgtEvents(report.events, state.cgtFilterText) : report.events;
      const sections = partitionCgtEvents(events);
      const expanded = state.cgtExpandedGroups ?? new Set<string>();
      return sections.map((s) => renderCgtSection(s, state.cgtSort, "cgt", expanded)).join("");
    })()}
    ` : `<div class="reportView__empty">No capital gains events in this financial year.</div>`}
    ${report.warnings.length > 0 ? `
    <div class="reportWarnings">
      <div class="reportWarnings__title">Warnings</div>
      ${report.warnings.map((w) => `<div class="reportWarnings__item">${escapeText(w)}</div>`).join("")}
    </div>
    ` : ""}
  </div>`;
}

export function renderIncomeReport(state: ReportState): string {
  const report = state.incomeReport;
  if (!report) return `<div class="reportView"><div class="reportView__empty">Select a financial year to generate the report.</div></div>`;
  const fy = report.financial_year;
  const fyText = fyLabel(fy, state.taxConfig);
  const filterText = state.incomeFilterText ?? "";
  const incomeEvents = filterText ? filterIncomeEvents(report.events, filterText) : report.events;
  const expenseEvents = filterText ? filterIncomeEvents(report.expense_events, filterText) : report.expense_events;
  const expanded = state.incomeExpandedGroups ?? new Set<string>();
  const sections: IncomeSection[] = [
    { id: "income", title: "Income", emptyMessage: "No income in this financial year.", events: incomeEvents },
    { id: "expenses", title: "Expenses", emptyMessage: "No expenses in this financial year.", events: expenseEvents },
  ];
  return `<div class="reportView">
    <div class="reportHeader">
      <h2 class="reportHeader__title">${renderNavButtons(state)}Income Tax - ${escapeText(fyText)}</h2>
      ${renderDateControls(state)}
      ${renderExportCsvButton("income")}
    </div>
    <div class="reportSummary">
      <div class="reportSummary__row"><span>Total Income</span><span>${formatMoneyWhole(report.total_income)}</span></div>
      <div class="reportSummary__row"><span>Total Expenses</span><span>${formatMoneyWhole(report.total_expenses)}</span></div>
      <div class="reportSummary__row reportSummary__row--total"><span>Net</span><span>${formatMoneyWhole(report.net)}</span></div>
    </div>
    <div class="reportFilter">
      <input type="text" id="incomeFilter" class="reportFilter__input" placeholder="Filter by asset, category, date…" value="${escapeText(filterText)}" />
    </div>
    ${sections.map((s) => renderIncomeSection(s, state.incomeSort, "income", expanded)).join("")}
    ${report.warnings.length > 0 ? `
    <div class="reportWarnings">
      <div class="reportWarnings__title">Warnings</div>
      ${report.warnings.map((w) => `<div class="reportWarnings__item">${escapeText(w)}</div>`).join("")}
    </div>
    ` : ""}
  </div>`;
}

function sortBalances(
  holdings: CoinBalance[],
  sort: GenericSortState<BalancesSortColumn>,
): CoinBalance[] {
  const dir = sort.direction === "asc" ? 1 : -1;
  return [...holdings].sort((a, b) => {
    let cmp = 0;
    switch (sort.column) {
      case "commodity": cmp = a.commodity.localeCompare(b.commodity); break;
      case "quantity": cmp = a.quantity - b.quantity; break;
      case "price": cmp = a.price - b.price; break;
      case "value": cmp = a.value - b.value; break;
      case "portfolio_weight": cmp = a.portfolio_weight - b.portfolio_weight; break;
    }
    return cmp * dir;
  });
}

function filterBalances(holdings: CoinBalance[], text: string): CoinBalance[] {
  const lower = text.toLowerCase();
  return holdings.filter((h) => h.commodity.toLowerCase().includes(lower));
}

export function renderBalancesReport(state: ReportState): string {
  const report = state.balancesReport;
  if (!report) {
    return `<div class="reportView"><div class="reportView__empty">Select a financial year to generate the report.</div></div>`;
  }
  const asOf = escapeText(report.as_of_date);
  const currency = escapeText(report.base_currency);
  const holdings = state.balancesFilterText
    ? filterBalances(report.holdings, state.balancesFilterText)
    : report.holdings;
  const sorted = state.balancesSort ? sortBalances(holdings, state.balancesSort) : holdings;
  const filterVal = escapeText(state.balancesFilterText ?? "");

  const expanded = state.balancesExpanded ?? new Set<string>();
  const rows = sorted.map((h) => {
    const weightPct = (h.portfolio_weight * 100).toFixed(1);
    const barWidth = Math.max(0, Math.min(100, h.portfolio_weight * 100));
    const isOpen = expanded.has(h.commodity);
    const hasAccounts = h.accounts.length > 0;
    const chevron = hasAccounts
      ? `<span class="balanceRow__chevron${isOpen ? " balanceRow__chevron--open" : ""}" aria-hidden="true">▸</span>`
      : `<span class="balanceRow__chevron balanceRow__chevron--placeholder" aria-hidden="true"></span>`;
    const rowCls = `balanceRow${hasAccounts ? " balanceRow--clickable" : ""}${isOpen ? " balanceRow--open" : ""}`;
    const rowAttrs = hasAccounts
      ? ` data-balance-commodity="${escapeText(h.commodity)}" role="button" tabindex="0" aria-expanded="${isOpen}"`
      : "";
    const main = `<tr class="${rowCls}"${rowAttrs}>
      <td>${chevron}<span class="balanceWeightBar__track"><span class="balanceWeightBar" style="width:${barWidth.toFixed(2)}%"></span></span>${escapeText(h.commodity)}</td>
      <td class="num">${h.quantity.toFixed(QUANTITY_DECIMALS)}</td>
      <td class="num">${money(h.price)}</td>
      <td class="num">${money(h.value)}</td>
      <td class="num">${weightPct}%</td>
    </tr>`;
    if (!isOpen || !hasAccounts) return main;
    const subRows = h.accounts.map((a) => {
      const sharePct = h.quantity !== 0 ? (a.quantity / h.quantity) * 100 : 0;
      const display = shortAccountPath(a.account, 18);
      return `<tr class="balanceAccountRow">
        <td class="balanceAccountRow__name" title="${escapeText(a.account)}">${escapeText(display)}</td>
        <td class="num">${a.quantity.toFixed(QUANTITY_DECIMALS)}</td>
        <td class="num"></td>
        <td class="num">${money(a.value)}</td>
        <td class="num">${sharePct.toFixed(1)}%</td>
      </tr>`;
    }).join("");
    return main + subRows;
  }).join("");

  return `<div class="reportView">
    <div class="reportHeader">
      <h2 class="reportHeader__title">${renderNavButtons(state)}Balances - as of ${asOf}</h2>
      ${renderDateControls(state)}
      ${renderExportCsvButton("balances")}
    </div>
    <div class="reportSummary">
      <div class="reportSummary__row"><span>Holdings</span><span>${report.holdings.length} ${report.holdings.length === 1 ? "coin" : "coins"}</span></div>
      <div class="reportSummary__row"><span>As of</span><span>${asOf}</span></div>
      <div class="reportSummary__row reportSummary__row--total"><span>Total value (${currency})</span><span>${formatMoneyWhole(report.total_value)}</span></div>
    </div>
    <div class="reportFilter">
      <input type="search" id="balancesFilter" class="reportFilter__input" placeholder="Filter by coin…" value="${filterVal}" />
    </div>
    ${report.holdings.length > 0 ? `
    <div class="tableCard">
      <table class="txTable reportTable">
        <thead><tr>
          ${renderSortHeader("Coin", "commodity", state.balancesSort, undefined, "balances")}
          ${renderSortHeader("Quantity", "quantity", state.balancesSort, "num", "balances")}
          ${renderSortHeader(`Spot (${currency})`, "price", state.balancesSort, "num", "balances")}
          ${renderSortHeader(`Value (${currency})`, "value", state.balancesSort, "num", "balances")}
          ${renderSortHeader("%", "portfolio_weight", state.balancesSort, "num", "balances")}
        </tr></thead>
        <tbody>${rows}</tbody>
      </table>
    </div>
    ` : `<div class="reportView__empty">No holdings at this date.</div>`}
    ${report.warnings.length > 0 ? `
    <div class="reportWarnings">
      <div class="reportWarnings__title">Warnings</div>
      ${report.warnings.map((w) => `<div class="reportWarnings__item">${escapeText(w)}</div>`).join("")}
    </div>
    ` : ""}
  </div>`;
}

function lossPositionBody(state: ReportState, report: LossHarvestReport): string {
  const currency = escapeText(report.base_currency);
  const gainLoss = (v: number) => (v >= 0 ? "gain" : "loss");
  const expanded = state.lossHarvestExpanded ?? new Set<string>();

  const rows = report.positions.map((p) => {
    const pctBelow = (p.pct_below_cost * 100).toFixed(1);
    const isOpen = expanded.has(p.commodity);
    const hasLots = p.lots.length > 0;
    const chevron = hasLots
      ? `<span class="balanceRow__chevron${isOpen ? " balanceRow__chevron--open" : ""}" aria-hidden="true">▸</span>`
      : `<span class="balanceRow__chevron balanceRow__chevron--placeholder" aria-hidden="true"></span>`;
    const rowCls = `balanceRow${hasLots ? " balanceRow--clickable" : ""}${isOpen ? " balanceRow--open" : ""}`;
    const rowAttrs = hasLots
      ? ` data-loss-commodity="${escapeText(p.commodity)}" role="button" tabindex="0" aria-expanded="${isOpen}"`
      : "";
    const main = `<tr class="${rowCls}"${rowAttrs}>
      <td>${chevron}${escapeText(p.commodity)}</td>
      <td class="num">${p.quantity.toFixed(QUANTITY_DECIMALS)}</td>
      <td class="num">${money(p.cost_basis)}</td>
      <td class="num">${money(p.value)}</td>
      <td class="num loss">-${money(p.unrealised_loss)}</td>
      <td class="num">${pctBelow}%</td>
    </tr>`;
    if (!isOpen || !hasLots) return main;
    // FIFO parcels, oldest-first: a partial sale disposes these from the top,
    // so the per-lot gain/loss shows what you'd actually realise.
    const lotRows = p.lots.map((lot) => {
      const cls = gainLoss(lot.unrealised);
      const sign = lot.unrealised >= 0 ? "+" : "-";
      const lotPct = lot.cost_basis !== 0 ? (lot.unrealised / lot.cost_basis) * 100 : 0;
      return `<tr class="balanceAccountRow lossLotRow">
        <td class="balanceAccountRow__name" title="${escapeText(lot.acquisition_date)}">${escapeText(lot.acquisition_date)} @ ${money(lot.cost_per_unit)}</td>
        <td class="num">${lot.quantity.toFixed(QUANTITY_DECIMALS)}</td>
        <td class="num">${money(lot.cost_basis)}</td>
        <td class="num">${money(lot.value)}</td>
        <td class="num ${cls}">${sign}${money(Math.abs(lot.unrealised))}</td>
        <td class="num ${cls}">${lotPct >= 0 ? "+" : ""}${lotPct.toFixed(1)}%</td>
      </tr>`;
    }).join("");
    return main + lotRows;
  }).join("");

  return `
    <div class="reportSummary">
      <div class="reportSummary__row"><span>Realised gains this year (${currency})</span><span class="${gainLoss(report.realised_net_gain)}" title="Short-term ${formatMoneyWhole(report.realised_short_gains)} / long-term ${formatMoneyWhole(report.realised_long_gains)}">${formatMoneyWhole(report.realised_net_gain)}</span></div>
      <div class="reportSummary__row"><span>Harvestable losses (${currency})</span><span class="loss">-${formatMoneyWhole(report.total_realisable_loss)}</span></div>
      <div class="reportSummary__row"><span>→ Offsets gains now</span><span>${formatMoneyWhole(report.offset_now)}</span></div>
      <div class="reportSummary__row"><span>→ Carries forward</span><span>${formatMoneyWhole(report.carry_forward)}</span></div>
      <div class="reportSummary__row reportSummary__row--total"><span>Estimated tax saved this year (@${report.marginal_rate_percent}%)</span><span class="gain">${formatMoneyWhole(report.estimated_tax_saved)}</span></div>
    </div>
    <p class="reportNote">Whole-position unrealised losses at FIFO cost basis. A capital loss offsets capital gains before the ${report.cgt_discount_percent}% CGT discount — applied to non-discounted gains first, then discounted gains (worth half) — capped at this year's gains; the remainder carries forward. Estimate only.</p>
    ${report.positions.length > 0 ? `
    <div class="tableCard">
      <table class="txTable reportTable">
        <thead><tr>
          <th>Coin</th>
          <th class="num">Quantity</th>
          <th class="num">Cost basis (${currency})</th>
          <th class="num">Value (${currency})</th>
          <th class="num">Unrealised loss (${currency})</th>
          <th class="num">% below cost</th>
        </tr></thead>
        <tbody>${rows}</tbody>
      </table>
    </div>
    ` : `<div class="reportView__empty">No holdings are currently underwater — nothing to harvest.</div>`}`;
}

function lossParcelBody(report: LossHarvestReport): string {
  const currency = escapeText(report.base_currency);
  const rows = report.underwater_parcels.map((p) => {
    const pct = (p.pct_below_cost * 100).toFixed(1);
    return `<tr>
      <td>${escapeText(p.commodity)}</td>
      <td>${escapeText(p.acquisition_date)}</td>
      <td class="num">${p.quantity.toFixed(QUANTITY_DECIMALS)}</td>
      <td class="num">${money(p.cost_per_unit)}</td>
      <td class="num">${money(p.value)}</td>
      <td class="num loss">-${money(p.unrealised_loss)}</td>
      <td class="num">${pct}%</td>
    </tr>`;
  }).join("");

  return `
    <div class="reportSummary">
      <div class="reportSummary__row"><span>Underwater parcels</span><span>${report.underwater_parcels.length}</span></div>
      <div class="reportSummary__row reportSummary__row--total"><span>Total underwater-parcel loss (${currency})</span><span class="loss">-${formatMoneyWhole(report.total_parcel_loss)}</span></div>
    </div>
    <p class="reportNote">Every FIFO parcel currently below its cost, across <strong>all</strong> holdings — including positions that are net in gain. Under FIFO you dispose parcels oldest-first, so realising a specific underwater parcel here generally needs <strong>specific-parcel identification</strong> (permitted for AU CGT; the engine keeps its books on FIFO). The tax saved depends on which parcels you realise and your gains — see the By position view.</p>
    ${report.underwater_parcels.length > 0 ? `
    <div class="tableCard">
      <table class="txTable reportTable">
        <thead><tr>
          <th>Coin</th>
          <th>Acquired</th>
          <th class="num">Quantity</th>
          <th class="num">Cost/unit (${currency})</th>
          <th class="num">Value (${currency})</th>
          <th class="num">Unrealised loss (${currency})</th>
          <th class="num">% below cost</th>
        </tr></thead>
        <tbody>${rows}</tbody>
      </table>
    </div>
    ` : `<div class="reportView__empty">No parcels are currently underwater.</div>`}`;
}

export function renderLossHarvestReport(state: ReportState): string {
  const report = state.lossHarvestReport;
  if (!report) {
    return `<div class="reportView"><div class="reportView__empty">Select a financial year to generate the report.</div></div>`;
  }
  const asOf = escapeText(report.as_of_date);
  const view = state.lossHarvestView ?? "position";
  const body = view === "parcel" ? lossParcelBody(report) : lossPositionBody(state, report);
  return `<div class="reportView">
    <div class="reportHeader">
      <h2 class="reportHeader__title">${renderNavButtons(state)}Tax Savings - as of ${asOf}</h2>
      ${renderDateControls(state)}
    </div>
    <div class="lossViewToggle reportDateControls__toggle">
      <button class="reportDateToggle ${view === "position" ? "reportDateToggle--active" : ""}" data-loss-view="position">By position</button>
      <button class="reportDateToggle ${view === "parcel" ? "reportDateToggle--active" : ""}" data-loss-view="parcel">By parcel</button>
    </div>
    ${body}
    ${report.warnings.length > 0 ? `
    <div class="reportWarnings">
      <div class="reportWarnings__title">Warnings</div>
      ${report.warnings.map((w) => `<div class="reportWarnings__item">${escapeText(w)}</div>`).join("")}
    </div>
    ` : ""}
  </div>`;
}

function renderAccountToggles(
  accounts: string[],
  excluded: string[],
  prefix: string,
): string {
  if (accounts.length === 0) return `<div class="accountToggles__empty">No accounts found</div>`;
  return accounts.map((acct) => {
    const isExcluded = excluded.includes(acct);
    return `<label class="accountToggle${isExcluded ? " accountToggle--excluded" : ""}">
      <input type="checkbox" class="accountToggle__input" data-account-group="${prefix}" value="${escapeText(acct)}" ${isExcluded ? "" : "checked"} />
      <span class="accountToggle__name">${escapeText(acct)}</span>
    </label>`;
  }).join("");
}

export function renderTaxSettingsModal(state: ReportState): string {
  if (!state.taxSettingsOpen) return "";
  const cfg = state.taxConfig ?? { financial_year_end_month: 6, financial_year_end_day: 30, cgt_discount_percent: 50, cgt_discount_holding_months: 12, non_taxable_accounts: [], non_deductible_accounts: [], marginal_tax_rate_percent: 47 };
  const avail = state.availableReportAccounts ?? { income: [], expenses: [] };
  return `
    <div class="modalOverlay" role="dialog" aria-modal="true">
      <div class="modal modal--wide">
        <h2 class="modal__title">Tax Settings</h2>
        <div class="modal__body">
          <div class="settingsSection">
            <h3 class="settingsSection__heading">Financial Year</h3>
            <div class="settingsSection__row">
              <label class="formField">
                <span class="formField__label">FY end month</span>
                <input id="taxFyMonth" type="number" min="1" max="12" value="${cfg.financial_year_end_month}" class="formField__input" />
              </label>
              <label class="formField">
                <span class="formField__label">FY end day</span>
                <input id="taxFyDay" type="number" min="1" max="31" value="${cfg.financial_year_end_day}" class="formField__input" />
              </label>
            </div>
          </div>
          <div class="settingsSection">
            <h3 class="settingsSection__heading">Capital Gains</h3>
            <div class="settingsSection__row">
              <label class="formField">
                <span class="formField__label">CGT discount %</span>
                <input id="taxCgtPercent" type="number" min="0" max="100" value="${cfg.cgt_discount_percent}" class="formField__input" />
              </label>
              <label class="formField">
                <span class="formField__label">Holding period (months)</span>
                <input id="taxCgtMonths" type="number" min="0" value="${cfg.cgt_discount_holding_months}" class="formField__input" />
              </label>
            </div>
            <div class="settingsSection__row">
              <label class="formField">
                <span class="formField__label">Marginal tax rate %</span>
                <input id="taxMarginalRate" type="number" min="0" max="100" value="${cfg.marginal_tax_rate_percent}" class="formField__input" />
              </label>
            </div>
            <p class="settingsSection__hint">Used by the Tax Savings report to estimate dollars saved by harvesting capital losses.</p>
          </div>
          ${avail.income.length > 0 ? `
          <div class="settingsSection">
            <h3 class="settingsSection__heading">Taxable Income</h3>
            <p class="settingsSection__hint">Uncheck accounts that are not taxable income.</p>
            <div class="accountToggles">
              ${renderAccountToggles(avail.income, cfg.non_taxable_accounts ?? [], "income")}
            </div>
          </div>
          ` : ""}
          ${avail.expenses.length > 0 ? `
          <div class="settingsSection">
            <h3 class="settingsSection__heading">Deductions</h3>
            <p class="settingsSection__hint">Uncheck accounts that are not tax deductible.</p>
            <div class="accountToggles">
              ${renderAccountToggles(avail.expenses, cfg.non_deductible_accounts ?? [], "expenses")}
            </div>
          </div>
          ` : ""}
        </div>
        <div class="modal__actions">
          <button id="taxSettingsSave" class="btn" type="button">Save</button>
          <button id="taxSettingsCancel" class="btn btn--secondary" type="button">Cancel</button>
        </div>
      </div>
    </div>
  `;
}

/// Global Settings modal. Currently holds the "Accounts included in balance"
/// editor (extra primary-account prefixes). Designed as the shared shell for
/// app-wide settings — other global settings (currency, decimals, data folder)
/// can be added as further `settingsSection`s.
export function renderGlobalSettingsModal(state: ReportState): string {
  if (!state.globalSettingsOpen) return "";
  // The draft is the working copy while the modal is open; fall back to the
  // persisted list so the modal still renders if opened without seeding.
  const prefixes = state.globalSettingsDraft ?? state.extraPrimaryAccountPrefixes ?? [];
  const options = (state.allAccounts ?? []).filter((a) => a.startsWith("assets:"));
  const rows = prefixes.length
    ? prefixes
        .map(
          (p) => `
          <li class="prefixList__row">
            <span class="prefixList__name">${escapeText(p)}</span>
            <button class="prefixList__remove" type="button" data-prefix-remove="${escapeText(p)}" aria-label="Remove ${escapeText(p)}">✕</button>
          </li>`,
        )
        .join("")
    : `<li class="prefixList__empty">Only source-folder accounts (wallets, exchanges, banks) count toward balances.</li>`;
  return `
    <div class="modalOverlay" role="dialog" aria-modal="true">
      <div class="modal modal--wide">
        <h2 class="modal__title">Global Settings</h2>
        <div class="modal__body">
          <div class="settingsSection">
            <h3 class="settingsSection__heading">Accounts included in balance</h3>
            <p class="settingsSection__hint">Source-folder accounts (wallets, exchanges, banks) always count. Add nominal account prefixes here — e.g. <code>assets:staking</code> — to also count them in the Balances, Performance and Tax Savings reports. Each entry includes its sub-accounts.</p>
            <ul class="prefixList" data-testid="primary-prefix-list">${rows}</ul>
            <div class="settingsSection__row">
              <label class="formField formField--grow">
                <span class="formField__label">Add account prefix</span>
                <input id="globalPrefixInput" data-testid="primary-prefix-input" class="formField__input" list="globalPrefixOptions" placeholder="assets:staking" />
              </label>
              <button id="globalPrefixAdd" class="btn btn--secondary" type="button">Add</button>
            </div>
            <datalist id="globalPrefixOptions">
              ${options.map((a) => `<option value="${escapeText(a)}"></option>`).join("")}
            </datalist>
          </div>
        </div>
        <div class="modal__actions">
          <button id="globalSettingsSave" class="btn" type="button">Save</button>
          <button id="globalSettingsCancel" class="btn btn--secondary" type="button">Cancel</button>
        </div>
      </div>
    </div>
  `;
}

export function renderPerformanceReport(state: ReportState): string {
  const report = state.performanceReport;
  if (!report) {
    return `<div class="reportView"><div class="reportView__empty">Select a financial year to generate the report.</div></div>`;
  }
  const cur = escapeText(report.base_currency);
  const fmt = money; // table cells: 2dp + separators
  const fmtWhole = formatMoneyWhole; // headline / summary stats: whole dollars
  const cls = (v: number) => (v >= 0 ? "gain" : "loss");
  const windowLabel = `${escapeText(report.date_from)} → ${escapeText(report.date_to)}`;
  const header = `<div class="reportHeader">
      <h2 class="reportHeader__title">${renderNavButtons(state)}Performance — ${windowLabel}</h2>
      ${renderDateControls(state)}
    </div>`;

  if (report.points.length === 0) {
    // Omit the chart mount entirely so the chart lifecycle hook tears down.
    return `<div class="reportView">
    ${header}
    <div class="reportView__empty">No performance data in this window.</div>
  </div>`;
  }

  const pctText =
    report.total_return_pct == null
      ? ""
      : `<span class="performanceSummary__pct ${cls(report.total_return_pct)}">${(report.total_return_pct * 100).toFixed(1)}%</span>`;

  const rows = report.points
    .map(
      (p) => `
        <tr>
          <td>${escapeText(p.label)}</td>
          <td class="num ${cls(p.realised_gain)}">${fmt(p.realised_gain)}</td>
          <td class="num">${fmt(p.income)}</td>
          <td class="num ${cls(p.unrealised_change)}">${fmt(p.unrealised_change)}</td>
          <td class="num">${fmt(p.portfolio_value)}</td>
        </tr>`,
    )
    .join("");

  // Per-holding attribution: realised + Δunrealised over the window (where the
  // return came from). Subtotal + income = total return.
  const attrSubtotal = report.total_realised_gain + report.unrealised_change;
  const attrRows = report.attribution
    .map(
      (a) => `
        <tr>
          <td>${escapeText(a.commodity)}</td>
          <td class="num ${cls(a.realised_gain)}">${fmt(a.realised_gain)}</td>
          <td class="num ${cls(a.unrealised_change)}">${fmt(a.unrealised_change)}</td>
          <td class="num ${cls(a.total)}">${fmt(a.total)}</td>
          <td class="num">${fmt(a.closing_value)}</td>
        </tr>`,
    )
    .join("");
  const attributionTable =
    report.attribution.length === 0
      ? ""
      : `<div class="performanceTableTitle">Where the return came from — by holding</div>
    <div class="tableCard">
      <table class="txTable reportTable performanceTable">
        <thead>
          <tr>
            <th>Holding</th>
            <th class="num">Realised</th>
            <th class="num">Unrealised Δ</th>
            <th class="num">Contribution</th>
            <th class="num">Value (${cur})</th>
          </tr>
        </thead>
        <tbody>${attrRows}
        </tbody>
        <tfoot>
          <tr class="performanceTable__total">
            <td><strong>Subtotal (excl. income)</strong></td>
            <td class="num ${cls(report.total_realised_gain)}"><strong>${fmt(report.total_realised_gain)}</strong></td>
            <td class="num ${cls(report.unrealised_change)}"><strong>${fmt(report.unrealised_change)}</strong></td>
            <td class="num ${cls(attrSubtotal)}"><strong>${fmt(attrSubtotal)}</strong></td>
            <td class="num"><strong>${fmt(report.closing_value)}</strong></td>
          </tr>
        </tfoot>
      </table>
      <div class="performanceTableNote">+ income ${fmt(report.total_income)} = total return ${fmt(report.total_return)} ${cur}</div>
    </div>`;

  return `<div class="reportView">
    ${header}
    <div class="performanceSummary">
      <div class="performanceSummary__label">Total return · ${windowLabel}</div>
      <div class="performanceSummary__headlineRow">
        <span class="performanceSummary__headline ${cls(report.total_return)}">${fmtWhole(report.total_return)} ${cur}</span>
        ${pctText}
      </div>
      <div class="performanceSummary__stats">
        <div class="performanceStat"><span class="performanceStat__label">Realised</span><span class="performanceStat__value ${cls(report.total_realised_gain)}">${fmtWhole(report.total_realised_gain)}</span></div>
        <div class="performanceStat"><span class="performanceStat__label">Income</span><span class="performanceStat__value performanceStat__value--income">${fmtWhole(report.total_income)}</span></div>
        <div class="performanceStat"><span class="performanceStat__label">Unrealised change</span><span class="performanceStat__value ${cls(report.unrealised_change)}">${fmtWhole(report.unrealised_change)}</span></div>
      </div>
      <div class="performanceSummary__context">Portfolio value: ${fmtWhole(report.value_open)} → ${fmtWhole(report.closing_value)} ${cur}</div>
    </div>
    <div class="performanceChart">
      <div id="performanceChart" data-morph-preserve></div>
    </div>
    ${
      report.account_breakdown.length > 0
        ? `<div class="performanceTableTitle">Growth by category — rebased to start · ${
            report.base_account_scope ? escapeText(report.base_account_scope) : "Top level"
          }</div>
    <div class="performanceChart">
      <div id="performanceGrowthChart" data-morph-preserve></div>
    </div>`
        : ""
    }
    <div class="tableCard">
      <table class="txTable reportTable performanceTable">
        <thead>
          <tr>
            <th>Month</th>
            <th class="num">Realised</th>
            <th class="num">Income</th>
            <th class="num">Unrealised Δ</th>
            <th class="num">Value (${cur})</th>
          </tr>
        </thead>
        <tbody>${rows}
        </tbody>
        <tfoot>
          <tr class="performanceTable__total">
            <td><strong>Total</strong></td>
            <td class="num ${cls(report.total_realised_gain)}"><strong>${fmt(report.total_realised_gain)}</strong></td>
            <td class="num"><strong>${fmt(report.total_income)}</strong></td>
            <td class="num ${cls(report.unrealised_change)}"><strong>${fmt(report.unrealised_change)}</strong></td>
            <td class="num"><strong>${fmt(report.closing_value)}</strong></td>
          </tr>
        </tfoot>
      </table>
    </div>
    ${attributionTable}
    ${
      report.warnings.length > 0
        ? `
    <div class="reportWarnings">
      <div class="reportWarnings__title">Warnings</div>
      ${report.warnings.map((w) => `<div class="reportWarnings__item">${escapeText(w)}</div>`).join("")}
    </div>
    `
        : ""
    }
  </div>`;
}

export function renderReportScope(state: ReportState): string {
  return `<div class="reportScope">
    <label class="reportScope__label" for="reportBaseScope">Scope</label>
    <input type="text" id="reportBaseScope" class="reportScopeInput" placeholder="e.g. assets:crypto" value="${state.reportBaseScope ?? ""}" />
  </div>`;
}

export function renderReportMenu(
  selectedReport?: "cgt" | "income" | "balances" | "performance" | "loss_harvest",
): string {
  return `
    <div class="reportMenu">
      <div class="reportMenu__heading">Tax</div>
      <button class="reportMenu__item ${selectedReport === "cgt" ? "reportMenu__item--active" : ""}" data-report="cgt">Capital Gains Tax</button>
      <button class="reportMenu__item ${selectedReport === "income" ? "reportMenu__item--active" : ""}" data-report="income">Income Tax</button>
      <button class="reportMenu__item ${selectedReport === "loss_harvest" ? "reportMenu__item--active" : ""}" data-report="loss_harvest">Tax Savings</button>
      <div class="reportMenu__heading">Portfolio</div>
      <button class="reportMenu__item ${selectedReport === "balances" ? "reportMenu__item--active" : ""}" data-report="balances">Balances</button>
      <button class="reportMenu__item ${selectedReport === "performance" ? "reportMenu__item--active" : ""}" data-report="performance">Performance</button>
    </div>
  `;
}
