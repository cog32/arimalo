import { describe, it, expect } from "vitest";
import {
  renderCgtReport,
  renderIncomeReport,
  renderBalancesReport,
  renderPerformanceReport,
  renderLossHarvestReport,
  renderTaxSettingsModal,
  renderGlobalSettingsModal,
  renderReportMenu,
  renderReportScope,
  renderRebuildStrip,
  renderSortHeader,
  renderNavButtons,
  withFadeInClass,
  escapeText,
  fyLabel,
  fyYearOptions,
  currentFinancialYear,
  type ReportState,
} from "./render";
import type {
  BalancesReport,
  CgtReport,
  IncomeTaxReport,
  GenericSortState,
  LossHarvestReport,
  PerformanceReport,
  TaxConfig,
} from "./types";
import {
  resolveAccountFolder,
  shortAddress,
} from "./account-utils";

describe("escapeText", () => {
  it("escapes HTML entities", () => {
    expect(escapeText('<script>"hello"</script>')).toBe(
      "&lt;script&gt;&quot;hello&quot;&lt;/script&gt;"
    );
  });
});

describe("renderRebuildStrip", () => {
  it("renders inactive strip when no rebuild is in flight", () => {
    const html = renderRebuildStrip(0);
    expect(html).toContain('data-testid="rebuild-strip"');
    expect(html).toContain('class="rebuildStrip"');
    expect(html).not.toContain("rebuildStrip--active");
    expect(html).toContain('aria-hidden="true"');
  });

  it("renders active strip when at least one mutation is pending", () => {
    const html = renderRebuildStrip(1);
    expect(html).toContain('data-testid="rebuild-strip"');
    expect(html).toContain("rebuildStrip rebuildStrip--active");
    expect(html).toContain('aria-hidden="false"');
  });

  it("stays active for any positive count", () => {
    expect(renderRebuildStrip(3)).toContain("rebuildStrip--active");
  });
});

describe("withFadeInClass", () => {
  it("appends txRow--entering when the txnId is newly arrived", () => {
    const out = withFadeInClass("txRow--swap", "txn:new", new Set(["txn:new"]));
    expect(out).toBe("txRow--swap txRow--entering");
  });

  it("leaves classes unchanged when the txnId is not in the new set", () => {
    const out = withFadeInClass("txRow--swap", "txn:old", new Set(["txn:new"]));
    expect(out).toBe("txRow--swap");
  });

  it("handles undefined justAddedTxnIds without crashing", () => {
    const out = withFadeInClass("a", "txn:x", undefined);
    expect(out).toBe("a");
  });

});

describe("fyLabel", () => {
  it("defaults to Jul-Jun for Australian FY", () => {
    expect(fyLabel("2025")).toBe("FY 2025 (Jul 2024 - Jun 2025)");
  });

  it("uses TaxConfig end month", () => {
    expect(fyLabel("2025", { financial_year_end_month: 3, financial_year_end_day: 31, cgt_discount_percent: 50, cgt_discount_holding_months: 12, non_taxable_accounts: [], non_deductible_accounts: [], marginal_tax_rate_percent: 47 }))
      .toBe("FY 2025 (Apr 2024 - Mar 2025)");
  });

  it("handles calendar year (end month 12)", () => {
    expect(fyLabel("2025", { financial_year_end_month: 12, financial_year_end_day: 31, cgt_discount_percent: 50, cgt_discount_holding_months: 12, non_taxable_accounts: [], non_deductible_accounts: [], marginal_tax_rate_percent: 47 }))
      .toBe("FY 2025 (Jan 2025 - Dec 2025)");
  });
});

describe("currentFinancialYear", () => {
  const d = (s: string) => new Date(s + "T12:00:00");
  it("uses a 30 June end by default (FY labelled by end year)", () => {
    expect(currentFinancialYear(undefined, d("2026-06-21"))).toBe(2026);
    expect(currentFinancialYear(undefined, d("2026-06-30"))).toBe(2026);
    expect(currentFinancialYear(undefined, d("2026-07-01"))).toBe(2027);
    expect(currentFinancialYear(undefined, d("2026-09-01"))).toBe(2027);
    expect(currentFinancialYear(undefined, d("2026-01-15"))).toBe(2026);
  });
  it("honours a custom FY end", () => {
    const dec = { financial_year_end_month: 12, financial_year_end_day: 31 } as TaxConfig;
    expect(currentFinancialYear(dec, d("2026-06-21"))).toBe(2026);
    expect(currentFinancialYear(dec, d("2026-12-31"))).toBe(2026);
    expect(currentFinancialYear(dec, d("2027-01-01"))).toBe(2027);
    const mar = { financial_year_end_month: 3, financial_year_end_day: 31 } as TaxConfig;
    expect(currentFinancialYear(mar, d("2026-03-31"))).toBe(2026);
    expect(currentFinancialYear(mar, d("2026-04-01"))).toBe(2027);
  });
});

describe("fyYearOptions", () => {
  it("always offers the current FY and selects it by default, even if absent from availableYears", () => {
    const cur = currentFinancialYear();
    const html = fyYearOptions(undefined, [cur - 2, cur - 1]);
    expect(html).toContain(`value="${cur}" selected`);
  });
  it("marks the explicitly selected year and includes it even if out of range", () => {
    const html = fyYearOptions(1999, [2025, 2024]);
    expect(html).toContain(`value="1999" selected`);
    expect(html).toContain('value="2025"');
  });
  it("sorts years descending", () => {
    const cur = currentFinancialYear();
    const html = fyYearOptions(undefined, []);
    expect(html.indexOf(`value="${cur - 5}"`)).toBeGreaterThan(html.indexOf(`value="${cur}"`));
  });
});

describe("renderReportMenu", () => {
  it("renders menu with no selection", () => {
    const html = renderReportMenu();
    expect(html).toContain("Capital Gains Tax");
    expect(html).toContain("Income Tax");
    expect(html).not.toContain("reportMenu__item--active");
  });

  it("highlights selected CGT report", () => {
    const html = renderReportMenu("cgt");
    expect(html).toContain('reportMenu__item--active" data-report="cgt"');
    expect(html).not.toContain('reportMenu__item--active" data-report="income"');
  });

  it("highlights selected Income report", () => {
    const html = renderReportMenu("income");
    expect(html).toContain('reportMenu__item--active" data-report="income"');
    expect(html).not.toContain('reportMenu__item--active" data-report="cgt"');
  });

  it("includes Balances button", () => {
    const html = renderReportMenu();
    expect(html).toContain('data-report="balances"');
    expect(html).toContain("Balances");
  });

  it("highlights selected Balances report", () => {
    const html = renderReportMenu("balances");
    expect(html).toContain('reportMenu__item--active" data-report="balances"');
    expect(html).not.toContain('reportMenu__item--active" data-report="cgt"');
    expect(html).not.toContain('reportMenu__item--active" data-report="income"');
  });

  it("lists Tax Savings under Tax and moves Balances under Portfolio", () => {
    const html = renderReportMenu();
    expect(html).toContain('data-report="loss_harvest"');
    expect(html).toContain("Tax Savings");
    const portfolioIdx = html.indexOf("Portfolio");
    const taxSavingsIdx = html.indexOf('data-report="loss_harvest"');
    const balancesIdx = html.indexOf('data-report="balances"');
    // Tax Savings sits under the Tax heading (before Portfolio); Balances after.
    expect(taxSavingsIdx).toBeGreaterThan(-1);
    expect(taxSavingsIdx).toBeLessThan(portfolioIdx);
    expect(balancesIdx).toBeGreaterThan(portfolioIdx);
  });
});

describe("renderCgtReport", () => {
  const baseState: ReportState = {
    sidebarView: "reports",
    selectedReport: "cgt",
    selectedReportYear: 2025,
  };

  it("renders empty state when no report data", () => {
    const html = renderCgtReport(baseState);
    expect(html).toContain("reportView");
    expect(html).toContain("Select a financial year");
    expect(html.length).toBeGreaterThan(20);
  });

  it("renders report with no events", () => {
    const report: CgtReport = {
      financial_year: "2025",
      events: [],
      total_gains: 0,
      total_losses: 0,
      net_capital_gain: 0,
      short_term_gains: 0, long_term_gains: 0, total_discounted_gain: 0,
      warnings: [],
    };
    const html = renderCgtReport({ ...baseState, cgtReport: report });
    expect(html).toContain("Capital Gains Tax");
    expect(html).toContain("FY 2025");
    expect(html).toContain("No capital gains events");
    expect(html).not.toContain("<tbody>");
  });

  it("renders a long-term gain as a collapsed commodity group inside Long-Term section", () => {
    const report: CgtReport = {
      financial_year: "2025",
      events: [
        {
          sell_date: "2025-08-20",
          buy_date: "2024-01-15",
          commodity: "ETH",
          quantity: 1.0,
          cost_basis: 1000,
          sale_proceeds: 2000,
          capital_gain: 1000,
          holding_days: 583,
          discount_eligible: true,
          discounted_gain: 500,
          trade_link_id: "test-link-0",
          sell_txn_id: "txn:sell-001",
          sell_account: "assets:exchange:eth",
        },
      ],
      total_gains: 1000,
      total_losses: 0,
      net_capital_gain: 1000,
      short_term_gains: 0, long_term_gains: 0, total_discounted_gain: 500,
      warnings: [],
    };
    const html = renderCgtReport({ ...baseState, cgtReport: report });
    expect(html).toContain('data-cgt-section="long_term"');
    expect(html).toContain("Long-Term Capital Gains");
    // Group header — section-scoped key so the same commodity in different
    // sections has independent expand state
    expect(html).toContain('data-cgt-group="long_term:ETH"');
    expect(html).toContain("cgtGroup__header");
    expect(html).toContain("ETH");
    expect(html).toContain("1,000.00");
    expect(html).toContain("2,000.00");
    // Detail rows are hidden when group is collapsed
    expect(html).not.toContain("cgtGroup__detail");
    expect(html).not.toContain('data-goto-txn="txn:sell-001"');
    // Sections that have no events should not be rendered
    expect(html).not.toContain('data-cgt-section="short_term"');
    expect(html).not.toContain('data-cgt-section="losses"');
  });

  it("expands commodity group detail rows when its key is in cgtExpandedGroups", () => {
    const report: CgtReport = {
      financial_year: "2025",
      events: [
        {
          sell_date: "2025-08-20", buy_date: "2024-01-15", commodity: "ETH",
          quantity: 1, cost_basis: 1000, sale_proceeds: 2000, capital_gain: 1000,
          holding_days: 583, discount_eligible: true, discounted_gain: 500,
          trade_link_id: "test-link-0", sell_txn_id: "txn:sell-001",
          sell_account: "assets:exchange:eth",
        },
      ],
      total_gains: 1000, total_losses: 0, net_capital_gain: 1000,
      short_term_gains: 0, long_term_gains: 0, total_discounted_gain: 500, warnings: [],
    };
    const expanded = new Set(["long_term:ETH"]);
    const html = renderCgtReport({ ...baseState, cgtReport: report, cgtExpandedGroups: expanded });
    expect(html).toContain("cgtGroup__detail");
    expect(html).toContain('data-goto-txn="txn:sell-001"');
    expect(html).toContain('data-goto-txn="test-link-0"');
  });

  it("partitions events into Short-Term, Long-Term, and Losses sections", () => {
    const events = [
      // Short-term gain (held <12mo)
      {
        sell_date: "2025-08-01", buy_date: "2025-02-01", commodity: "ETH",
        quantity: 1, cost_basis: 1000, sale_proceeds: 1500, capital_gain: 500,
        holding_days: 181, discount_eligible: false, discounted_gain: 500,
        trade_link_id: "", sell_txn_id: "txn:st-001", sell_account: "a",
      },
      // Long-term gain (held >12mo)
      {
        sell_date: "2025-08-02", buy_date: "2024-01-01", commodity: "BTC",
        quantity: 0.5, cost_basis: 5000, sale_proceeds: 8000, capital_gain: 3000,
        holding_days: 583, discount_eligible: true, discounted_gain: 1500,
        trade_link_id: "", sell_txn_id: "txn:lt-001", sell_account: "a",
      },
      // Loss
      {
        sell_date: "2025-08-03", buy_date: "2024-01-15", commodity: "DOGE",
        quantity: 100, cost_basis: 200, sale_proceeds: 50, capital_gain: -150,
        holding_days: 565, discount_eligible: false, discounted_gain: -150,
        trade_link_id: "", sell_txn_id: "txn:loss-001", sell_account: "a",
      },
    ];
    const report: CgtReport = {
      financial_year: "2025", events,
      total_gains: 3500, total_losses: 150, net_capital_gain: 3350,
      short_term_gains: 0, long_term_gains: 0, total_discounted_gain: 1850, warnings: [],
    };
    const html = renderCgtReport({ ...baseState, cgtReport: report });
    expect(html).toContain('data-cgt-section="short_term"');
    expect(html).toContain('data-cgt-section="long_term"');
    expect(html).toContain('data-cgt-section="losses"');
    // Each section gets its own commodity group; same commodity in different
    // sections is keyed independently so expand state doesn't leak.
    expect(html).toContain('data-cgt-group="short_term:ETH"');
    expect(html).toContain('data-cgt-group="long_term:BTC"');
    expect(html).toContain('data-cgt-group="losses:DOGE"');
    // Section order: short-term first, then long-term, then losses
    expect(html.indexOf("Short-Term")).toBeLessThan(html.indexOf("Long-Term"));
    expect(html.indexOf("Long-Term")).toBeLessThan(html.indexOf("Capital Losses"));
  });

  it("orders commodity groups inside a section by the active sort column", () => {
    const events = [
      {
        sell_date: "2025-09-01", buy_date: "2024-01-01", commodity: "AAA",
        quantity: 10, cost_basis: 100, sale_proceeds: 200, capital_gain: 100,
        holding_days: 600, discount_eligible: true, discounted_gain: 50,
        trade_link_id: "", sell_txn_id: "", sell_account: "",
      },
      {
        sell_date: "2025-09-02", buy_date: "2024-01-02", commodity: "BBB",
        quantity: 5, cost_basis: 1000, sale_proceeds: 6000, capital_gain: 5000,
        holding_days: 600, discount_eligible: true, discounted_gain: 2500,
        trade_link_id: "", sell_txn_id: "", sell_account: "",
      },
      {
        sell_date: "2025-09-03", buy_date: "2024-01-03", commodity: "CCC",
        quantity: 1, cost_basis: 500, sale_proceeds: 1000, capital_gain: 500,
        holding_days: 600, discount_eligible: true, discounted_gain: 250,
        trade_link_id: "", sell_txn_id: "", sell_account: "",
      },
    ];
    const report: CgtReport = {
      financial_year: "2025", events,
      total_gains: 5600, total_losses: 0, net_capital_gain: 5600,
      short_term_gains: 0, long_term_gains: 0, total_discounted_gain: 2800, warnings: [],
    };
    // capital_gain desc: BBB (5000) → CCC (500) → AAA (100)
    const desc = renderCgtReport({ ...baseState, cgtReport: report, cgtSort: { column: "capital_gain", direction: "desc" } });
    expect(desc.indexOf("long_term:BBB")).toBeLessThan(desc.indexOf("long_term:CCC"));
    expect(desc.indexOf("long_term:CCC")).toBeLessThan(desc.indexOf("long_term:AAA"));
    // cost_basis asc: AAA (100) → CCC (500) → BBB (1000)
    const ascCost = renderCgtReport({ ...baseState, cgtReport: report, cgtSort: { column: "cost_basis", direction: "asc" } });
    expect(ascCost.indexOf("long_term:AAA")).toBeLessThan(ascCost.indexOf("long_term:CCC"));
    expect(ascCost.indexOf("long_term:CCC")).toBeLessThan(ascCost.indexOf("long_term:BBB"));
    // sale_proceeds desc: BBB (6000) → CCC (1000) → AAA (200)
    const descProc = renderCgtReport({ ...baseState, cgtReport: report, cgtSort: { column: "sale_proceeds", direction: "desc" } });
    expect(descProc.indexOf("long_term:BBB")).toBeLessThan(descProc.indexOf("long_term:CCC"));
    expect(descProc.indexOf("long_term:CCC")).toBeLessThan(descProc.indexOf("long_term:AAA"));
    // quantity desc: AAA (10) → BBB (5) → CCC (1)
    const descQty = renderCgtReport({ ...baseState, cgtReport: report, cgtSort: { column: "quantity", direction: "desc" } });
    expect(descQty.indexOf("long_term:AAA")).toBeLessThan(descQty.indexOf("long_term:BBB"));
    expect(descQty.indexOf("long_term:BBB")).toBeLessThan(descQty.indexOf("long_term:CCC"));
  });

  it("renders summary before the table", () => {
    const report: CgtReport = {
      financial_year: "2025",
      events: [
        {
          sell_date: "2025-08-20", buy_date: "2024-01-15", commodity: "ETH",
          quantity: 1, cost_basis: 1000, sale_proceeds: 2000, capital_gain: 1000,
          holding_days: 583, discount_eligible: true, discounted_gain: 500,
          trade_link_id: "", sell_txn_id: "", sell_account: "",
        },
      ],
      total_gains: 1000, total_losses: 0, net_capital_gain: 1000,
      short_term_gains: 0, long_term_gains: 0, total_discounted_gain: 500, warnings: [],
    };
    const html = renderCgtReport({ ...baseState, cgtReport: report });
    const summaryPos = html.indexOf("reportSummary");
    const tablePos = html.indexOf("tableCard");
    expect(summaryPos).toBeLessThan(tablePos);
  });

  it("renders FY label from taxConfig", () => {
    const report: CgtReport = {
      financial_year: "2025",
      events: [],
      total_gains: 0,
      total_losses: 0,
      net_capital_gain: 0,
      short_term_gains: 0, long_term_gains: 0, total_discounted_gain: 0,
      warnings: [],
    };
    const html = renderCgtReport({
      ...baseState,
      cgtReport: report,
      taxConfig: { financial_year_end_month: 3, financial_year_end_day: 31, cgt_discount_percent: 50, cgt_discount_holding_months: 12, non_taxable_accounts: [], non_deductible_accounts: [], marginal_tax_rate_percent: 47 },
    });
    expect(html).toContain("Apr 2024 - Mar 2025");
    expect(html).not.toContain("Jul");
  });

  it("renders warnings", () => {
    const report: CgtReport = {
      financial_year: "2025",
      events: [],
      total_gains: 0,
      total_losses: 0,
      net_capital_gain: 0,
      short_term_gains: 0, long_term_gains: 0, total_discounted_gain: 0,
      warnings: ["Unlinked trade: txn:abc on 2025-08-01"],
    };
    const html = renderCgtReport({ ...baseState, cgtReport: report });
    expect(html).toContain("Warnings");
    expect(html).toContain("Unlinked trade");
  });

  it("does not produce empty or broken HTML", () => {
    const states: ReportState[] = [
      baseState,
      { ...baseState, cgtReport: { financial_year: "2025", events: [], total_gains: 0, total_losses: 0, net_capital_gain: 0, short_term_gains: 0, long_term_gains: 0, total_discounted_gain: 0, warnings: [] } },
    ];
    for (const s of states) {
      const html = renderCgtReport(s);
      expect(html.length).toBeGreaterThan(10);
      expect(html).toContain("reportView");
    }
  });
});

describe("renderIncomeReport", () => {
  const baseState: ReportState = {
    sidebarView: "reports",
    selectedReport: "income",
    selectedReportYear: 2025,
  };

  it("renders empty state when no report data", () => {
    const html = renderIncomeReport(baseState);
    expect(html).toContain("Select a financial year");
    expect(html.length).toBeGreaterThan(20);
  });

  it("groups income events by commodity with collapsed headers", () => {
    const report: IncomeTaxReport = {
      financial_year: "2025",
      income_categories: [
        { account: "income:staking:eth", total: 2500, base_currency: "AUD" },
        { account: "income:salary",      total: 5000, base_currency: "AUD" },
      ],
      expense_categories: [
        { account: "expenses:internet", total: 80, base_currency: "AUD" },
      ],
      events: [
        { date: "2025-08-15", account: "income:staking:eth", commodity: "ETH",
          quantity: 0.25, price: 4000, value: 1000,
          base_currency: "AUD", txn_id: "txn:a", asset_account: "assets:crypto:wallet:eth" },
        { date: "2025-09-10", account: "income:staking:eth", commodity: "ETH",
          quantity: 0.5, price: 3000, value: 1500,
          base_currency: "AUD", txn_id: "txn:b", asset_account: "assets:crypto:wallet:eth" },
        { date: "2025-07-15", account: "income:salary", commodity: "AUD",
          quantity: 5000, price: 1, value: 5000,
          base_currency: "AUD", txn_id: "txn:c", asset_account: "assets:bank:checking" },
      ],
      expense_events: [
        { date: "2025-08-10", account: "expenses:internet", commodity: "AUD",
          quantity: 80, price: 1, value: 80,
          base_currency: "AUD", txn_id: "txn:d", asset_account: "assets:bank:checking" },
      ],
      total_income: 7500,
      total_expenses: 80,
      net: 7420,
      warnings: [],
    };
    const html = renderIncomeReport({ ...baseState, incomeReport: report });
    expect(html).toContain("Income Tax");
    expect(html).toContain("FY 2025");
    // Header rows for each commodity
    expect(html).toContain('data-income-group="income:ETH"');
    expect(html).toContain('data-income-group="income:AUD"');
    expect(html).toContain('data-income-group="expenses:AUD"');
    // Aggregate total appears (5000 AUD salary + 1000 + 1500 ETH-valued = 7500)
    expect(html).toContain("7,500");
    expect(html).toContain("Total Income");
    // Detail rows are NOT emitted while groups are collapsed
    expect(html).not.toContain("2025-08-15");
    expect(html).not.toContain("income:staking:eth");
  });

  it("expands a group to reveal date/quantity/price line items", () => {
    const report: IncomeTaxReport = {
      financial_year: "2025",
      income_categories: [
        { account: "income:staking:eth", total: 1500, base_currency: "AUD" },
      ],
      expense_categories: [],
      events: [
        { date: "2025-09-10", account: "income:staking:eth", commodity: "ETH",
          quantity: 0.5, price: 3000, value: 1500,
          base_currency: "AUD", txn_id: "txn:b", asset_account: "assets:crypto:wallet:eth" },
      ],
      expense_events: [],
      total_income: 1500,
      total_expenses: 0,
      net: 1500,
      warnings: [],
    };
    const expanded = new Set<string>(["income:ETH"]);
    const html = renderIncomeReport({ ...baseState, incomeReport: report, incomeExpandedGroups: expanded });
    // Detail row contents
    expect(html).toContain("2025-09-10");
    expect(html).toContain("income:staking:eth");
    // Quantity (4dp) and price (2dp)
    expect(html).toContain("0.5000");
    expect(html).toContain("3,000.00");
    // Asset-account navigation hook
    expect(html).toContain('data-goto-account="assets:crypto:wallet:eth"');
  });

  it("renders empty income report with no events", () => {
    const report: IncomeTaxReport = {
      financial_year: "2025",
      income_categories: [],
      expense_categories: [],
      events: [],
      expense_events: [],
      total_income: 0,
      total_expenses: 0,
      net: 0,
      warnings: [],
    };
    const html = renderIncomeReport({ ...baseState, incomeReport: report });
    expect(html).toContain("No income in this financial year");
    expect(html).toContain("No expenses in this financial year");
    expect(html).toContain("reportSummary");
    expect(html).toContain("<span>0</span>");
  });

  it("does not produce empty or broken HTML", () => {
    const html = renderIncomeReport(baseState);
    expect(html.length).toBeGreaterThan(10);
    expect(html).toContain("reportView");
  });
});

describe("renderTaxSettingsModal", () => {
  it("returns empty string when not open", () => {
    const html = renderTaxSettingsModal({ sidebarView: "reports" });
    expect(html).toBe("");
  });

  it("renders modal when open", () => {
    const html = renderTaxSettingsModal({
      sidebarView: "reports",
      taxSettingsOpen: true,
    });
    expect(html).toContain("Tax Settings");
    expect(html).toContain("FY end month");
    expect(html).toContain("CGT discount");
    expect(html).toContain("taxSettingsSave");
    expect(html).toContain("taxSettingsCancel");
  });

  it("renders with custom config values", () => {
    const html = renderTaxSettingsModal({
      sidebarView: "reports",
      taxSettingsOpen: true,
      taxConfig: {
        financial_year_end_month: 3,
        financial_year_end_day: 31,
        cgt_discount_percent: 33,
        cgt_discount_holding_months: 24,
        non_taxable_accounts: [],
        non_deductible_accounts: [],
        marginal_tax_rate_percent: 45,
      },
    });
    expect(html).toContain('value="3"');
    expect(html).toContain('value="31"');
    expect(html).toContain('value="33"');
    expect(html).toContain('value="24"');
  });
});

describe("renderGlobalSettingsModal", () => {
  it("returns empty string when not open", () => {
    const html = renderGlobalSettingsModal({ sidebarView: "reports" });
    expect(html).toBe("");
  });

  it("renders the included-accounts editor with current prefixes when open", () => {
    const html = renderGlobalSettingsModal({
      sidebarView: "reports",
      globalSettingsOpen: true,
      extraPrimaryAccountPrefixes: ["assets:staking", "assets:lending"],
    });
    expect(html).toContain("Global Settings");
    expect(html).toContain("Accounts included in balance");
    expect(html).toContain("assets:lending"); // only appears as a list row
    expect(html).toContain('data-prefix-remove="assets:staking"');
    expect(html).toContain('data-testid="primary-prefix-input"');
    expect(html).toContain("globalSettingsSave");
  });

  it("prefers the working draft over the persisted list", () => {
    const html = renderGlobalSettingsModal({
      sidebarView: "reports",
      globalSettingsOpen: true,
      extraPrimaryAccountPrefixes: ["assets:persistedonly"],
      globalSettingsDraft: ["assets:draftonly"],
    });
    expect(html).toContain('data-prefix-remove="assets:draftonly"');
    expect(html).not.toContain("assets:persistedonly");
  });

  it("shows the empty hint and offers only assets accounts as suggestions", () => {
    const html = renderGlobalSettingsModal({
      sidebarView: "reports",
      globalSettingsOpen: true,
      extraPrimaryAccountPrefixes: [],
      allAccounts: ["assets:crypto:wallet:eth", "income:dividends"],
    });
    expect(html).toContain("Only source-folder accounts");
    expect(html).toContain('value="assets:crypto:wallet:eth"');
    expect(html).not.toContain('value="income:dividends"');
  });
});

describe("report rendering does not blank the screen", () => {
  it("CGT report always returns non-empty HTML for any state", () => {
    const states: ReportState[] = [
      { sidebarView: "reports", selectedReport: "cgt" },
      { sidebarView: "reports", selectedReport: "cgt", selectedReportYear: 2025 },
      { sidebarView: "reports", selectedReport: "cgt", cgtReport: undefined },
      {
        sidebarView: "reports",
        selectedReport: "cgt",
        selectedReportYear: 2025,
        cgtReport: {
          financial_year: "2025",
          events: [],
          total_gains: 0,
          total_losses: 0,
          net_capital_gain: 0,
          short_term_gains: 0, long_term_gains: 0, total_discounted_gain: 0,
          warnings: [],
        },
      },
    ];
    for (const s of states) {
      const html = renderCgtReport(s);
      expect(html).toBeTruthy();
      expect(html.trim().length).toBeGreaterThan(0);
      // Should always have the reportView wrapper
      expect(html).toContain("reportView");
    }
  });

  it("Income report always returns non-empty HTML for any state", () => {
    const states: ReportState[] = [
      { sidebarView: "reports", selectedReport: "income" },
      { sidebarView: "reports", selectedReport: "income", selectedReportYear: 2025 },
      { sidebarView: "reports", selectedReport: "income", incomeReport: undefined },
      {
        sidebarView: "reports",
        selectedReport: "income",
        selectedReportYear: 2025,
        incomeReport: {
          financial_year: "2025",
          income_categories: [],
          expense_categories: [],
          events: [],
          expense_events: [],
          total_income: 0,
          total_expenses: 0,
          net: 0,
          warnings: [],
        },
      },
    ];
    for (const s of states) {
      const html = renderIncomeReport(s);
      expect(html).toBeTruthy();
      expect(html.trim().length).toBeGreaterThan(0);
      expect(html).toContain("reportView");
    }
  });

  it("Report menu always returns non-empty HTML", () => {
    for (const sel of [undefined, "cgt" as const, "income" as const]) {
      const html = renderReportMenu(sel);
      expect(html.trim().length).toBeGreaterThan(0);
      expect(html).toContain("reportMenu");
    }
  });
});

describe("renderPerformanceReport", () => {
  const makeReport = (overrides: Partial<PerformanceReport> = {}): PerformanceReport => ({
    label: "FY2026",
    date_from: "2025-07-01",
    date_to: "2026-06-30",
    base_currency: "AUD",
    base_account_scope: null,
    points: [
      { date: "2025-07-31", label: "Jul 2025", realised_gain: 0, income: 0, portfolio_value: 1200, cost_basis: 1000, unrealised_gain: 200, unrealised_change: 200 },
      { date: "2025-09-30", label: "Sep 2025", realised_gain: 600, income: 0, portfolio_value: 1500, cost_basis: 600, unrealised_gain: 900, unrealised_change: 700 },
      { date: "2026-06-30", label: "Jun 2026", realised_gain: 0, income: 50, portfolio_value: 1680, cost_basis: 600, unrealised_gain: 1080, unrealised_change: 180 },
    ],
    closing_holdings: [],
    attribution: [
      { commodity: "ETH", realised_gain: 600, unrealised_change: 1080, total: 1680, closing_value: 1680, closing_unrealised: 1080 },
    ],
    total_realised_gain: 600,
    total_income: 50,
    unrealised_change: 1080,
    value_open: 1200,
    closing_value: 1680,
    closing_cost_basis: 600,
    total_return: 1730,
    total_return_pct: 1730 / 600,
    account_breakdown: [],
    warnings: [],
    ...overrides,
  });

  const baseState: ReportState = { sidebarView: "reports", selectedReport: "performance" };

  it("shows an empty message when no report is loaded", () => {
    const html = renderPerformanceReport(baseState);
    expect(html).toContain("reportView");
    expect(html).toContain("Select a financial year");
    expect(html.trim().length).toBeGreaterThan(0);
  });

  it("omits the chart mount when there are no points", () => {
    const html = renderPerformanceReport({ ...baseState, performanceReport: makeReport({ points: [] }) });
    expect(html).toContain("reportView");
    expect(html).toContain("No performance data");
    expect(html).not.toContain('id="performanceChart"');
  });

  it("renders headline, stats, chart mount and a row per point", () => {
    const html = renderPerformanceReport({ ...baseState, performanceReport: makeReport() });
    expect(html).toContain("performanceSummary");
    expect(html).toContain("1,730");
    expect(html).toContain("Realised");
    expect(html).toContain("Income");
    expect(html).toContain("Unrealised");
    expect(html).toContain('id="performanceChart"');
    expect(html).toContain("data-morph-preserve");
    expect(html).toContain("Jul 2025");
    expect(html).toContain("Sep 2025");
    expect(html).toContain("Jun 2026");
  });

  it("renders the per-holding attribution table", () => {
    const html = renderPerformanceReport({ ...baseState, performanceReport: makeReport() });
    expect(html).toContain("Where the return came from");
    expect(html).toContain("Contribution");
    expect(html).toContain("ETH");
    expect(html).toContain("Subtotal (excl. income)");
    expect(html).toContain("= total return");
  });

  it("renders the growth-by-category chart mount when a breakdown is present", () => {
    const html = renderPerformanceReport({
      ...baseState,
      performanceReport: makeReport({
        base_account_scope: "assets",
        account_breakdown: [
          { account: "assets:crypto", values: [0, 1200, 2800] },
          { account: "assets:cash", values: [5000, 4000, 4050] },
        ],
      }),
    });
    expect(html).toContain('id="performanceGrowthChart"');
    expect(html).toContain("Growth by category — rebased to start · assets");
  });

  it("labels the growth section 'Top level' at the root scope", () => {
    const html = renderPerformanceReport({
      ...baseState,
      performanceReport: makeReport({
        base_account_scope: null,
        account_breakdown: [{ account: "assets", values: [1, 2, 3] }],
      }),
    });
    expect(html).toContain('id="performanceGrowthChart"');
    expect(html).toContain("Top level");
  });

  it("omits the growth chart mount when the breakdown is empty", () => {
    const html = renderPerformanceReport({ ...baseState, performanceReport: makeReport() });
    expect(html).not.toContain('id="performanceGrowthChart"');
  });

  it("colors a negative total return as a loss; income stays neutral", () => {
    const html = renderPerformanceReport({
      ...baseState,
      performanceReport: makeReport({ total_return: -500, total_return_pct: -0.1 }),
    });
    expect(html).toContain("performanceSummary__headline loss");
    expect(html).toContain("performanceStat__value--income");
  });

  it("renders warnings when present", () => {
    const html = renderPerformanceReport({
      ...baseState,
      performanceReport: makeReport({
        warnings: ["No AUD price for FOO as of 2026-06-30 - carried at cost"],
      }),
    });
    expect(html).toContain("reportWarnings");
    expect(html).toContain("carried at cost");
  });

  it("always returns non-empty reportView HTML for any state", () => {
    const states: ReportState[] = [
      { sidebarView: "reports", selectedReport: "performance" },
      { sidebarView: "reports", selectedReport: "performance", performanceReport: makeReport({ points: [] }) },
      { sidebarView: "reports", selectedReport: "performance", performanceReport: makeReport() },
    ];
    for (const s of states) {
      const html = renderPerformanceReport(s);
      expect(html.trim().length).toBeGreaterThan(0);
      expect(html).toContain("reportView");
    }
  });
});

describe("renderLossHarvestReport", () => {
  const makeReport = (overrides: Partial<LossHarvestReport> = {}): LossHarvestReport => ({
    as_of_date: "2026-06-30",
    financial_year: "2026",
    base_currency: "AUD",
    base_account_scope: null,
    positions: [
      { commodity: "SOL", quantity: 100, cost_basis: 30000, value: 15000, unrealised_loss: 15000, pct_below_cost: 0.5, price: 150, price_date: "2026-06-30", lots: [
        { acquisition_date: "2025-09-01", quantity: 100, cost_per_unit: 300, cost_basis: 30000, value: 15000, unrealised: -15000 },
      ] },
    ],
    total_realisable_loss: 15000,
    realised_net_gain: 10000,
    realised_short_gains: 10000,
    realised_long_gains: 0,
    offset_now: 10000,
    carry_forward: 5000,
    marginal_rate_percent: 47,
    cgt_discount_percent: 50,
    estimated_tax_saved: 4700,
    underwater_parcels: [
      { commodity: "SOL", acquisition_date: "2025-09-01", quantity: 100, cost_per_unit: 300, cost_basis: 30000, value: 15000, unrealised_loss: 15000, pct_below_cost: 0.5 },
    ],
    total_parcel_loss: 15000,
    warnings: [],
    ...overrides,
  });

  const baseState: ReportState = { sidebarView: "reports", selectedReport: "loss_harvest" };

  it("shows an empty message when no report is loaded", () => {
    const html = renderLossHarvestReport(baseState);
    expect(html).toContain("reportView");
    expect(html).toContain("Select a financial year");
  });

  it("renders the summary band and the underwater holding row", () => {
    const html = renderLossHarvestReport({ ...baseState, lossHarvestReport: makeReport() });
    expect(html).toContain("Tax Savings - as of 2026-06-30");
    expect(html).toContain("Harvestable losses");
    expect(html).toContain("Estimated tax saved this year (@47%)");
    expect(html).toContain("4,700");
    expect(html).toContain("SOL");
    expect(html).toContain("50.0%");
  });

  it("expands a holding to show its FIFO parcels with per-lot gain/loss", () => {
    const report = makeReport({
      positions: [
        { commodity: "BTC", quantity: 2, cost_basis: 200000, value: 180000, unrealised_loss: 20000, pct_below_cost: 0.1, price: 90000, price_date: "2026-06-30", lots: [
          { acquisition_date: "2024-03-01", quantity: 1, cost_per_unit: 50000, cost_basis: 50000, value: 90000, unrealised: 40000 },
          { acquisition_date: "2025-11-01", quantity: 1, cost_per_unit: 150000, cost_basis: 150000, value: 90000, unrealised: -60000 },
        ] },
      ],
    });
    // Collapsed: parcels hidden, the row is clickable.
    const collapsed = renderLossHarvestReport({ ...baseState, lossHarvestReport: report });
    expect(collapsed).toContain('data-loss-commodity="BTC"');
    expect(collapsed).not.toContain("2024-03-01");
    // Expanded: both parcels shown oldest-first, with gain/loss colouring.
    const expanded = renderLossHarvestReport({ ...baseState, lossHarvestReport: report, lossHarvestExpanded: new Set(["BTC"]) });
    expect(expanded).toContain("2024-03-01");
    expect(expanded).toContain("2025-11-01");
    expect(expanded.indexOf("2024-03-01")).toBeLessThan(expanded.indexOf("2025-11-01"));
    expect(expanded).toContain('class="num gain">+'); // the cheap early parcel is in gain
    expect(expanded).toContain('class="num loss">-'); // the recent parcel is underwater
  });

  it("parcel view lists underwater parcels even when no position is net-underwater", () => {
    // BTC sits in a net-positive holding (no position row) but has an underwater
    // parcel — it must appear in the parcel view.
    const report = makeReport({
      positions: [],
      underwater_parcels: [
        { commodity: "BTC", acquisition_date: "2025-11-01", quantity: 1, cost_per_unit: 100000, cost_basis: 100000, value: 90000, unrealised_loss: 10000, pct_below_cost: 0.1 },
      ],
      total_parcel_loss: 10000,
    });
    const parcel = renderLossHarvestReport({ ...baseState, lossHarvestReport: report, lossHarvestView: "parcel" });
    expect(parcel).toContain('data-loss-view="parcel"');
    expect(parcel).toContain("Underwater parcels");
    expect(parcel).toContain("Total underwater-parcel loss");
    expect(parcel).toContain("BTC");
    expect(parcel).toContain("2025-11-01");
    expect(parcel).toContain("specific-parcel identification");
    // Position view of the same report has no net-underwater holdings.
    const position = renderLossHarvestReport({ ...baseState, lossHarvestReport: report });
    expect(position).not.toContain("specific-parcel identification");
    expect(position).toContain("nothing to harvest");
  });

  it("shows a friendly empty state when nothing is underwater", () => {
    const html = renderLossHarvestReport({
      ...baseState,
      lossHarvestReport: makeReport({ positions: [], total_realisable_loss: 0, offset_now: 0, carry_forward: 0, estimated_tax_saved: 0 }),
    });
    expect(html).toContain("nothing to harvest");
  });

  it("renders warnings when present", () => {
    const html = renderLossHarvestReport({
      ...baseState,
      lossHarvestReport: makeReport({ warnings: ["No AUD price for FOO as of 2026-06-30"] }),
    });
    expect(html).toContain("reportWarnings");
    expect(html).toContain("No AUD price for FOO");
  });

  it("always returns non-empty reportView HTML for any state", () => {
    const states: ReportState[] = [
      baseState,
      { ...baseState, lossHarvestReport: makeReport({ positions: [] }) },
      { ...baseState, lossHarvestReport: makeReport() },
    ];
    for (const s of states) {
      const html = renderLossHarvestReport(s);
      expect(html.trim().length).toBeGreaterThan(0);
      expect(html).toContain("reportView");
    }
  });
});

describe("resolveAccountFolder", () => {
  const foldersMap: Record<string, string> = {
    "assets:ethereum": "richard/ethereum",
    "assets:savings": "richard/savings",
  };

  it("returns exact match from foldersMap", () => {
    expect(resolveAccountFolder(foldersMap, "richard", "assets:ethereum")).toBe("richard/ethereum");
  });

  it("walks up hierarchy to find parent match", () => {
    expect(resolveAccountFolder(foldersMap, "richard", "assets:ethereum:0xabc123")).toBe("richard/ethereum");
  });

  it("prepends account set in fallback when no map match", () => {
    expect(resolveAccountFolder(foldersMap, "richard", "assets:bitcoin:addr1")).toBe("richard/bitcoin/addr1");
  });

  it("does not create stray top-level folder for nested accounts", () => {
    const result = resolveAccountFolder(foldersMap, "richard", "assets:ethereum:0xabc");
    expect(result).not.toBe("ethereum/0xabc");
    expect(result.startsWith("richard/")).toBe(true);
  });

  it("works without account set (no prefix)", () => {
    expect(resolveAccountFolder(foldersMap, undefined, "assets:bitcoin:addr1")).toBe("bitcoin/addr1");
  });

  it("works with empty foldersMap", () => {
    expect(resolveAccountFolder({}, "richard", "assets:ethereum:0xabc")).toBe("richard/ethereum/0xabc");
  });
});

describe("renderSortHeader", () => {
  it("renders inactive header with no sort state", () => {
    const html = renderSortHeader("Date", "date");
    expect(html).toContain('data-sort-col="date"');
    expect(html).toContain("sortable");
    expect(html).not.toContain("sortable--active");
    expect(html).not.toContain("\u25B2");
    expect(html).not.toContain("\u25BC");
  });

  it("renders active asc header", () => {
    const html = renderSortHeader("Date", "date", { column: "date", direction: "asc" });
    expect(html).toContain("sortable--active");
    expect(html).toContain("\u25B2");
  });

  it("renders active desc header", () => {
    const html = renderSortHeader("Date", "date", { column: "date", direction: "desc" });
    expect(html).toContain("sortable--active");
    expect(html).toContain("\u25BC");
  });

  it("renders inactive when sort is on different column", () => {
    const html = renderSortHeader("Date", "date", { column: "amount", direction: "asc" });
    expect(html).not.toContain("sortable--active");
  });

  it("includes extraClass", () => {
    const html = renderSortHeader("Amount", "amount", undefined, "num");
    expect(html).toContain("num");
  });

  it("includes data-sort-scope when provided", () => {
    const html = renderSortHeader("Date", "sell_date", undefined, undefined, "cgt");
    expect(html).toContain('data-sort-scope="cgt"');
  });

  it("omits data-sort-scope when not provided", () => {
    const html = renderSortHeader("Date", "date");
    expect(html).not.toContain("data-sort-scope");
  });
});

describe("renderCgtReport sorting and filtering", () => {
  const mkEvent = (commodity: string, sell_date: string, capital_gain: number): CgtReport["events"][0] => ({
    sell_date, buy_date: "2024-01-01", commodity, quantity: 1,
    cost_basis: 1000, sale_proceeds: 1000 + capital_gain, capital_gain,
    holding_days: 365, discount_eligible: true, discounted_gain: capital_gain * 0.5,
    trade_link_id: "", sell_txn_id: "", sell_account: "assets:exchange",
  });

  const baseState: ReportState = {
    sidebarView: "reports",
    selectedReport: "cgt",
    selectedReportYear: 2025,
    cgtReport: {
      financial_year: "2025",
      events: [
        mkEvent("BTC", "2025-09-01", 500),
        mkEvent("ETH", "2025-08-01", -200),
        mkEvent("SOL", "2025-10-01", 1000),
      ],
      total_gains: 1500, total_losses: 200,
      net_capital_gain: 1300, short_term_gains: 0, long_term_gains: 0, total_discounted_gain: 650,
      warnings: [],
    },
  };

  it("renders sort arrows on CGT columns", () => {
    const html = renderCgtReport({ ...baseState, cgtSort: { column: "commodity", direction: "asc" } });
    expect(html).toContain('data-sort-scope="cgt"');
    // The Asset column should have the active arrow
    expect(html).toContain("Asset \u25B2");
  });

  it("sorts events by commodity ascending within each section", () => {
    // BTC and SOL are gains → Short-Term section; ETH is a loss → Losses section.
    // Within Short-Term, ascending commodity sort puts BTC before SOL.
    const html = renderCgtReport({ ...baseState, cgtSort: { column: "commodity", direction: "asc" } });
    const stIdx = html.indexOf('data-cgt-section="short_term"');
    const lossIdx = html.indexOf('data-cgt-section="losses"');
    const btcIdx = html.indexOf("BTC");
    const solIdx = html.indexOf("SOL");
    expect(btcIdx).toBeGreaterThan(stIdx);
    expect(btcIdx).toBeLessThan(solIdx);
    expect(solIdx).toBeLessThan(lossIdx);
  });

  it("sorts events by capital_gain descending", () => {
    const html = renderCgtReport({ ...baseState, cgtSort: { column: "capital_gain", direction: "desc" } });
    const solIdx = html.indexOf("SOL");
    const btcIdx = html.indexOf("BTC");
    const ethIdx = html.indexOf("ETH");
    expect(solIdx).toBeLessThan(btcIdx);
    expect(btcIdx).toBeLessThan(ethIdx);
  });

  it("filters events by commodity", () => {
    const html = renderCgtReport({ ...baseState, cgtFilterText: "eth" });
    expect(html).toContain("ETH");
    expect(html).not.toContain("BTC");
    expect(html).not.toContain("SOL");
  });

  it("filters events by account", () => {
    const events = [
      { ...mkEvent("BTC", "2025-09-01", 500), sell_account: "assets:coinbase" },
      { ...mkEvent("ETH", "2025-08-01", -200), sell_account: "assets:kraken" },
    ];
    const report = { ...baseState.cgtReport!, events };
    const html = renderCgtReport({ ...baseState, cgtReport: report, cgtFilterText: "kraken" });
    expect(html).toContain("ETH");
    expect(html).not.toContain("BTC");
  });

  it("shows filtered count in pagination", () => {
    // Create 60 events, filter to match only some
    const events = Array.from({ length: 60 }, (_, i) =>
      mkEvent(i < 30 ? "BTC" : "ETH", `2025-08-${String((i % 28) + 1).padStart(2, "0")}`, i * 10)
    );
    const report = { ...baseState.cgtReport!, events };
    const html = renderCgtReport({ ...baseState, cgtReport: report, cgtFilterText: "ETH" });
    // Should show 30 ETH events (all fit in default page size 50)
    expect(html).not.toContain("reportShowMore");
    expect(html).not.toContain("BTC");
  });

  it("summary totals unchanged when filtering", () => {
    const html = renderCgtReport({ ...baseState, cgtFilterText: "eth" });
    // Summary should still show full report totals
    expect(html).toContain("1,500"); // total gains
    expect(html).toContain("1,300"); // net capital gain
  });

  it("renders filter input with current text", () => {
    const html = renderCgtReport({ ...baseState, cgtFilterText: "hello" });
    expect(html).toContain('id="cgtFilter"');
    expect(html).toContain('value="hello"');
  });
});

describe("renderIncomeReport sorting", () => {
  const baseState: ReportState = {
    sidebarView: "reports",
    selectedReport: "income",
    selectedReportYear: 2025,
    incomeReport: {
      financial_year: "2025",
      income_categories: [],
      expense_categories: [],
      events: [
        { date: "2025-08-15", account: "income:staking:eth", commodity: "ETH",
          quantity: 0.25, price: 4000, value: 1000,
          base_currency: "AUD", txn_id: "txn:a", asset_account: "assets:crypto:wallet:eth" },
        { date: "2025-09-10", account: "income:staking:eth", commodity: "ETH",
          quantity: 0.5, price: 3000, value: 1500,
          base_currency: "AUD", txn_id: "txn:b", asset_account: "assets:crypto:wallet:eth" },
        { date: "2025-10-05", account: "income:mining:btc", commodity: "BTC",
          quantity: 0.001, price: 100000, value: 100,
          base_currency: "AUD", txn_id: "txn:c", asset_account: "assets:crypto:wallet:btc" },
        { date: "2025-07-15", account: "income:salary", commodity: "AUD",
          quantity: 5000, price: 1, value: 5000,
          base_currency: "AUD", txn_id: "txn:d", asset_account: "assets:bank:checking" },
      ],
      expense_events: [],
      total_income: 7600,
      total_expenses: 0,
      net: 7600,
      warnings: [],
    },
  };

  it("renders sort headers on income columns", () => {
    const html = renderIncomeReport(baseState);
    expect(html).toContain('data-sort-scope="income"');
    expect(html).toContain('data-sort-col="date"');
    expect(html).toContain('data-sort-col="account"');
    expect(html).toContain('data-sort-col="quantity"');
    expect(html).toContain('data-sort-col="price"');
    expect(html).toContain('data-sort-col="value"');
  });

  it("sorts groups by aggregate value descending", () => {
    const html = renderIncomeReport({ ...baseState, incomeSort: { column: "value", direction: "desc" } });
    // AUD: 5000, ETH: 2500, BTC: 100
    const audIdx = html.indexOf('data-income-group="income:AUD"');
    const ethIdx = html.indexOf('data-income-group="income:ETH"');
    const btcIdx = html.indexOf('data-income-group="income:BTC"');
    expect(audIdx).toBeGreaterThanOrEqual(0);
    expect(audIdx).toBeLessThan(ethIdx);
    expect(ethIdx).toBeLessThan(btcIdx);
  });

  it("sorts groups by commodity name ascending", () => {
    const html = renderIncomeReport({ ...baseState, incomeSort: { column: "commodity", direction: "asc" } });
    const audIdx = html.indexOf('data-income-group="income:AUD"');
    const btcIdx = html.indexOf('data-income-group="income:BTC"');
    const ethIdx = html.indexOf('data-income-group="income:ETH"');
    expect(audIdx).toBeLessThan(btcIdx);
    expect(btcIdx).toBeLessThan(ethIdx);
  });

  it("shows sort arrow on active column", () => {
    const html = renderIncomeReport({ ...baseState, incomeSort: { column: "value", direction: "asc" } });
    expect(html).toContain("Value \u25B2");
  });
});

describe("renderNavButtons", () => {
  it("renders both buttons disabled when no history", () => {
    const html = renderNavButtons({ sidebarView: "accounts" });
    expect(html).toContain('id="navBack"');
    expect(html).toContain('id="navForward"');
    expect(html).toContain("disabled");
  });

  it("renders back enabled when navCanGoBack is true", () => {
    const html = renderNavButtons({ sidebarView: "accounts", navCanGoBack: true });
    // Back button should NOT be disabled
    const backBtn = html.match(/<button[^>]*id="navBack"[^>]*>/)?.[0] ?? "";
    expect(backBtn).not.toContain("disabled");
    // Forward should still be disabled
    const fwdBtn = html.match(/<button[^>]*id="navForward"[^>]*>/)?.[0] ?? "";
    expect(fwdBtn).toContain("disabled");
  });

  it("renders forward enabled when navCanGoForward is true", () => {
    const html = renderNavButtons({ sidebarView: "accounts", navCanGoForward: true });
    const backBtn = html.match(/<button[^>]*id="navBack"[^>]*>/)?.[0] ?? "";
    expect(backBtn).toContain("disabled");
    const fwdBtn = html.match(/<button[^>]*id="navForward"[^>]*>/)?.[0] ?? "";
    expect(fwdBtn).not.toContain("disabled");
  });

  it("renders both enabled when both flags are true", () => {
    const html = renderNavButtons({ sidebarView: "accounts", navCanGoBack: true, navCanGoForward: true });
    const backBtn = html.match(/<button[^>]*id="navBack"[^>]*>/)?.[0] ?? "";
    const fwdBtn = html.match(/<button[^>]*id="navForward"[^>]*>/)?.[0] ?? "";
    expect(backBtn).not.toContain("disabled");
    expect(fwdBtn).not.toContain("disabled");
  });

  it("includes keyboard shortcut titles", () => {
    const html = renderNavButtons({ sidebarView: "accounts" });
    expect(html).toContain("Alt+Left");
    expect(html).toContain("Alt+Right");
  });
});

describe("nav buttons in report headers", () => {
  const baseReport: CgtReport = {
    financial_year: "2025", events: [],
    total_gains: 0, total_losses: 0, net_capital_gain: 0,
    short_term_gains: 0, long_term_gains: 0, total_discounted_gain: 0, warnings: [],
  };

  it("CGT report includes nav buttons", () => {
    const html = renderCgtReport({
      sidebarView: "reports", selectedReport: "cgt",
      cgtReport: baseReport, navCanGoBack: true,
    });
    expect(html).toContain('id="navBack"');
    expect(html).toContain('id="navForward"');
  });

  it("income report includes nav buttons", () => {
    const html = renderIncomeReport({
      sidebarView: "reports", selectedReport: "income",
      incomeReport: {
        financial_year: "2025", income_categories: [], expense_categories: [],
        events: [], expense_events: [],
        total_income: 0, total_expenses: 0, net: 0, warnings: [],
      },
      navCanGoBack: true,
    });
    expect(html).toContain('id="navBack"');
    expect(html).toContain('id="navForward"');
  });
});

describe("shortAddress", () => {
  it("returns short names unchanged", () => {
    expect(shortAddress("Bybit")).toBe("Bybit");
    expect(shortAddress("Self Transfer")).toBe("Self Transfer");
  });

  it("truncates long addresses", () => {
    const addr = "0x1849964c441d9720979f74c2e688709680264ab6";
    const short = shortAddress(addr);
    expect(short.length).toBeLessThan(addr.length);
    expect(short).toContain("0x1849964c");
  });

  it("differentiates address-poisoning lookalikes", () => {
    // These addresses share 0x1849 prefix and 4ab6 suffix — a real
    // address-poisoning attack.  shortAddress must produce distinct strings.
    const addrs = [
      "0x1849964c441d9720979f74c2e688709680264ab6",
      "0x18491e6178053a66eb8c4a1bc759243831474ab6",
      "0x1849ffa8642b8f9baf3ebc9ed50d068c160b4ab6",
      "0x1849e37eabcedeb0f8b04f44ff3c2976eb8a4ab6",
    ];
    const shorts = addrs.map(shortAddress);
    const unique = new Set(shorts);
    expect(unique.size).toBe(addrs.length);
  });
});

describe("renderBalancesReport", () => {
  const baseState: ReportState = {
    sidebarView: "reports",
    selectedReport: "balances",
  };

  const sampleReport: BalancesReport = {
    as_of_date: "2026-06-30",
    base_currency: "AUD",
    base_account_scope: "assets:crypto",
    holdings: [
      { commodity: "BTC", quantity: 0.5, price: 50000, price_date: "2025-12-31", value: 25000, portfolio_weight: 0.7352941176, accounts: [
        { account: "assets:crypto:ledger", quantity: 0.3, value: 15000 },
        { account: "assets:crypto:coinbase", quantity: 0.2, value: 10000 },
      ] },
      { commodity: "ETH", quantity: 2.0, price: 4500, price_date: "2025-12-31", value: 9000, portfolio_weight: 0.2647058823, accounts: [
        { account: "assets:crypto:metamask", quantity: 2.0, value: 9000 },
      ] },
    ],
    total_value: 34000,
    warnings: [],
  };

  it("shows empty-state message when no report is loaded", () => {
    const html = renderBalancesReport(baseState);
    expect(html).toContain("Select a financial year");
  });

  it("renders the summary card with total, count, and as-of date", () => {
    const html = renderBalancesReport({ ...baseState, balancesReport: sampleReport });
    expect(html).toContain("Total value (AUD)");
    expect(html).toContain("34,000");
    expect(html).toContain("2 coins");
    expect(html).toContain("as of 2026-06-30");
  });

  it("renders one row per holding with quantity, price, value, and weight%", () => {
    const html = renderBalancesReport({ ...baseState, balancesReport: sampleReport });
    expect(html).toContain(">BTC<");
    expect(html).toContain(">ETH<");
    expect(html).toContain("0.5000"); // BTC quantity
    expect(html).toContain("50,000.00"); // BTC price
    expect(html).toContain("25,000.00"); // BTC value
    // Weight percentages rounded to 1dp
    expect(html).toContain("73.5%");
    expect(html).toContain("26.5%");
  });

  it("sets the balanceWeightBar inline width from portfolio_weight", () => {
    const html = renderBalancesReport({ ...baseState, balancesReport: sampleReport });
    expect(html).toMatch(/balanceWeightBar__track"><span class="balanceWeightBar" style="width:73\.53%"/);
    expect(html).toMatch(/balanceWeightBar__track"><span class="balanceWeightBar" style="width:26\.47%"/);
  });

  it("renders warnings section when warnings are present", () => {
    const withWarnings: BalancesReport = {
      ...sampleReport,
      warnings: ["No AUD price for SPAM as of 2026-06-30"],
    };
    const html = renderBalancesReport({ ...baseState, balancesReport: withWarnings });
    expect(html).toContain("Warnings");
    expect(html).toContain("SPAM");
  });

  it("omits warnings section when there are no warnings", () => {
    const html = renderBalancesReport({ ...baseState, balancesReport: sampleReport });
    expect(html).not.toContain("reportWarnings__title");
  });

  it("filters holdings by commodity substring", () => {
    const html = renderBalancesReport({
      ...baseState,
      balancesReport: sampleReport,
      balancesFilterText: "btc",
    });
    expect(html).toContain(">BTC<");
    expect(html).not.toContain(">ETH<");
  });

  it("applies sort when balancesSort is set", () => {
    const html = renderBalancesReport({
      ...baseState,
      balancesReport: sampleReport,
      balancesSort: { column: "commodity", direction: "asc" },
    });
    // BTC should appear before ETH in the rendered table body with ascending commodity sort.
    const btcIdx = html.indexOf(">BTC<");
    const ethIdx = html.indexOf(">ETH<");
    expect(btcIdx).toBeGreaterThan(-1);
    expect(ethIdx).toBeGreaterThan(-1);
    expect(btcIdx).toBeLessThan(ethIdx);
  });

  it("emits an empty-state message when holdings is empty", () => {
    const emptyReport: BalancesReport = { ...sampleReport, holdings: [], total_value: 0 };
    const html = renderBalancesReport({ ...baseState, balancesReport: emptyReport });
    expect(html).toContain("No holdings at this date");
  });

  it("commodity row carries data-balance-commodity for click handler", () => {
    const html = renderBalancesReport({ ...baseState, balancesReport: sampleReport });
    expect(html).toContain('data-balance-commodity="BTC"');
    expect(html).toContain('aria-expanded="false"');
  });

  it("does not render account sub-rows when commodity is collapsed", () => {
    const html = renderBalancesReport({ ...baseState, balancesReport: sampleReport });
    expect(html).not.toContain("assets:crypto:ledger");
    expect(html).not.toContain("balanceAccountRow");
  });

  it("renders per-account breakdown rows when commodity is expanded", () => {
    const html = renderBalancesReport({
      ...baseState,
      balancesReport: sampleReport,
      balancesExpanded: new Set(["BTC"]),
    });
    expect(html).toContain('aria-expanded="true"');
    expect(html).toContain("balanceAccountRow");
    // Paths longer than the 18-char column budget get the leaf-priority
    // truncation; full path is preserved on hover via the title attribute.
    expect(html).toContain(">…ets:crypto:ledger<");
    expect(html).toContain(">…s:crypto:coinbase<");
    expect(html).toContain('title="assets:crypto:ledger"');
    // ETH still collapsed — its account row should not appear
    expect(html).not.toContain("metamask");
  });

  it("omits chevron control when a commodity has no per-account breakdown", () => {
    const noAccounts: BalancesReport = {
      ...sampleReport,
      holdings: [{ ...sampleReport.holdings[0]!, accounts: [] }],
    };
    const html = renderBalancesReport({ ...baseState, balancesReport: noAccounts });
    expect(html).not.toContain("data-balance-commodity");
  });

  it("escapes HTML in the base currency and date", () => {
    const malicious: BalancesReport = {
      ...sampleReport,
      as_of_date: "<script>",
      base_currency: "A\"UD",
    };
    const html = renderBalancesReport({ ...baseState, balancesReport: malicious });
    expect(html).not.toContain("<script>"); // verbatim
    expect(html).toContain("&lt;script&gt;");
    expect(html).toContain("A&quot;UD");
  });
});

describe("renderReportScope", () => {
  it("renders an empty scope input when state has no scope set", () => {
    const html = renderReportScope({ sidebarView: "reports" } as any);
    expect(html).toContain("reportScope");
    expect(html).toContain("id=\"reportBaseScope\"");
    expect(html).toContain("value=\"\"");
  });
  it("renders the configured scope value", () => {
    const html = renderReportScope({ sidebarView: "reports", reportBaseScope: "assets:crypto" } as any);
    expect(html).toContain("value=\"assets:crypto\"");
  });
});
