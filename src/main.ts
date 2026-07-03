import "./style.css";
import { morphInPlace } from "./dom-update";
import { bindOnceDuring } from "./bind-once";
import { ROW_AI_SPARKLE_SELECTOR } from "./ai-sparkle";
import { buildSaveRuleArgs } from "./save-rule-args";
import { invoke, listen, open, confirm, save } from "./ipc";
import { APP_NAME, APP_TITLE } from "./app-config";
import type {
  Posting, Transaction, CommodityAmount, AccountBalance, AccountProperties,
  Diagnostic, ParseResponse, QueryResult, PipelineResult, PipelineResponse,
  TradeLink, TradeSuggestion,
  CgtEvent, CgtReport, TaxCategory, IncomeTaxReport, TaxConfig,
  BalancesReport, BalancesSortColumn, LossHarvestReport, PerformanceReport,
  GenericSortState, CgtSortColumn, IncomeSortColumn, SortColumn, SortState,
} from "./types";
import {
  renderMarkdownReport as _renderMarkdownReport,
  renderCgtReport as _renderCgtReport,
  renderIncomeReport as _renderIncomeReport,
  renderBalancesReport as _renderBalancesReport,
  renderPerformanceReport as _renderPerformanceReport,
  renderLossHarvestReport as _renderLossHarvestReport,
  renderTaxSettingsModal as _renderTaxSettingsModal,
  renderGlobalSettingsModal as _renderGlobalSettingsModal,
  renderReportMenu,
  renderReportScope,
  renderRebuildStrip,
  renderSortHeader,
  renderNavButtons,
  withFadeInClass,
  currentFinancialYear,
} from "./render";
import { marked } from "marked";
import { accountPrefixForScope, filterByAccountPrefix } from "./account-search";

import { buildManualPostings, computeManualBalance, formatCash, resolveAccountFolder as _resolveAccountFolder, resolveAccountFolderFromMap, folderToAccountName, shortAddress, resetStateForAccountNavigation, resetStateForCategoryNavigation, detectTradePairs, detectGroupTradePairs, sortTxnIdsByAbsAmount, type ManualBalance } from "./account-utils";
import { backendSortForTransactionWindow, shouldClientSortLoadedTransactions } from "./query-sort";
import { filterSidebarBalances, filterCategoryBalances } from "./sidebar-accounts";
import { ROOT_FOLDER_PATH, folderDisplayName, folderHeaderTotal, formatFolderAmount, isFolderSelection } from "./folder-view";
import { paginateWithGroups, buildTxnIdMap, type TxGroup, type TxRowItem } from "./tx-grouping";
import { normalStartup as _normalStartup } from "./startup";
import { displayLegs } from "./transaction-legs";
import { computeTransactionValues } from "./transaction-values";
import { wildcardMatch, computeRulePreviewFromDraft, extractRuleConfigPills, draftConditionsToPills, isDefaultRule, buildRuleFromDraft, categoryEditRuleMatch, derivePatternAndPayeeCondition, mergeRuleConditions, type RuleInfo, type PreviewMatch, type AmountCondition } from "./rules";
import { collectAccountSuggestions } from "./account-suggestions";
import { type SearchPill, parseSmartInput, toFlatString, renderSmartSearch, attachSmartSearch } from "./smart-search";
import { collectIssues, buildAccountIssueCounts, type IssueGroup, type AccountGap } from "./issues";
import { loadTreeBaseCurrencyTotals } from "./currency-totals";
import { applyPipelineResponse, applyMutationResponse, applyParse, type MutationResponse } from "./apply-pipeline";
import { MutationQueue } from "./mutation-queue";
import {
  buildOptimisticTransaction,
  filterPendingDeletes,
  nextOptimisticTempId,
  pendingAddsForView,
  pruneExpandedKeysForTxn,
  resetSearchPaging,
} from "./optimistic";
import { attachAccountInput, type AccountInputController } from "./account-input";
import { formatAmount as formatAmountPure, formatAmountSmart, type DisplayConfig } from "./format";
import {
  extractRuleId,
  legIdFromMeta,
  matchesPrefix,
  metaSegmentValue,
  swapPartnerRefFromMeta,
  txnIdFromMeta,
  txnValueKey,
} from "./meta";
import { displayName, escapeText, highlightNonAscii } from "./text";
import { type DateFilter, dateFilterStart, nowYYYYMM, nowYYYYMMDD, fyDateRange } from "./date-helpers";
import { syncPerformanceChart, syncPerformanceGrowthChart } from "./performance-chart";

function formatAmount(amount: number, commodity: string): string {
  return formatAmountPure(amount, commodity, state.displayConfig);
}
/** Valid search keywords — must match Rust query::SEARCH_FIELDS */
const TRANSACTION_SEARCH_KEYWORDS = ["payee", "narration", "meta", "date", "account", "commodity", "amount", "fee", "is"] as const;

/** Build search string from pills + free text, joining with AND. */
function searchFromPills(pills: SearchPill[], text: string): string {
  const parts = pills.map((p) => `${p.negated ? "-" : ""}${p.key}:${p.value}`);
  if (text) parts.push(text);
  return parts.join(" AND ");
}

/** Filter transactions via the Rust query engine. accountPrefix is prepended as account:PREFIX AND ... */
async function filterBySearch(accountSet: string, pills: SearchPill[], accountPrefix?: string): Promise<Transaction[]> {
  if (pills.length === 0 && !accountPrefix) return [];
  const parts: string[] = [];
  if (accountPrefix) parts.push(`account:${accountPrefix}`);
  const pillStr = searchFromPills(pills, "");
  if (pillStr) parts.push(pillStr);
  const search = parts.join(" AND ");
  try {
    const result = await invoke<QueryResult>("query_search", { accountSet, search });
    return result.transactions;
  } catch {
    return [];
  }
}
import { TX_WINDOW } from "./virtual-scroll";

// Module-level cache for virtual-scroll tbody re-renders (scroll handler needs these)
let vsItems: TxRowItem[] = [];
let vsCleanup: (() => void) | null = null;

// Smart search cleanup functions (called before re-attaching on render)
let cleanupAccountSearch: (() => void) | null = null;
let cleanupRuleSearch: (() => void) | null = null;

type PriceImportResponse = { commodities: string[]; total_count: number };

type SuggestTransformResponse = {
  needs_transform: boolean;
  suggestion: string | null;
  csv_filename: string;
  headers: string[];
};

type AiRuleSuggestion = {
  pattern: string;
  amount_account: string;
  payee?: string;
  match_field?: string;
  explanation: string;
};

type AiSuggestResponse = {
  suggestions: AiRuleSuggestion[];
  raw_output: string;
  success: boolean;
  error?: string;
};

type CsvTypeInfo = {
  pattern: string;
  headers: string[];
  file_count: number;
  sample_rows: string[][];
};

type AiTransformResponse = {
  script: string;
  raw_output: string;
  success: boolean;
  error?: string;
  csv_types: CsvTypeInfo[];
};

// ── Plugin types ──

type PluginConfigField = {
  type: string;
  default?: unknown;
  description?: string;
  required?: boolean;
};

type PluginManifest = {
  plugin: { name: string; version: string; description?: string; script: string; daily?: boolean };
  config: Record<string, PluginConfigField>;
  secrets: Record<string, PluginConfigField>;
};

type PluginInfo = {
  dir_name: string;
  dir: string;
  manifest: PluginManifest;
  last_run?: string;
  last_status?: string;
};

type PluginRunResult = {
  success: boolean;
  stdout: string;
  stderr: string;
  duration_ms: number;
};

type DailyPluginOutcome = {
  dir_name: string;
  name: string;
  success: boolean;
  duration_ms: number;
  skipped_ran_today: boolean;
  // 0 = updated, 2 = updated with warnings, anything else / null = failed.
  exit_code?: number | null;
};

type DailyRunSummary = { outcomes: DailyPluginOutcome[] };

function renderPluginField(key: string, field: PluginConfigField, value: unknown, isSecret: boolean): string {
  const desc = field.description ? `<span class="pluginCfg__hint">${escapeText(field.description)}</span>` : "";
  const label = `<label class="pluginCfg__label">${escapeText(key)}${field.required ? " *" : ""}${desc}</label>`;
  let input: string;
  if (isSecret) {
    input = `<input class="pluginCfg__input" type="password" autocomplete="off" data-secret-field="${escapeText(key)}" value="${escapeText(String(value ?? ""))}" placeholder="not set" />`;
  } else if (field.type === "boolean") {
    input = `<label class="pluginCfg__check"><input type="checkbox" data-cfg-field="${escapeText(key)}" data-cfg-type="boolean" ${value ? "checked" : ""} /> enabled</label>`;
  } else if (field.type === "integer" || field.type === "number") {
    input = `<input class="pluginCfg__input" type="number" data-cfg-field="${escapeText(key)}" data-cfg-type="${escapeText(field.type)}" value="${escapeText(String(value ?? ""))}" />`;
  } else {
    input = `<input class="pluginCfg__input" type="text" data-cfg-field="${escapeText(key)}" data-cfg-type="string" value="${escapeText(String(value ?? ""))}" />`;
  }
  return `<div class="pluginCfg__row">${label}${input}</div>`;
}

function renderPluginConfigPanel(p: PluginInfo, state: AppState): string {
  const cfg = p.manifest.config ?? {};
  const sec = p.manifest.secrets ?? {};
  const cfgVals = state.pluginConfigValues ?? {};
  const secVals = state.pluginSecretValues ?? {};
  const rows = [
    ...Object.entries(cfg).map(([k, f]) => renderPluginField(k, f, k in cfgVals ? cfgVals[k] : f.default, false)),
    ...Object.entries(sec).map(([k, f]) => renderPluginField(k, f, secVals[k] ?? "", true)),
  ];
  if (rows.length === 0) return "";
  const saved = state.pluginConfigSaved === p.dir_name ? `<span class="pluginCfg__saved">Saved ✓</span>` : "";
  return `<div class="pluginCfg">${rows.join("")}<div class="pluginCfg__actions">`
    + `<button class="btn btn--secondary pluginCfg__btn" data-cancel-plugin-config="1" type="button">Close</button>`
    + `<button class="btn pluginCfg__btn" data-save-plugin-config="${escapeText(p.dir_name)}" type="button" ${state.pluginConfigSaving ? "disabled" : ""}>${state.pluginConfigSaving ? "Saving…" : "Save"}</button>`
    + `${saved}</div></div>`;
}

function renderPluginItem(p: PluginInfo, state: AppState): string {
  const running = state.pluginRunning === p.dir_name;
  const hasConfig = Object.keys(p.manifest.config ?? {}).length > 0 || Object.keys(p.manifest.secrets ?? {}).length > 0;
  const open = state.pluginConfigOpen === p.dir_name;
  const badge = p.manifest.plugin.daily ? ` <span class="pluginItem__badge" data-testid="plugin-daily-badge">Daily</span>` : "";
  const desc = p.manifest.plugin.description ? `<div class="pluginItem__desc">${escapeText(p.manifest.plugin.description)}</div>` : "";
  const meta = `<div class="pluginItem__meta">v${escapeText(p.manifest.plugin.version)}${p.last_run ? ` · ${p.last_status === "success" ? "OK" : "Failed"} · ${new Date(p.last_run).toLocaleDateString()}` : ""}</div>`;
  const configBtn = hasConfig ? `<button class="btn btn--secondary pluginItem__config" data-config-plugin="${escapeText(p.dir_name)}" type="button">${open ? "Close" : "Configure"}</button>` : "";
  return `<div class="pluginItem ${running ? "pluginItem--running" : ""}" data-plugin="${escapeText(p.dir_name)}">`
    + `<div class="pluginItem__name">${escapeText(p.manifest.plugin.name)}${badge}</div>${desc}${meta}`
    + `<div class="pluginItem__actions">`
    + `<button class="btn btn--secondary pluginItem__run" data-run-plugin="${escapeText(p.dir_name)}" ${state.pluginRunning ? "disabled" : ""}>${running ? "Running..." : "Run"}</button>`
    + `${configBtn}</div>`
    + `${open ? renderPluginConfigPanel(p, state) : ""}`
    + `</div>`;
}

// Sentinel stored in state.pluginRunning while the daily price-sync batch runs,
// so the existing per-plugin disabled/running logic applies unchanged.
const DAILY_SYNC_SENTINEL = "__daily__";

// ── Navigation history ──

type NavEntry = {
  sidebarView: "accounts" | "categories" | "reports" | "rule-editor" | "plugins";
  selectedAccount?: string;
  drillPath?: string[];
  selectedCategory?: string;
  categoryDrillPath?: string[];
  selectedReport?: "cgt" | "income" | "balances" | "performance" | "loss_harvest";
  selectedReportYear?: number;
  search?: string;
  searchPills?: SearchPill[];
  searchText?: string;
  txSort?: SortState;
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
};

const NAV_STORAGE_KEY = "arimalo_navState";
const navHistory: NavEntry[] = [];
let navIndex = -1;
let navNavigating = false; // true while restoring from history (prevents re-push)

function captureNav(state: { [K in keyof NavEntry]: unknown }): NavEntry {
  return {
    sidebarView: state.sidebarView as NavEntry["sidebarView"],
    selectedAccount: state.selectedAccount as string | undefined,
    drillPath: state.drillPath ? [...(state.drillPath as string[])] : undefined,
    selectedCategory: state.selectedCategory as string | undefined,
    categoryDrillPath: state.categoryDrillPath ? [...(state.categoryDrillPath as string[])] : undefined,
    selectedReport: state.selectedReport as NavEntry["selectedReport"],
    selectedReportYear: state.selectedReportYear as number | undefined,
    search: state.search as string | undefined,
    searchPills: state.searchPills ? [...(state.searchPills as SearchPill[])] : undefined,
    searchText: state.searchText as string | undefined,
    txSort: state.txSort as SortState | undefined,
    cgtSort: state.cgtSort as GenericSortState<CgtSortColumn> | undefined,
    cgtFilterText: state.cgtFilterText as string | undefined,
    incomeSort: state.incomeSort as GenericSortState<IncomeSortColumn> | undefined,
    incomeFilterText: state.incomeFilterText as string | undefined,
    balancesSort: state.balancesSort as GenericSortState<BalancesSortColumn> | undefined,
    balancesFilterText: state.balancesFilterText as string | undefined,
  };
}

function pushNav(state: { [K in keyof NavEntry]: unknown }): void {
  if (navNavigating) return;
  const entry = captureNav(state);
  // Deduplicate: don't push if identical to current entry
  if (navIndex >= 0) {
    const cur = navHistory[navIndex];
    if (cur.sidebarView === entry.sidebarView &&
        cur.selectedAccount === entry.selectedAccount &&
        cur.selectedCategory === entry.selectedCategory &&
        cur.selectedReport === entry.selectedReport &&
        cur.selectedReportYear === entry.selectedReportYear &&
        cur.search === entry.search) {
      return;
    }
  }
  // Truncate forward history
  navHistory.splice(navIndex + 1);
  navHistory.push(entry);
  navIndex = navHistory.length - 1;
  saveNavState(entry);
}

function applyNavEntry(entry: NavEntry, state: Record<string, unknown>): void {
  state.sidebarView = entry.sidebarView;
  state.selectedAccount = entry.selectedAccount;
  state.drillPath = entry.drillPath ?? [];
  state._drillInitialized = true; // don't auto-drill, we have the saved path
  state.selectedCategory = entry.selectedCategory;
  state.categoryDrillPath = entry.categoryDrillPath ?? [];
  state._categoryDrillInitialized = true;
  state.selectedReport = entry.selectedReport;
  state.selectedReportYear = entry.selectedReportYear;
  state.search = entry.search ?? "";
  state.searchPills = entry.searchPills ?? [];
  state.searchText = entry.searchText ?? "";
  state.txSort = entry.txSort;
  state.cgtSort = entry.cgtSort;
  state.cgtFilterText = entry.cgtFilterText;
  state.incomeSort = entry.incomeSort;
  state.incomeFilterText = entry.incomeFilterText;
  state.balancesSort = entry.balancesSort;
  state.balancesFilterText = entry.balancesFilterText;
  state.reportPageSize = 50;
  state.txWindowStart = 0;
  state.txExpandedGroups = new Set();
}

function navigateBack(state: Record<string, unknown>, render: (s: any) => void): boolean {
  if (navIndex <= 0) return false;
  navNavigating = true;
  navIndex--;
  applyNavEntry(navHistory[navIndex], state);
  updateNavFlags(state);
  saveNavState(navHistory[navIndex]);
  render(state);
  navNavigating = false;
  return true;
}

function navigateForward(state: Record<string, unknown>, render: (s: any) => void): boolean {
  if (navIndex >= navHistory.length - 1) return false;
  navNavigating = true;
  navIndex++;
  applyNavEntry(navHistory[navIndex], state);
  updateNavFlags(state);
  saveNavState(navHistory[navIndex]);
  render(state);
  navNavigating = false;
  return true;
}

function updateNavFlags(state: Record<string, unknown>): void {
  state.navCanGoBack = navIndex > 0;
  state.navCanGoForward = navIndex < navHistory.length - 1;
}

function saveNavState(entry: NavEntry): void {
  try {
    localStorage.setItem(NAV_STORAGE_KEY, JSON.stringify(entry));
  } catch { /* localStorage may be unavailable */ }
}

function loadNavState(): NavEntry | undefined {
  try {
    const raw = localStorage.getItem(NAV_STORAGE_KEY);
    if (raw) return JSON.parse(raw) as NavEntry;
  } catch { /* ignore parse errors */ }
  return undefined;
}

function sortTransactions(txns: Transaction[], sort: SortState, selectedAccount?: string): Transaction[] {
  const dir = sort.direction === "asc" ? 1 : -1;
  return [...txns].sort((a, b) => {
    let cmp = 0;
    switch (sort.column) {
      case "date":
        cmp = a.datetime.localeCompare(b.datetime);
        break;
      case "party":
        cmp = (a.payee ?? "").localeCompare(b.payee ?? "");
        break;
      case "notes":
        cmp = (a.narration ?? "").localeCompare(b.narration ?? "");
        break;
      case "category": {
        const catA = a.postings.find((p) => p.account !== selectedAccount)?.account ?? "";
        const catB = b.postings.find((p) => p.account !== selectedAccount)?.account ?? "";
        cmp = catA.localeCompare(catB);
        break;
      }
      case "amount":
        cmp = a.amount - b.amount;
        break;
    }
    return cmp * dir;
  });
}

// ── Transaction aggregation (display-only grouping) ──

// Grouping logic imported from tx-grouping.ts — see tx-grouping.test.ts for tests

type ManualPostingInput = {
  account: string;
  amount: string;
  commodity: string;
  remainder?: string | null;
};

type ManualTransactionInput = {
  datetime: string;
  status?: string | null;
  payee: string;
  narration: string;
  postings: ManualPostingInput[];
};

type SyncEvent = {
  timestamp: number;
  device_id: string;
  event_type: string;
  target_id: string;
  details: string;
};

type DeviceInfo = {
  device_id: string;
  device_name: string;
  last_seen: number;
};

type SyncResponse = {
  files_transferred: number;
  metadata_merged: boolean;
};

type PairInitiateResult = {
  group_id: string;
  pairing_code: string;
  expires_in: number;
};

type RelayConfig = {
  relay_url: string;
  group_id: string;
};

type RelaySyncResponse = {
  metadata_merged: boolean;
  blobs_uploaded: number;
  blobs_downloaded: number;
};

const ACCOUNT_TYPES = ["assets"] as const;

type TreeNode = {
  name: string;
  fullPath: string;
  totals: Map<string, number>;
  children: TreeNode[];
  isLeaf: boolean;
};

/** The selected account / drill path for whichever left-pane is active. The
 *  Categories pane keeps its own selection (`selectedCategory`/`categoryDrillPath`)
 *  so switching panes never clobbers the other; the shared transaction-window and
 *  query layer read through these accessors. */
function activeSelectedAccount(state: AppState): string | undefined {
  return state.sidebarView === "categories" ? state.selectedCategory : state.selectedAccount;
}
function activeDrillPath(state: AppState): string[] {
  return state.sidebarView === "categories" ? state.categoryDrillPath : state.drillPath;
}

/** Both left-panes that render the account header + transaction table (Accounts
 *  and Categories), as opposed to Reports/Plugins/rule-editor. */
function isTransactionPane(state: AppState): boolean {
  return state.sidebarView === "accounts" || state.sidebarView === "categories";
}

/** Select a leaf account in the active pane, resetting the shared tx window. */
function selectActiveAccount(state: AppState, account: string): void {
  if (state.sidebarView === "categories") resetStateForCategoryNavigation(state, account);
  else resetStateForAccountNavigation(state, account);
}

/** Drill into a group node (`name` = segment, `group` = full path) in the active pane. */
function drillIntoActive(state: AppState, name: string, group: string): void {
  if (state.sidebarView === "categories") {
    state.categoryDrillPath = [...state.categoryDrillPath, name];
    state.selectedCategory = group;
  } else {
    state.drillPath = [...state.drillPath, name];
    state.selectedAccount = group;
  }
}

/** Pop one drill level in the active pane and update the selection. For
 *  Categories the path includes the root segment so the group account is the
 *  path joined directly; popping to empty lands on the roots view (no selection).
 *  For Accounts, popping past the top lands on the synthetic root folder. */
function drillBackActive(state: AppState): void {
  if (state.sidebarView === "categories") {
    state.categoryDrillPath = state.categoryDrillPath.slice(0, -1);
    state.selectedCategory = state.categoryDrillPath.length > 0
      ? state.categoryDrillPath.join(":")
      : undefined;
  } else {
    state.drillPath = state.drillPath.slice(0, -1);
    state.selectedAccount = state.drillPath.length > 0
      ? "assets:" + state.drillPath.join(":")
      : ROOT_FOLDER_PATH;
  }
}

function buildAccountTree(balances: AccountBalance[]): TreeNode[] {
  const roots: TreeNode[] = ACCOUNT_TYPES.map((t) => ({
    name: t,
    fullPath: t,
    totals: new Map<string, number>(),
    children: [],
    isLeaf: false,
  }));
  const rootMap = new Map<string, TreeNode>(roots.map((r) => [r.name, r]));

  for (const balance of balances) {
    const segments = balance.account.split(":");
    const rootName = segments[0];
    let parent = rootMap.get(rootName);
    if (!parent) {
      parent = { name: rootName, fullPath: rootName, totals: new Map(), children: [], isLeaf: false };
      roots.push(parent);
      rootMap.set(rootName, parent);
    }

    let current = parent;
    for (let i = 1; i < segments.length; i++) {
      const seg = segments[i];
      const path = segments.slice(0, i + 1).join(":");
      let child = current.children.find((c) => c.name === seg);
      if (!child) {
        child = { name: seg, fullPath: path, totals: new Map(), children: [], isLeaf: false };
        current.children.push(child);
      }
      current = child;
    }
    current.isLeaf = true;

    // Set leaf totals and aggregate upward
    for (const t of balance.totals) {
      current.totals.set(t.commodity, (current.totals.get(t.commodity) ?? 0) + t.amount);
    }

    // Walk ancestors to aggregate
    let ancestor = rootMap.get(rootName)!;
    for (const t of balance.totals) {
      ancestor.totals.set(t.commodity, (ancestor.totals.get(t.commodity) ?? 0) + t.amount);
    }
    for (let i = 1; i < segments.length - 1; i++) {
      const path = segments.slice(0, i + 1).join(":");
      ancestor = ancestor.children.find((c) => c.fullPath === path)!;
      if (ancestor) {
        for (const t of balance.totals) {
          ancestor.totals.set(t.commodity, (ancestor.totals.get(t.commodity) ?? 0) + t.amount);
        }
      }
    }
  }

  // Sort children alphabetically at each level
  function sortTree(nodes: TreeNode[]): void {
    nodes.sort((a, b) => a.name.localeCompare(b.name));
    for (const n of nodes) sortTree(n.children);
  }
  sortTree(roots);

  return roots;
}

function findTreeNode(tree: TreeNode[], fullPath: string): TreeNode | undefined {
  const parts = fullPath.split(":");
  let current: TreeNode | undefined = tree.find((r) => r.name === parts[0]);
  for (let i = 1; current && i < parts.length; i++) {
    current = current.children.find((c) => c.name === parts[i]);
  }
  return current;
}

/** Drill path (group chain) to reach `account`. `rootName` non-null = single-root
 *  (Accounts): the root segment is implicit and stripped. `rootName === null` =
 *  multi-root (Categories): every top-level tree node is a root, so the path
 *  includes the root segment. */
function drillPathForAccount(
  account: string | undefined,
  tree: TreeNode[],
  rootName: string | null = "assets",
): string[] {
  if (!account) return [];
  let current: TreeNode;
  let segments: string[];
  if (rootName === null) {
    current = { name: "", fullPath: "", totals: new Map(), children: tree, isLeaf: false };
    segments = account.split(":");
  } else {
    const root = tree.find((r) => r.name === rootName);
    if (!root) return [];
    current = root;
    segments = account.split(":").slice(1); // strip the single implicit root (e.g. "assets:")
  }
  const path: string[] = [];
  for (const seg of segments) {
    const child = current.children.find((c) => c.name === seg);
    if (!child || child.children.length === 0) break;
    path.push(seg);
    current = child;
  }
  return path;
}

function drillDownNodeTotal(node: TreeNode, treeBaseTotals?: Map<string, number>, baseCurrency?: string): string {
  const baseTotal = treeBaseTotals?.get(node.fullPath);
  if (baseTotal !== undefined && baseCurrency) return formatAmount(baseTotal, baseCurrency);
  const dt = pickTreeTotal(node.totals);
  return dt ? formatAmount(dt.amount, dt.commodity) : "";
}

function drillDownIssueCount(node: TreeNode, issueCounts: Map<string, number>): number {
  let count = issueCounts.get(node.fullPath) ?? 0;
  const prefix = node.fullPath + ":";
  for (const [account, n] of issueCounts) {
    if (account.startsWith(prefix)) count += n;
  }
  return count;
}

function renderDrillDownNode(node: TreeNode, selectedAccount: string | undefined, totalText: string, badgeHtml: string): string {
  if (node.children.length > 0) {
    const groupActive = node.fullPath === selectedAccount;
    return `<button class="drilldown__item${groupActive ? " drilldown__item--active" : ""}" type="button" data-testid="drilldown-item" data-drill="${escapeText(node.name)}" data-group="${escapeText(node.fullPath)}">
      <span class="drilldown__label"><span class="drilldown__name">${escapeText(displayName(node.name))}${badgeHtml}</span>${totalText ? `<span class="drilldown__total">${escapeText(totalText)}</span>` : ""}</span>
      <span class="drilldown__chevron">\u203A</span>
    </button>`;
  }
  const active = node.fullPath === selectedAccount;
  return `<button class="account ${active ? "account--active" : ""}" type="button" data-testid="account-item" data-account="${escapeText(node.fullPath)}">
    <span class="account__name">${escapeText(displayName(node.name))}${badgeHtml}</span>
    <span class="account__amt">${escapeText(totalText)}</span>
  </button>`;
}

function renderDrillDown(
  tree: TreeNode[],
  drillPath: string[],
  selectedAccount: string | undefined,
  issueCounts: Map<string, number>,
  treeBaseTotals?: Map<string, number>,
  baseCurrency?: string,
  rootName: string | null = "assets",
): string {
  // rootName non-null = render the children of that single root (Accounts).
  // rootName === null = treat the whole tree as the level-0 list (Categories),
  // via a synthetic root so the walk/back-button logic is shared unchanged.
  let current: TreeNode;
  if (rootName === null) {
    current = { name: "", fullPath: "", totals: new Map(), children: tree, isLeaf: false };
  } else {
    const root = tree.find((r) => r.name === rootName);
    if (!root) return "";
    current = root;
  }
  for (const seg of drillPath) {
    const child = current.children.find((c) => c.name === seg);
    if (!child) break;
    current = child;
  }

  let html = "";
  if (drillPath.length > 0) {
    html += `<button class="drilldown__back" type="button" data-testid="drilldown-back"><span class="drilldown__backArrow">\u2039</span><span>${escapeText(displayName(current.name))}</span></button>`;
  }
  for (const node of current.children) {
    const totalText = drillDownNodeTotal(node, treeBaseTotals, baseCurrency);
    const issueCount = drillDownIssueCount(node, issueCounts);
    const badgeHtml = issueCount > 0 ? `<span class="account__badge">${issueCount}</span>` : "";
    html += renderDrillDownNode(node, selectedAccount, totalText, badgeHtml);
  }
  return html;
}

function pickTreeTotal(totals: Map<string, number>, preferredCurrencies: string[] = ["USD", "AUD"]): CommodityAmount | undefined {
  for (const c of preferredCurrencies) {
    if (totals.has(c)) return { commodity: c, amount: totals.get(c)! };
  }
  const first = totals.entries().next();
  if (first.done) return undefined;
  return { commodity: first.value[0], amount: first.value[1] };
}

const app = document.querySelector<HTMLDivElement>("#app")!;
if (!app) {
  throw new Error("#app not found");
}

// Product name lives in one place (src/app-config.ts); override via VITE_APP_TITLE.
document.title = APP_TITLE;

// Debug console — captures console.warn, console.error, and unhandled rejections
const debugLog: { ts: string; level: string; message: string }[] = [];
let debugPanelOpen = false;
const MAX_DEBUG_LINES = 200;

function pushDebug(level: string, message: string) {
  const ts = new Date().toLocaleTimeString();
  debugLog.push({ ts, level, message });
  if (debugLog.length > MAX_DEBUG_LINES) debugLog.shift();
  renderDebugPanel();
}

const origWarn = console.warn.bind(console);
const origError = console.error.bind(console);
console.warn = (...args: unknown[]) => { origWarn(...args); pushDebug("warn", args.map(String).join(" ")); };
console.error = (...args: unknown[]) => { origError(...args); pushDebug("error", args.map(String).join(" ")); };
window.addEventListener("unhandledrejection", (e) => { pushDebug("error", `Unhandled: ${e.reason}`); });

// Long Task observer: any chunk of sync JS work over 50ms gets logged
// with its duration and the time it began. macOS shows the beach ball
// on sustained main-thread blocks; this surfaces them so we can find
// exactly which call site was responsible.
if (typeof PerformanceObserver !== "undefined") {
  try {
    const obs = new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        if (entry.duration >= 50) {
          pushDebug("perf", `LONG_TASK ${entry.duration.toFixed(0)}ms (started ${entry.startTime.toFixed(0)})`);
        }
      }
    });
    obs.observe({ entryTypes: ["longtask"] });
  } catch { /* longtask not supported */ }
}

// Render the debug toggle button on startup
requestAnimationFrame(() => renderDebugPanel());

function renderDebugPanel() {
  let panel = document.getElementById("debugPanel");
  if (!panel) {
    panel = document.createElement("div");
    panel.id = "debugPanel";
    document.body.appendChild(panel);
  }
  const badge = debugLog.filter((l) => l.level === "error").length;
  const badgeHtml = badge > 0 ? `<span class="debugPanel__badge">${badge}</span>` : "";
  panel.innerHTML = `
    <button class="debugPanel__toggle" id="debugToggle">Debug ${badgeHtml}</button>
    ${debugPanelOpen ? `<div class="debugPanel__content">
      <div class="debugPanel__toolbar">
        <button class="btn btn--small" id="debugClear">Clear</button>
      </div>
      <div class="debugPanel__log">${
        debugLog.length === 0
          ? `<div class="debugPanel__empty">No messages</div>`
          : debugLog.map((l) =>
              `<div class="debugPanel__line debugPanel__line--${l.level}"><span class="debugPanel__ts">${l.ts}</span> <span class="debugPanel__lvl">${l.level}</span> ${escapeText(l.message)}</div>`
            ).join("")
      }</div>
    </div>` : ""}
  `;
  document.getElementById("debugToggle")?.addEventListener("click", () => {
    debugPanelOpen = !debugPanelOpen;
    renderDebugPanel();
  });
  document.getElementById("debugClear")?.addEventListener("click", () => {
    debugLog.length = 0;
    renderDebugPanel();
  });
  // Auto-scroll to bottom
  if (debugPanelOpen) {
    const log = panel.querySelector(".debugPanel__log");
    if (log) log.scrollTop = log.scrollHeight;
  }
}

function copyToClipboard(text: string): void {
  // navigator.clipboard.writeText() hangs in Tauri WebView (Promise never
  // resolves or rejects), so always use the execCommand fallback.
  const ta = document.createElement("textarea");
  ta.value = text;
  ta.style.position = "fixed";
  ta.style.opacity = "0";
  document.body.appendChild(ta);
  ta.select();
  document.execCommand("copy");
  document.body.removeChild(ta);
}

/** Find a transaction by its txn:xxx ID in meta. Returns the txn and its first posting account. */
function findTxnByMeta(state: AppState, txnId: string): Transaction | undefined {
  return (state.parse?.transactions ?? []).find((t) =>
    t.meta?.includes(txnId)
  );
}

function resolveAccountFolder(state: AppState, account: string): string {
  return _resolveAccountFolder(state.accountFoldersMap, state.selectedAccountSet, account);
}

/** Compute the target folder path for a given scope selection (used for save path). */
function targetFolderForScope(accountFolder: string, scope: "local" | "institution" | "global"): string {
  if (scope === "global") return "";
  if (scope === "institution") {
    const parts = accountFolder.split("/");
    return parts.length > 1 ? parts.slice(0, -1).join("/") : "";
  }
  return accountFolder;
}



/** Derive account folder and isASell for a trade link between two txn IDs. */
function tradeLinkParams(state: AppState, txnIdA: string, txnIdB: string): { accountFolder: string; isASell: boolean } {
  const txnA = findTxnByMeta(state, txnIdA);
  const firstAccount = txnA?.postings[0]?.account ?? "";
  const accountFolder = resolveAccountFolder(state, firstAccount);
  const amountA = txnA?.amount ?? 0;
  return { accountFolder, isASell: amountA < 0 };
}

function formatDateCell(t: Transaction): string {
  const date = escapeText(t.date);
  // Extract time from datetime if present (e.g. "2022-08-15 14:30:00" or "2022-08-15T14:30:00")
  const dt = t.datetime ?? "";
  const timeMatch = dt.match(/[T ](\d{2}:\d{2}:\d{2})/);
  if (timeMatch) {
    return `<a class="dateLink" data-date-pill="${escapeText(t.date)}">${date}<div class="date__time">${timeMatch[1]}</div></a>`;
  }
  return `<a class="dateLink" data-date-pill="${escapeText(t.date)}">${date}</a>`;
}

function pickDisplayTotal(totals: CommodityAmount[]): CommodityAmount | undefined {
  return totals.find((t) => t.commodity === "USD") ?? totals[0];
}

const DATE_FILTER_LABELS: Record<DateFilter, string> = {
  week: "This Week",
  month: "This Month",
  year: "This Year",
  all: "All Time",
};

async function loadDisplayConfig(state: AppState): Promise<void> {
  try {
    const config = await invoke<DisplayConfig>("get_display_config", {
      accountSet: state.selectedAccountSet ?? "",
    });
    state.displayConfig = config;
  } catch {
    // Not available in non-Tauri contexts; fall back to defaults
  }
}

async function loadPlugins(state: AppState): Promise<void> {
  try {
    state.pluginsList = await invoke<PluginInfo[]>("list_plugins");
  } catch (e) {
    console.error("Failed to load plugins:", e);
    state.pluginsList = [];
  }
  try {
    state.updatePricesOnStartup = await invoke<boolean>("get_update_prices_on_startup");
  } catch {
    /* non-Tauri context — leave undefined */
  }
  try {
    state.extraPrimaryAccountPrefixes = await invoke<string[]>("get_extra_primary_account_prefixes");
  } catch {
    /* non-Tauri context — leave undefined */
  }
}

function attachDebouncedFilterInput(
  state: AppState,
  selector: string,
  apply: (s: AppState, value: string | undefined) => void,
): void {
  const input = document.querySelector<HTMLInputElement>(selector);
  if (!input) return;
  let timer: ReturnType<typeof setTimeout> | null = null;
  input.addEventListener("input", (e) => {
    const el = e.target as HTMLInputElement;
    apply(state, el.value || undefined);
    if (timer) clearTimeout(timer);
    const savedCursor = el.selectionStart;
    timer = setTimeout(() => {
      render(state);
      const restored = document.querySelector<HTMLInputElement>(selector);
      if (restored) {
        restored.focus();
        if (savedCursor != null) {
          const pos = Math.min(savedCursor, restored.value.length);
          restored.selectionStart = restored.selectionEnd = pos;
        }
      }
    }, 200);
  });
}

function toggleSort<C extends string>(
  current: GenericSortState<C> | undefined,
  col: C,
): GenericSortState<C> | undefined {
  if (current?.column === col) {
    return current.direction === "desc" ? { column: col, direction: "asc" } : undefined;
  }
  return { column: col, direction: "desc" };
}

function applySortToggle(state: AppState, scope: string | null, col: string): void {
  if (scope === "cgt") {
    state.cgtSort = toggleSort(state.cgtSort, col as CgtSortColumn);
  } else if (scope === "income") {
    state.incomeSort = toggleSort(state.incomeSort, col as IncomeSortColumn);
  } else if (scope === "balances") {
    state.balancesSort = toggleSort(state.balancesSort, col as BalancesSortColumn);
  } else {
    state.txSort = toggleSort(state.txSort, col as SortColumn);
  }
}

/**
 * When entering Balances with an empty scope, default it to the currently
 * selected sidebar account. This both surfaces the implicit data-partition to
 * the user and avoids the common "0 balance" trap where asset-side postings
 * (e.g. wallet:bitcoin +X BTC) cancel against their `assets:transfer -X BTC`
 * counterparts when summed across the entire vault. The user can clear the
 * Scope input to see the full unfiltered aggregate.
 */
function maybePrefillBalancesScope(state: AppState): void {
  if (state.selectedReport !== "balances") return;
  if (state.reportBaseScope) return;
  if (!state.selectedAccount) return;
  state.reportBaseScope = state.selectedAccount;
}

type ReportFetchArgs = {
  accountSet: string;
  baseCurrency: string;
  baseAccountScope: string | null;
};

function clearReportData(state: AppState): void {
  state.cgtReport = undefined;
  state.incomeReport = undefined;
  state.balancesReport = undefined;
  state.performanceReport = undefined;
  state.lossHarvestReport = undefined;
}

async function fetchFyReport(
  state: AppState,
  reportType: "cgt" | "income" | "balances" | "performance" | "loss_harvest",
  fy: string,
  args: ReportFetchArgs,
): Promise<void> {
  clearReportData(state);
  if (reportType === "performance") {
    // Performance is range-only on the backend; an FY *is* a 12-month window.
    const { start, end } = fyDateRange(
      Number(fy),
      state.taxConfig?.financial_year_end_month ?? 6,
      state.taxConfig?.financial_year_end_day ?? 30,
    );
    state.performanceReport = await invoke<PerformanceReport>(
      "generate_performance_report_range_cmd",
      { ...args, dateFrom: start, dateTo: end },
    );
    return;
  }
  const fyArgs = { ...args, financialYear: fy };
  if (reportType === "cgt") {
    state.cgtReport = await invoke<CgtReport>("generate_cgt_report_cmd", fyArgs);
  } else if (reportType === "income") {
    state.incomeReport = await invoke<IncomeTaxReport>("generate_income_report_cmd", fyArgs);
  } else if (reportType === "balances") {
    state.balancesReport = await invoke<BalancesReport>("generate_balances_report_cmd", fyArgs);
  } else {
    state.lossHarvestReport = await invoke<LossHarvestReport>("generate_loss_harvest_report_cmd", fyArgs);
  }
}

async function fetchCustomReport(
  state: AppState,
  reportType: "cgt" | "income" | "balances" | "performance" | "loss_harvest",
  dateFrom: string,
  dateTo: string,
  args: ReportFetchArgs,
): Promise<void> {
  clearReportData(state);
  if (reportType === "performance") {
    state.performanceReport = await invoke<PerformanceReport>(
      "generate_performance_report_range_cmd",
      { ...args, dateFrom, dateTo },
    );
    return;
  }
  if (reportType === "cgt") {
    state.cgtReport = await invoke<CgtReport>("generate_cgt_report_range_cmd", {
      ...args, dateFrom, dateTo,
    });
  } else if (reportType === "income") {
    state.incomeReport = await invoke<IncomeTaxReport>("generate_income_report_range_cmd", {
      ...args, dateFrom, dateTo,
    });
  } else if (reportType === "balances") {
    // Balances is a point-in-time snapshot — only the "to" date is meaningful.
    state.balancesReport = await invoke<BalancesReport>("generate_balances_report_range_cmd", {
      ...args, dateTo,
    });
  } else {
    // Loss harvesting offsets gains realised across the whole window.
    state.lossHarvestReport = await invoke<LossHarvestReport>("generate_loss_harvest_report_range_cmd", {
      ...args, dateFrom, dateTo,
    });
  }
}

async function loadReport(state: AppState): Promise<void> {
  if (!state.selectedReport) return;
  // Show a loading screen immediately — the report commands are async, so the
  // thread stays responsive while an uncached report generates (no beach ball).
  clearReportData(state);
  state.reportLoading = true;
  render(state);
  try {
    if (state.reportDateMode === "custom") {
      await loadCustomReport(state);
      return;
    }
    const reportType = state.selectedReport;

    // Load available years first so we can pick a sensible default. Performance
    // and Tax Savings are computed live (no cached report files), so reuse the
    // CGT year list to populate their FY dropdowns.
    const yearsReportType =
      reportType === "performance" || reportType === "loss_harvest" ? "cgt" : reportType;
    const years = await invoke<number[]>("list_report_years_cmd", {
      accountSet: state.selectedAccountSet ?? "",
      reportType: yearsReportType,
    });
    state.reportYears = years;

    // Default to the CURRENT financial year (computed from today + FY-end), not
    // the latest *cached* year — so reports open on "this year" even before the
    // current FY's cache exists, and don't get stuck on a stale completed year.
    if (!state.selectedReportYear || (years.length > 0 && !years.includes(state.selectedReportYear))) {
      state.selectedReportYear = currentFinancialYear(state.taxConfig);
    }

    if (!state.selectedReportYear) return;

    const args: ReportFetchArgs = {
      accountSet: state.selectedAccountSet ?? "",
      baseCurrency: state.displayConfig?.base_currency ?? "AUD",
      baseAccountScope: state.reportBaseScope || null,
    };
    await fetchFyReport(state, reportType, String(state.selectedReportYear), args);
    state.reportMarkdown = undefined;
  } catch (err) {
    console.error("Failed to load report:", err);
  } finally {
    state.reportLoading = false;
    render(state);
  }
}

async function loadCustomReport(state: AppState): Promise<void> {
  if (!state.selectedReport || !state.reportDateFrom || !state.reportDateTo) return;
  try {
    state.reportMarkdown = undefined;
    const args: ReportFetchArgs = {
      accountSet: state.selectedAccountSet ?? "",
      baseCurrency: state.displayConfig?.base_currency ?? "AUD",
      baseAccountScope: state.reportBaseScope || null,
    };
    await fetchCustomReport(
      state,
      state.selectedReport,
      state.reportDateFrom,
      state.reportDateTo,
      args,
    );
  } catch (err) {
    console.error("Failed to load custom report:", err);
  }
}

let heavyRefreshTimer: ReturnType<typeof setTimeout> | undefined;

/** The expensive, non-critical tail of a ledger refresh: base-currency
 *  conversions (hundreds of requests) and the full-vault trade-suggestion scan.
 *  Pulled off the per-change critical path and run via scheduleHeavyRefresh so a
 *  burst of changes triggers it once, after the user pauses — the cheap refresh
 *  (header balance + visible window) has already rendered. */
async function refreshHeavy(state: AppState): Promise<void> {
  await Promise.all([loadTransactionValues(state), loadAccountTotalValue(state), _loadTreeBaseCurrencyTotals(state)]);
  if (state.metadataReady) {
    try {
      const suggestions = await invoke<TradeSuggestion[]>("suggest_trade_links_cmd", {
        accountSet: state.selectedAccountSet ?? "",
        baseCurrency: state.displayConfig?.base_currency ?? null,
      });
      state.tradeSuggestions = suggestions;
    } catch (e) {
      console.warn("trade suggestions failed:", e);
    }
  }
  render(state);
}

/** Debounce the heavy refresh: collapses a burst of refreshes into a single run
 *  ~400ms after the last one, so rapid changes don't each pay the full-vault scan. */
function scheduleHeavyRefresh(state: AppState): void {
  if (heavyRefreshTimer !== undefined) clearTimeout(heavyRefreshTimer);
  heavyRefreshTimer = setTimeout(() => {
    heavyRefreshTimer = undefined;
    void refreshHeavy(state);
  }, 400);
}

async function loadGeneratedLedger(state: AppState, opts?: { silent?: boolean }): Promise<void> {
  // `silent` lets a mutation refresh the ledger beneath its own status/spinner
  // (e.g. "Adding transaction…") without flashing "Loading ledger…" or clearing
  // the caller's status. The cheap data refresh in `finally` still runs.
  const silent = opts?.silent ?? false;
  if (!silent) {
    state.busy = true;
    state.status = "Loading ledger...";
    const t0 = performance.now();
    render(state);
    pushDebug("info", `loadGeneratedLedger: first render took ${(performance.now() - t0).toFixed(0)}ms`);
  }

  try {
    // Load account tree from per-folder summaries (no transaction parsing)
    const balances = await invoke<{ account: string; totals: { commodity: string; amount: number }[] }[]>(
      "load_account_tree",
      { accountSet: state.selectedAccountSet ?? "" },
    );
    // Build a lightweight ParseResponse with just balances (no transactions)
    const response: ParseResponse = {
      ok: true,
      diagnostics: [],
      transactions: [],
      balances,
      accounts_with_opening: [],
      account_properties: {},
    };
    applyParse(state, response);
    if (!silent) state.status = undefined;
  } catch (err) {
    state.status = `Error: ${String(err)}`;
    state.parse = undefined;
  } finally {
    if (!silent) state.busy = false;
    // Cheap, scoped refresh on the critical path: header balance + visible
    // window. Render immediately so the change shows without waiting on the
    // expensive base-currency conversions + full-vault trade-suggestion scan.
    await Promise.all([loadPrefixQuery(state), runSearchFilter(state)]);
    render(state);
    // Heavy, non-critical work is deferred + debounced — it lands a beat later
    // and collapses a burst of changes into one run.
    scheduleHeavyRefresh(state);
  }
}

/** Apply a slim mutation response and refresh the view cheaply. Balance-changing
 *  mutations (manual add, CSV import) used to return the full active ledger so
 *  the frontend could `applyPipelineResponse` — a ~40MB IPC payload whose
 *  main-thread `JSON.parse` stalled the WebView for seconds on large vaults.
 *
 *  Instead we reload via `loadGeneratedLedger({ silent })`, which refreshes the
 *  sidebar balances (`state.parse.balances` from `load_account_tree`) AND the
 *  account header (`state.prefixQuery` from `query_search`) from the SAME fresh
 *  on-disk state. That shared source is the fix for the earlier window-only
 *  refresh, which left the sidebar stale while the header updated. */
async function applyMutationAndRefresh(state: AppState, response: MutationResponse): Promise<void> {
  state.pipelineWarnings = response.warnings ?? [];
  if (response.output_files_written > 0) state.reportsBuilding = true;
  // The rebuild invalidates every loaded page of the search window; reset so the
  // refresh re-fetches from offset 0 (a new/imported row sorts to the top).
  state.txWindowStart = 0;
  state.searchFilteredTransactions = undefined;
  state.searchFilteredCount = undefined;
  state.searchFilteredOffset = undefined;
  await loadGeneratedLedger(state, { silent: true });
}

/** Refresh the global account pool (load_account_tree("") = whole vault) that
 *  feeds account-name autocomplete. Called once at startup and again after a
 *  pipeline rebuild, since the account universe only changes when data does —
 *  switching the selected set does not. */
async function loadAllAccounts(state: AppState): Promise<void> {
  try {
    const all = await invoke<{ account: string }[]>("load_account_tree", { accountSet: "" });
    state.allAccounts = all.map((b) => b.account);
  } catch (e) {
    console.warn("load all accounts failed:", e);
  }
}

async function loadTransactionValues(state: AppState): Promise<void> {
  // Value computation lives in ./transaction-values (unit-tested there). Wire it
  // to the Tauri `convert_to_base_currency` command.
  state.transactionValues = await computeTransactionValues(state, (baseCurrency, requests) =>
    invoke<{ values: (number | null)[] }>("convert_to_base_currency", { baseCurrency, requests }).then((r) => r.values),
  );
}

async function loadPrefixQuery(state: AppState): Promise<void> {
  // Categories pane: the header total comes straight from the in-memory tree
  // balances (load_account_tree already carries nominal/contra accounts), so no
  // prefix query is needed — loadAccountTotalValue falls back to state.parse.balances.
  if (state.sidebarView === "categories") {
    state.prefixQuery = undefined;
    return;
  }
  const account = state.selectedAccount ?? "";
  const search = account ? `account:${account}` : "";
  try {
    // Only `aggregated_balance` is read from this result (the account header
    // total). For an `account:` search the backend computes balances over the
    // full match regardless of `limit`, so `limit: 0` returns the balance with
    // zero transactions — KB instead of tens of MB, avoiding a multi-second
    // JSON.parse stall on the WebView main thread on large vaults.
    state.prefixQuery = await invoke<QueryResult>("query_search", {
      accountSet: state.selectedAccountSet ?? "",
      search,
      limit: account ? 0 : undefined,
    });
  } catch {
    state.prefixQuery = undefined;
  }
}

/** Build the combined search string from state (account + date + user search). */
function buildSearchString(state: AppState): string {
  const parts: string[] = [];
  const account = activeSelectedAccount(state) ?? "";
  if (account) parts.push(`account:${account}`);
  const dateStart = dateFilterStart(state.dateFilter);
  if (dateStart) parts.push(`date:>=${dateStart}`);
  const searchRaw = state.search.trim();
  if (searchRaw) parts.push(searchRaw);
  return parts.join(" AND ");
}

/** Run the search filter via Rust and store results in state.
 *  Supported columns are sorted by Rust before pagination. Unsupported columns
 *  keep a stable backend date order and are only sorted locally in the loaded window. */
async function runSearchFilter(state: AppState): Promise<void> {
  const account = activeSelectedAccount(state) ?? "";
  const accountSet = state.selectedAccountSet ?? "";
  const combinedSearch = buildSearchString(state);
  // Categories are non-folder-backed, so the folder-pruned query_search returns
  // nothing for them — use the union query_global instead.
  const queryCmd = state.sidebarView === "categories" ? "query_global" : "query_search";

  // Categories with no selected category and no user search → roots view, empty
  // table. Without this an empty search would union ALL transactions across every
  // category (query_global ignores the folder scoping that bounds query_search).
  if (state.sidebarView === "categories" && !account && !state.search.trim()) {
    state.searchFilteredTransactions = undefined;
    state.searchFilteredCount = undefined;
    state.searchFilteredOffset = undefined;
    state.searchError = undefined;
    return;
  }

  if (!combinedSearch && !account && !state.selectedAccountSet) {
    state.searchFilteredTransactions = undefined;
    state.searchFilteredCount = undefined;
    state.searchFilteredOffset = undefined;
    state.searchError = undefined;
    return;
  }

  const backendSort = backendSortForTransactionWindow(state.txSort);

  const windowStart = state.txWindowStart;
  const windowSize = TX_WINDOW;

  try {
    const result = await invoke<QueryResult>(queryCmd, {
      accountSet,
      search: combinedSearch,
      sortField: backendSort.field,
      sortOrder: backendSort.order,
      offset: windowStart,
      limit: windowSize,
    });
    // Infinite scroll: append new page when fetching past the start.
    if (windowStart > 0 && state.searchFilteredTransactions) {
      state.searchFilteredTransactions = [...state.searchFilteredTransactions, ...result.transactions];
    } else {
      state.searchFilteredTransactions = result.transactions;
    }
    state.searchFilteredCount = result.transaction_count;
    state.searchFilteredOffset = 0;
    state.searchError = undefined;
  } catch (e) {
    state.searchError = String(e);
    state.searchFilteredTransactions = undefined;
    state.searchFilteredCount = undefined;
    state.searchFilteredOffset = undefined;
  }
}

async function loadAccountTotalValue(state: AppState): Promise<void> {
  const baseCurrency = state.displayConfig?.base_currency;
  const account = activeSelectedAccount(state);
  if (!baseCurrency || !state.parse || !account) {
    state.accountTotalValue = undefined;
    return;
  }
  const balance = state.prefixQuery
    ? { account: account ?? "", totals: state.prefixQuery.aggregated_balance }
    : state.parse.balances.find((b) => b.account === account);
  if (!balance || balance.totals.length === 0) {
    state.accountTotalValue = undefined;
    return;
  }
  // Single commodity that IS the base currency — no conversion needed
  if (balance.totals.length === 1 && balance.totals[0].commodity === baseCurrency) {
    state.accountTotalValue = {
      total: balance.totals[0].amount,
      currency: baseCurrency,
      bycommodity: [{ commodity: baseCurrency, amount: balance.totals[0].amount, value: balance.totals[0].amount }],
    };
    return;
  }
  // Convert each commodity to base currency at latest known price
  const now = new Date().toISOString().slice(0, 19);
  const requests = balance.totals.map((t) => ({
    commodity: t.commodity,
    amount: Math.abs(t.amount),
    datetime: now,
  }));
  try {
    const result = await invoke<{ values: (number | null)[] }>("convert_to_base_currency", {
      baseCurrency, requests,
    });
    let total = 0;
    const bycommodity = balance.totals.map((t, i) => {
      const rawValue = result.values[i];
      const value = rawValue !== null ? (t.amount < 0 ? -rawValue : rawValue) : null;
      if (value !== null) total += value;
      return { commodity: t.commodity, amount: t.amount, value };
    });
    state.accountTotalValue = { total, currency: baseCurrency, bycommodity };
  } catch {
    state.accountTotalValue = undefined;
  }
}

async function _loadTreeBaseCurrencyTotals(state: AppState): Promise<void> {
  state.treeBaseTotals = await loadTreeBaseCurrencyTotals(
    state.displayConfig?.base_currency,
    state.parse?.balances ?? [],
    invoke,
  );
}

function rebuildPipeline(state: AppState): Promise<void> {
  // Route through the mutation queue so the click handler returns to
  // the event loop immediately. Without this the synchronous render
  // before the await fires while state.parse holds tens of thousands
  // of transactions, blocking the WebKit main thread long enough for
  // macOS to show a wait cursor.
  return mutationQueue.enqueue(state, "Rebuilding...", async () => {
    try {
      const response = await invoke<PipelineResponse>("rebuild_pipeline", {
        nowYyyymm: nowYYYYMM(),
        accountSet: state.selectedAccountSet ?? "",
      });
      await applyPipelineResponse(state, response);
      const parsedCount = response.parse.transactions.length;
      state.status = `Parsed (${parsedCount} transactions) — transformed ${response.result.csv_transformed}, cached ${response.result.csv_cached}, manual ${response.result.manual_count}`;
    } catch (err) {
      state.status = `Error: ${String(err)}`;
      throw err;
    }
    await Promise.all([loadPrefixQuery(state), runSearchFilter(state)]);
    await Promise.all([loadTransactionValues(state), loadAccountTotalValue(state), _loadTreeBaseCurrencyTotals(state)]);
  }).catch(() => { /* status already set */ });
}

/** Append a manual transaction, then refresh balances and the visible window
 *  via the slim mutation path (no full-ledger IPC — see applyMutationAndRefresh). */
async function addManualTransaction(state: AppState): Promise<void> {
  const draft = state.manualDraft;
  if (!draft) return;

  const built = buildManualPostings(draft);
  if (!built.ok) throw new Error(built.error);

  const input: ManualTransactionInput = {
    datetime: draft.datetime.trim(),
    status: "*",
    payee: draft.payee.trim(),
    narration: draft.narration.trim(),
    postings: built.postings,
  };

  if (!input.datetime || !input.payee || !input.narration) {
    throw new Error("Date, payee, and narration are required.");
  }

  // Use the currently selected account's folder so the manual transaction
  // lands in the correct source directory (not the root).
  const selAcct = state.selectedAccount ?? "";
  const accountFolder = selAcct ? resolveAccountFolder(state, selAcct) : "";

  // Optimistic: synthesize the row + show it immediately (modal closes, balance
  // adjusts), then persist + refresh in the BACKGROUND so the UI is never
  // blocked by the ~seconds-long rebuild. The user can keep working meanwhile.
  const tempId = nextOptimisticTempId();
  const tempTxn = buildOptimisticTransaction(input, built.postings, selAcct, tempId);
  state.pendingAdds = [tempTxn, ...state.pendingAdds];
  if (!state.justAddedTxnIds) state.justAddedTxnIds = new Set();
  state.justAddedTxnIds.add(tempId);
  const savedDraft = draft;
  state.manualDraft = undefined; // close the modal — the row is already visible
  state.status = "Added manual transaction";
  render(state);

  mutationQueue
    .enqueue(state, "Adding transaction…", async () => {
      await invoke<MutationResponse>("add_manual_transaction", {
        nowYyyymm: nowYYYYMM(),
        input,
        accountFolder,
        accountSet: state.selectedAccountSet ?? "",
      });
      // The real row is now on disk; drop the temp BEFORE the refresh so the
      // reload renders the real row in its place (no double-row flash).
      state.pendingAdds = state.pendingAdds.filter((t) => t !== tempTxn);
      await loadGeneratedLedger(state, { silent: true });
    })
    .catch((err) => {
      // Rollback: removing the temp also reverts its render-time balance delta.
      state.pendingAdds = state.pendingAdds.filter((t) => t !== tempTxn);
      state.manualDraft = savedDraft;
      state.status = `Error: ${String(err)}`;
      render(state);
    });
}

async function addAccountDeclaration(state: AppState): Promise<void> {
  const draft = state.addAccountDraft;
  if (!draft) return;

  const rawName = draft.accountName.trim();
  if (!rawName) {
    throw new Error("Account name is required.");
  }
  // Auto-prefix assets: — user just types "ethereum" or "bank:savings"
  const accountName = rawName.startsWith("assets:") ? rawName : `assets:${rawName}`;

  const currency = draft.currency.trim() || null;
  const openingBalance = draft.openingBalance.trim() || null;

  state.busy = true;
  state.status = "Adding account...";
  render(state);

  try {
    // Resolve account folder from the map so declarations land in the same
    // folder as imported CSVs / manual transactions.
    const accountFolder = resolveAccountFolder(state, accountName);

    const response = await invoke<PipelineResponse>("add_account_declaration", {
      nowYyyymm: nowYYYYMM(),
      accountName,
      currency,
      openingBalance,
      accountSet: state.selectedAccountSet ?? "",
      accountFolder,
    });
    await applyPipelineResponse(state, response);
    state.selectedAccount = accountName;
    state.addAccountDraft = undefined;
    state.busy = false;
    render(state);

    // Immediately prompt to import a CSV or OFX file into the new account
    const filePath = await open({
      multiple: false,
      filters: [{ name: "CSV or OFX", extensions: ["csv", "ofx"] }],
      title: "Import CSV or OFX into new account",
    });
    if (!filePath) {
      state.status = `Added account "${accountName}"`;
      render(state);
      return;
    }

    state.busy = true;
    state.status = "Checking transform...";
    render(state);
    const suggestion = await invoke<SuggestTransformResponse>("suggest_transform", {
      sourcePath: filePath,
      accountFolder,
      accountName,
      currency,
    });

    if (!suggestion.needs_transform) {
      state.status = "Importing file...";
      render(state);
      const importResp = await invoke<MutationResponse>("import_csv_to_account", {
        nowYyyymm: nowYYYYMM(),
        sourcePath: filePath,
        accountFolder,
        accountSet: state.selectedAccountSet ?? "",
      });
      await applyMutationAndRefresh(state, importResp);
      state.status = `Added account & imported file`;
    } else {
      state.transformDraft = {
        csvFilename: suggestion.csv_filename,
        sourcePath: filePath,
        accountFolder,
        script: suggestion.suggestion ?? "",
        headers: suggestion.headers ?? [],
        error: undefined,
        currency: currency ?? undefined,
      };
      state.status = undefined;
    }
  } finally {
    state.busy = false;
    render(state);
  }
}

// Report render functions imported from ./render.ts
const markdownToHtml = (md: string) => marked(md) as string;
const renderMarkdownReport = (s: AppState) => _renderMarkdownReport(s, markdownToHtml);
const renderCgtReport = (s: AppState) => _renderCgtReport(s);
const renderIncomeReport = (s: AppState) => _renderIncomeReport(s);
const renderBalancesReport = (s: AppState) => _renderBalancesReport(s);
const renderPerformanceReport = (s: AppState) => _renderPerformanceReport(s);
const renderLossHarvestReport = (s: AppState) => _renderLossHarvestReport(s);
const renderTaxSettingsModal = (s: AppState) => _renderTaxSettingsModal(s);
const renderGlobalSettingsModal = (s: AppState) => _renderGlobalSettingsModal(s);

function renderVaultPicker(state: AppState): string {
  return `<div class="vaultPicker" data-testid="vault-picker">
        <div class="vaultPicker__card">
          <div class="vaultPicker__brand">${APP_NAME}</div>
          <h1 class="vaultPicker__title">Choose a data folder</h1>
          <p class="vaultPicker__desc">Pick a folder where your accounting data will be stored. You can use any folder — it will contain a <code>sources/</code> directory for your data and a <code>generated/</code> directory for outputs. This folder is great for version control with git.</p>
          <button id="vaultPickerOpen" class="btn vaultPicker__btn" type="button">Open Folder</button>
          ${state.knownRoots.length > 0 ? `
            <div class="vaultPicker__known">
              <div class="vaultPicker__knownTitle">Previously used</div>
              ${state.knownRoots.map((r) => `<button class="vaultPicker__knownItem" data-root="${escapeText(r)}" type="button">${escapeText(r.split("/").slice(-2).join("/"))}</button>`).join("")}
            </div>
          ` : ""}
        </div>
      </div>`;
}

function renderAccountsSidebarContent(state: AppState, tree: TreeNode[], selectedAccount: string | undefined, issueCounts: Map<string, number>): string {
  const setLabel = state.accountSets.length > 0
    ? state.selectedAccountSet ?? state.accountSets[0]
    : "";
  const setLabelDisplay = setLabel.charAt(0).toUpperCase() + setLabel.slice(1);
  const rootActive = selectedAccount === ROOT_FOLDER_PATH && state.drillPath.length === 0;
  const rootButton = setLabel
    ? `<button type="button" class="accountSetLabel${rootActive ? " accountSetLabel--active" : ""}" data-testid="sidebar-folder-root" data-folder-root="1">${escapeText(setLabelDisplay)}</button>`
    : "";
  return `<div class="sidebar__title">${rootButton}</div>
      <div class="accounts ${state.drillPath.length > 0 ? "accounts--drilled" : ""}" role="list">${renderDrillDown(tree, state.drillPath, selectedAccount, issueCounts, state.treeBaseTotals, state.displayConfig?.base_currency)}</div>
      <button id="addAccountBtn" class="btn btn--secondary btn--full" type="button" data-testid="add-account-btn" style="margin-top:8px" ${state.busy ? "disabled" : ""}>+ Add Account</button>`;
}

function renderSidebarContent(state: AppState, tree: TreeNode[], selectedAccount: string | undefined, issueCounts: Map<string, number>): string {
  if (state.sidebarView === "accounts") {
    return renderAccountsSidebarContent(state, tree, selectedAccount, issueCounts);
  }
  if (state.sidebarView === "categories") {
    // Non-folder-backed accounts (income/expenses/equity/liabilities + asset
    // contras), rendered as a multi-root drill-down (rootName === null). No
    // "Add Account" — categories aren't folder-backed.
    return `<div class="sidebar__title"><span class="accountSetLabel accountSetLabel--static">Categories</span></div>
      <div class="accounts ${state.categoryDrillPath.length > 0 ? "accounts--drilled" : ""}" role="list">${renderDrillDown(tree, state.categoryDrillPath, selectedAccount, issueCounts, state.treeBaseTotals, state.displayConfig?.base_currency, null)}</div>`;
  }
  if (state.sidebarView === "plugins") {
    return `<div class="sidebar__title">Plugins</div>
      <div class="pluginsList" role="list">${(state.pluginsList ?? []).length === 0
        ? `<div class="pluginsList__empty">No plugins found.<br>Add plugin folders to <code>plugins/</code></div>`
        : (state.pluginsList ?? []).map(p => renderPluginItem(p, state)).join("")}</div>`;
  }
  return renderReportMenu(state.selectedReport);
}

function renderSidebarSync(state: AppState): string {
  const relayHtml = state.relayConfig
    ? `<div class="relayStatus"><div class="relayStatus__label">Relay</div><div class="relayStatus__url">${escapeText(state.relayConfig.relay_url)}</div><button id="relaySyncBtn" class="btn btn--full" type="button" ${state.busy ? "disabled" : ""}>Sync Now</button></div>`
    : `<button id="relayPairBtn" class="btn btn--secondary btn--full" type="button" style="margin-top:4px" ${state.busy ? "disabled" : ""}>Pair Device</button>`;
  const devicesHtml = state.devices && state.devices.length > 0
    ? `<div class="syncDevices"><div class="syncDevices__title">Devices</div>${state.devices.map((d) => `<div class="syncDevices__item"><span class="syncDevices__name">${escapeText(d.device_name)}</span><span class="syncDevices__id">${escapeText(d.device_id)}</span></div>`).join("")}</div>`
    : "";
  return `<details class="syncAccordion"><summary class="syncAccordion__toggle">Sync</summary><div class="syncAccordion__body">${relayHtml}<button id="syncViewLogBtn" class="btn btn--secondary btn--full" type="button" style="margin-top:4px">View Sync Log</button>${devicesHtml}</div></details>`;
}

function renderSidebar(state: AppState, tree: TreeNode[], selectedAccount: string | undefined, balances: AccountBalance[], issueCounts: Map<string, number>): string {
  const navItems = (["accounts", "categories", "reports", "plugins"] as const).map((view) => {
    const icons: Record<string, string> = {
      accounts: '<svg class="sidebarNav__icon" viewBox="0 0 20 20" fill="currentColor"><path d="M4 4a2 2 0 00-2 2v1h16V6a2 2 0 00-2-2H4z"/><path fill-rule="evenodd" d="M2 9v5a2 2 0 002 2h12a2 2 0 002-2V9H2zm4 2a1 1 0 100 2h4a1 1 0 100-2H6z" clip-rule="evenodd"/></svg>',
      categories: '<svg class="sidebarNav__icon" viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M17.707 9.293l-5-5A1 1 0 0012 4H7a3 3 0 00-3 3v5a1 1 0 00.293.707l5 5a1 1 0 001.414 0l7-7a1 1 0 000-1.414zM6 8a1 1 0 100-2 1 1 0 000 2z" clip-rule="evenodd"/></svg>',
      reports: '<svg class="sidebarNav__icon" viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M6 2a2 2 0 00-2 2v12a2 2 0 002 2h8a2 2 0 002-2V7.414A2 2 0 0015.414 6L12 2.586A2 2 0 0010.586 2H6zm1 8a1 1 0 100 2h6a1 1 0 100-2H7zm0 4a1 1 0 100 2h3a1 1 0 100-2H7z" clip-rule="evenodd"/></svg>',
      plugins: '<svg class="sidebarNav__icon" viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M11.3 1.046A1 1 0 0112 2v5h4a1 1 0 01.82 1.573l-7 10A1 1 0 018 18v-5H4a1 1 0 01-.82-1.573l7-10a1 1 0 011.12-.38z" clip-rule="evenodd"/></svg>',
    };
    const label = view.charAt(0).toUpperCase() + view.slice(1);
    return `<button class="sidebarNav__item ${state.sidebarView === view ? "sidebarNav__item--active" : ""}" data-view="${view}">${icons[view]} ${label}</button>`;
  }).join("");

  const dataFolderHtml = state.rootDir
    ? `<div class="dataFolder"><div class="dataFolder__label">Data Folder</div><div class="dataFolder__path">${escapeText(state.rootDir.split("/").slice(-2).join("/"))}</div><button id="changeRootBtn" class="dataFolder__change" type="button">Change</button></div>`
    : "";

  return `<aside class="sidebar">
    <div class="sidebar__top"><div class="brand">${APP_NAME}</div><div class="brandSub">Plain Text Accounting</div></div>
    <nav class="sidebarNav">${navItems}</nav>
    <div class="sidebar__section">${renderSidebarContent(state, tree, selectedAccount, issueCounts)}</div>
    <div class="sidebar__bottom"><button id="globalSettingsBtn" class="sidebarSettingsBtn" type="button" title="Global Settings">⚙ Settings</button>${dataFolderHtml}${renderSidebarSync(state)}</div>
  </aside>`;
}

function renderTxnDetailRow(
  t: Transaction,
  txnId: string,
  expandKey: string,
  accountFolder: string,
  explorerUrl: string | undefined,
  colSpan: number,
): string {
  const rawId = txnId.startsWith("txn:") ? txnId.slice(4) : txnId;
  const txnLink = explorerUrl && rawId
    ? `<a class="txDetail__link" data-open-explorer="${escapeText(explorerUrl.replace("{txn_id}", rawId))}">${escapeText(rawId)}</a>`
    : `<span class="txDetail__hash">${escapeText(rawId || "—")}</span>`;

  const postingsHtml = t.postings
    .map(
      (p) =>
        `<div class="txDetail__posting"><span class="txDetail__postingAccount">${escapeText(p.account)}</span> <span class="txDetail__postingAmount">${escapeText(String(p.amount))} ${escapeText(p.commodity)}</span></div>`,
    )
    .join("");

  return `<tr class="txRow__detail" data-txn-detail-for="${escapeText(expandKey)}">
    <td colspan="${colSpan}">
      <div class="txDetail">
        <div class="txDetail__row"><span class="txDetail__label">Txn ID</span>${txnLink}</div>
        <div class="txDetail__row"><span class="txDetail__label">Payee</span><span>${escapeText(t.payee ?? "—")}</span></div>
        <div class="txDetail__row"><span class="txDetail__label">Meta</span><span>${escapeText(t.meta ?? "—")}</span></div>
        <div class="txDetail__row"><span class="txDetail__label">Source</span><span>${escapeText(accountFolder || "—")}</span></div>
        <div class="txDetail__row txDetail__row--postings"><span class="txDetail__label">Postings</span><div>${postingsHtml}</div></div>
        <div class="txDetail__actions">
          <button class="btn btn--small txDetail__ruleBtn" data-edit-rule-from-detail="${escapeText(expandKey)}">Edit Rule</button>
        </div>
      </div>
    </td>
  </tr>`;
}

/** One in-scope posting leg, pre-formatted for the amount cell. A row usually
 *  has one; a multi-commodity holding (e.g. property land + building) has one
 *  per commodity. */
type DisplayLeg = {
  amount: number;
  commodity: string;
  childSuffix: string;
  amountDisplay: string;
  dirClass: string;
  dirArrow: string;
  isOutgoing: boolean;
};

/** Per-row computed context for renderTransactionRows. */
type TxnRowContext = {
  t: Transaction;
  txnId: string;
  legId: string;
  expandKey: string;
  isExpanded: boolean;
  detailColSpan: number;
  legs: DisplayLeg[];
  postingAmount: number;
  commodity: string;
  childSuffix: string;
  other: Posting | undefined;
  isOutgoing: boolean;
  amountDisplay: string;
  dirClass: string;
  dirArrow: string;
  accountFolder: string;
  explorerUrl: string | undefined;
  isManual: boolean;
  isUncategorised: boolean | string | undefined;
  isAiSuggested: boolean;
  feePosting: Posting | undefined;
  isLinking: boolean;
  linkInfo: { link: TradeLink; partnerId: string } | undefined;
  partnerTxn: Transaction | undefined;
};

/** Zero-amount placeholder used when a transaction has no in-scope leg, so the
 *  row's single-leg fields have concrete defaults without per-field fallbacks. */
const EMPTY_LEG: DisplayLeg = {
  amount: 0, commodity: "USD", childSuffix: "", amountDisplay: "0",
  dirClass: "amount--in", dirArrow: "←", isOutgoing: false,
};

function makeDisplayLeg(p: Posting, selectedAccount: string | undefined): DisplayLeg {
  const isOutgoing = p.amount < 0;
  const childSuffix = selectedAccount && p.account !== selectedAccount
    ? p.account.slice(selectedAccount.length + 1)
    : "";
  return {
    amount: p.amount,
    commodity: p.commodity,
    childSuffix,
    amountDisplay: p.amount !== 0 ? formatAmount(Math.abs(p.amount), p.commodity) : "0",
    dirClass: isOutgoing ? "amount--out" : "amount--in",
    dirArrow: isOutgoing ? "→" : "←",
    isOutgoing,
  };
}

function computePostingContext(t: Transaction, selectedAccount: string | undefined) {
  // `displayLegs` returns the in-scope posting(s) to show: a single leg for the
  // common row and for in-scope transfers (where summing same-commodity legs
  // would net to zero and hide a half-recorded transfer), every leg for a
  // multi-commodity holding such as a property's separate land + building legs,
  // and — for a revealed ignored row whose legs all moved to `ignore:*` — the
  // first posting so the amount shows instead of a bare 0. The `other` fallback
  // returns a remaining leg so the contra column has something to show even when
  // both representative legs are in scope.
  const displayPostings = displayLegs(t, selectedAccount);
  const legs = displayPostings.map((p) => makeDisplayLeg(p, selectedAccount));
  const first = legs[0] ?? EMPTY_LEG;
  const other = selectedAccount
    ? (t.postings.find((p) => !matchesPrefix(p.account, selectedAccount))
       ?? t.postings.find((p) => p !== displayPostings[0]))
    : undefined;
  return {
    legs,
    postingAmount: first.amount,
    commodity: first.commodity,
    childSuffix: first.childSuffix,
    other,
    isOutgoing: first.isOutgoing,
    amountDisplay: first.amountDisplay,
    dirClass: first.dirClass,
    dirArrow: first.dirArrow,
  };
}

function deriveAccountFolder(t: Transaction, state: AppState, selectedAccount: string | undefined) {
  let accountFolder = "";
  for (const p of t.postings) {
    const folder = resolveAccountFolderFromMap(state.accountFoldersMap, p.account);
    if (folder) { accountFolder = folder; break; }
  }
  if (!accountFolder && selectedAccount) {
    accountFolder = resolveAccountFolder(state, selectedAccount);
  }
  const explorerUrl = accountFolder ? state.accountConfigs?.get(accountFolder)?.explorer_url : undefined;
  return { accountFolder, explorerUrl };
}

function computeRowFlags(t: Transaction, txnId: string, state: AppState, selectedAccount: string | undefined) {
  const isManual = txnId === "" || txnId.startsWith("txn:man-");
  const isUncategorised = selectedAccount && (!t.postings.find((p) => p.account !== selectedAccount) || t.postings.some((p) => p.account === "expenses:unknown"));
  const isAiSuggested = !isUncategorised && state.aiAppliedPatterns?.size
    ? [...state.aiAppliedPatterns].some((pat) =>
        wildcardMatch(pat, t.payee ?? "") || wildcardMatch(pat, t.narration ?? "") || wildcardMatch(pat, t.display_payee ?? ""))
    : false;
  const feePosting = t.postings.find((p) => p.account === "expenses:fees:trading");
  const isLinking = state.tradeLinkSelection === txnId && txnId !== "";
  return { isManual, isUncategorised, isAiSuggested, feePosting, isLinking };
}

function resolveSwapPartner(
  t: Transaction,
  txnId: string,
  tradeLinkMap: Map<string, { link: TradeLink; partnerId: string }>,
  renderedPartnerIds: Set<string>,
  txnIdMap?: Map<string, Transaction[]>,
) {
  const renderKey = `${txnId}|${t.amount_commodity}`;
  if (txnId && renderedPartnerIds.has(renderKey)) return null; // already rendered

  // The Rust pipeline stamps `swap:txn:<HASH>` and `swap_partner_commodity:<C>`
  // in meta for every paired txn. (hash, commodity) is unique per ledger row,
  // so the partner lookup is exact — no heuristics, and the same data
  // arimalo-query exposes on the CLI. Skip rows the pipeline didn't pair.
  const swapRef = swapPartnerRefFromMeta(t.meta);
  if (!swapRef) return { linkInfo: undefined, partnerTxn: undefined };

  // The "swap row" merge (collapse buy+sell onto one line) is gated on a
  // saved user trade-link: auto-detected pairs render as separate rows with
  // a chain-icon hint. linkInfo carries the saved link for the chain UI.
  const linkInfo = txnId ? tradeLinkMap.get(txnId) : undefined;
  if (!linkInfo) return { linkInfo: undefined, partnerTxn: undefined };

  const partnerTxn = txnIdMap
    ?.get(swapRef.partnerTxnId)
    ?.find((pt) => pt !== t && pt.amount_commodity === swapRef.partnerCommodity);
  if (partnerTxn) {
    renderedPartnerIds.add(`${swapRef.partnerTxnId}|${partnerTxn.amount_commodity}`);
  }
  return { linkInfo, partnerTxn };
}

function buildTxnRowContext(
  t: Transaction,
  state: AppState,
  selectedAccount: string | undefined,
  tradeLinkMap: Map<string, { link: TradeLink; partnerId: string }>,
  renderedPartnerIds: Set<string>,
  showValueColumn: boolean,
  txnIdMap?: Map<string, Transaction[]>,
): TxnRowContext | null {
  const txnId = txnIdFromMeta(t.meta);
  const legId = legIdFromMeta(t.meta);
  // Prefer the per-leg id as the expand/row key. Sibling legs of one on-chain
  // tx share txn+datetime+amount+narration, so the legacy tuple collides and
  // morphdom (which keys row identity on data-expand-key) collapses the pair;
  // the leg id is unique per leg. Falls back to the tuple for single-leg rows.
  const expandKey = legId || (txnId ? `${txnId}|${t.datetime}|${t.amount}|${t.narration ?? ""}` : "");
  const isExpanded = !!(expandKey && state.txExpandedRows?.has(expandKey));
  const detailColSpan = showValueColumn ? 7 : 6;

  const swap = resolveSwapPartner(t, txnId, tradeLinkMap, renderedPartnerIds, txnIdMap);
  if (!swap) return null;

  const posting = computePostingContext(t, selectedAccount);
  const folder = deriveAccountFolder(t, state, selectedAccount);
  const flags = computeRowFlags(t, txnId, state, selectedAccount);

  return {
    t, txnId, legId, expandKey, isExpanded, detailColSpan,
    ...posting, ...folder, ...flags,
    linkInfo: swap.linkInfo, partnerTxn: swap.partnerTxn,
  };
}

function renderPayeeCell(t: Transaction): string {
  return t.display_payee
    ? `<span class="copyable" data-copy="${escapeText(t.payee ?? "")}" title="Right-click to copy address">${escapeText(t.display_payee)}</span><div class="payee__address copyable" data-copy="${escapeText(t.payee ?? "")}" title="Right-click to copy address">${escapeText(shortAddress(t.payee ?? ""))}</div>`
    : `<span class="copyable" data-copy="${escapeText(t.payee ?? "")}" title="Right-click to copy">${escapeText(shortAddress(t.payee ?? ""))}</span>`;
}

function buildValueCellHtml(
  t: Transaction,
  state: AppState,
  showValueColumn: boolean,
  baseCurrency: string | undefined,
  commodity: string,
): string {
  if (!showValueColumn) return "";
  const val = state.transactionValues?.get(txnValueKey(t));
  const valAttrs = `data-value-commodity="${escapeText(commodity)}" data-value-datetime="${escapeText(t.datetime)}"`;
  if (val !== undefined && val !== null) {
    const valDisplay = formatAmount(Math.abs(val), baseCurrency!);
    const valDirClass = val < 0 ? "amount--out" : "amount--in";
    return `<td class="num ${valDirClass} value-clickable" ${valAttrs}>${escapeText(valDisplay)} <span class="amount__currency">${escapeText(baseCurrency!)}</span></td>`;
  }
  return `<td class="num value-clickable" ${valAttrs}>\u2014</td>`;
}

function buildSwapValueCellHtml(
  t: Transaction,
  partnerTxn: Transaction,
  state: AppState,
  showValueColumn: boolean,
  baseCurrency: string | undefined,
  commodity: string,
): string {
  if (!showValueColumn) return "";
  const valA = state.transactionValues?.get(txnValueKey(t));
  const valB = state.transactionValues?.get(txnValueKey(partnerTxn));
  const val = valA ?? valB;
  const attrs = `data-value-commodity="${escapeText(commodity)}" data-value-datetime="${escapeText(t.datetime)}"`;
  if (val != null) {
    return `<td class="num value-clickable" ${attrs}>${escapeText(formatAmount(Math.abs(val), baseCurrency!))} <span class="amount__currency">${escapeText(baseCurrency!)}</span></td>`;
  }
  return `<td class="num value-clickable" ${attrs}>\u2014</td>`;
}

function renderSwapAmountLine(amount: number, commodity: string, accountFolder: string): string {
  const isOut = amount < 0;
  const display = amount !== 0 ? formatAmount(Math.abs(amount), commodity) : "0";
  const cls = isOut ? "swap-amounts__out" : "swap-amounts__in";
  const arrow = isOut ? "\u2192" : "\u2190";
  return `<span class="${cls}"><span class="amount__arrow">${arrow}</span> ${escapeText(display)} <span class="amount__currency commodity-clickable" data-commodity="${escapeText(commodity)}" data-account-folder="${escapeText(accountFolder)}">${highlightNonAscii(escapeText(commodity))}</span></span>`;
}

function renderSwapRowHtml(
  ctx: TxnRowContext,
  state: AppState,
  selectedAccount: string | undefined,
  showValueColumn: boolean,
  baseCurrency: string | undefined,
): string {
  const { t, txnId, legId, expandKey, isExpanded, detailColSpan, commodity, childSuffix, accountFolder, explorerUrl, linkInfo, partnerTxn, postingAmount } = ctx;
  const partner = computePostingContext(partnerTxn!, selectedAccount);
  const swapValueCellHtml = buildSwapValueCellHtml(t, partnerTxn!, state, showValueColumn, baseCurrency, commodity);

  const detailHtml = isExpanded ? renderTxnDetailRow(t, txnId, expandKey, accountFolder, explorerUrl, detailColSpan) : "";
  return `
                            <tr data-testid="txn-row" data-txn-row-id="${escapeText(txnId)}" data-leg-id="${escapeText(legId)}" data-expand-key="${escapeText(expandKey)}" class="txRow--swap">
                              <td>${formatDateCell(t)}</td>
                              <td data-testid="txn-payee">${renderPayeeCell(t)}</td>
                              <td><div class="notes__main" data-testid="txn-notes">${highlightNonAscii(escapeText(t.narration ?? ""))}</div></td>
                              <td class="category">swap${childSuffix ? `<div class="category__account">${escapeText(childSuffix)}</div>` : ""}</td>
                              <td class="num" data-testid="txn-amount">
                                <div class="swap-amounts">
                                  ${renderSwapAmountLine(postingAmount, commodity, accountFolder)}
                                  ${renderSwapAmountLine(partner.postingAmount, partner.commodity, accountFolder)}
                                </div>
                              </td>
                              ${swapValueCellHtml}
                              <td class="txn-actions-col">
                                <button class="chain-btn chain-btn--linked"
                                  data-chain-txn-a="${escapeText(txnId)}"
                                  data-chain-txn-b="${escapeText(linkInfo!.partnerId)}"
                                  data-chain-linked="true"
                                  data-chain-account-folder="${escapeText(accountFolder)}"
                                  data-chain-is-a-sell="${ctx.isOutgoing}"
                                  title="Unlink trade"
                                >${CHAIN_LINK_SVG}</button>
                              </td>
                            </tr>
                            ${detailHtml}`;
}

const CHAIN_LINK_SVG = `<svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor"><path d="M17 7h-3c-.55 0-1 .45-1 1s.45 1 1 1h3c1.65 0 3 1.35 3 3s-1.35 3-3 3h-3c-.55 0-1 .45-1 1s.45 1 1 1h3c2.76 0 5-2.24 5-5s-2.24-5-5-5zm-10 5c0 .55.45 1 1 1h8c.55 0 1-.45 1-1s-.45-1-1-1H8c-.55 0-1 .45-1 1zM7 7c-2.76 0-5 2.24-5 5s2.24 5 5 5h3c.55 0 1-.45 1-1s-.45-1-1-1H7c-1.65 0-3-1.35-3-3s1.35-3 3-3h3c.55 0 1-.45 1-1s-.45-1-1-1H7z"/></svg>`;

function buildRowClasses(ctx: TxnRowContext, hasChainInfo: boolean): string {
  const parts: string[] = [];
  if (ctx.isUncategorised) parts.push("txRow--uncategorised");
  if (ctx.isAiSuggested) parts.push("txRow--ai-suggested");
  if (ctx.isLinking) parts.push("txRow--linking");
  if (hasChainInfo) parts.push("txRow--pair-top");
  return parts.join(" ");
}

function buildChainBtnHtml(
  txnId: string,
  chainInfo?: { txnIdB: string; isLinked: boolean; accountFolder: string; isASell: boolean },
): string {
  if (!chainInfo) return "";
  return `<span class="chain-btn-wrap"><button class="chain-btn ${chainInfo.isLinked ? "chain-btn--linked" : ""}"
      data-chain-txn-a="${escapeText(txnId)}"
      data-chain-txn-b="${escapeText(chainInfo.txnIdB)}"
      data-chain-linked="${chainInfo.isLinked}"
      data-chain-account-folder="${escapeText(chainInfo.accountFolder)}"
      data-chain-is-a-sell="${chainInfo.isASell}"
      title="${chainInfo.isLinked ? "Unlink trade" : "Link as trade pair"}"
    >${CHAIN_LINK_SVG}</button></span>`;
}

function renderTxnActionsCell(ctx: TxnRowContext, state: AppState, chainBtnHtml: string): string {
  const { t, txnId, isManual, isUncategorised, accountFolder } = ctx;
  const aiBtn = isUncategorised ? `<button
    class="ai-sparkle-btn${state.aiSuggest?.txnId === txnId ? " ai-sparkle-btn--active" : ""}"
    data-ai-txn-id="${escapeText(txnId)}"
    data-ai-payee="${escapeText(t.display_payee ?? t.payee ?? "")}"
    data-ai-narration="${escapeText(t.narration ?? "")}"
    data-ai-amount="${t.amount}"
    data-ai-commodity="${escapeText(t.amount_commodity)}"
    data-ai-date="${escapeText(t.date)}"
    data-ai-datetime="${escapeText(t.datetime)}"
    title="AI suggest category"
    ${state.busy ? "disabled" : ""}
  ><svg class="ai-sparkle-svg" viewBox="0 0 24 24" fill="currentColor"><path d="M12 0l2.4 9.6L24 12l-9.6 2.4L12 24l-2.4-9.6L0 12l9.6-2.4z"/></svg></button>` : "";
  const deleteBtn = `<button
    class="txn-delete-btn"
    data-testid="txn-delete"
    data-txn-id="${escapeText(txnId)}"
    data-txn-is-manual="${isManual}"
    data-txn-datetime="${escapeText(t.datetime)}"
    data-txn-payee="${escapeText(t.payee ?? "")}"
    data-txn-narration="${escapeText(t.narration ?? "")}"
    data-account-folder="${escapeText(accountFolder)}"
    title="${isManual ? "Delete transaction" : "Hide transaction"}"
  >&#x2715;</button>`;
  return `<td class="txn-actions-col">${aiBtn}${deleteBtn}${chainBtnHtml}</td>`;
}

function renderAmountLineHtml(leg: DisplayLeg, accountFolder: string): string {
  return `<span class="${leg.dirClass}"><span class="amount__arrow">${leg.dirArrow}</span> ${escapeText(leg.amountDisplay)} <span class="amount__currency commodity-clickable" data-commodity="${escapeText(leg.commodity)}" data-account-folder="${escapeText(accountFolder)}">${highlightNonAscii(escapeText(leg.commodity))}</span></span>`;
}

/** Amount cell. Single leg = one inline line (byte-identical to the old output);
 *  multiple in-scope legs (e.g. property land + building) = one line each. */
function renderAmountCellHtml(ctx: TxnRowContext, accountFolder: string, feeHtml: string): string {
  if (ctx.legs.length > 1) {
    const lines = ctx.legs.map((leg) => renderAmountLineHtml(leg, accountFolder)).join("");
    return `<td class="num" data-testid="txn-amount"><div class="multi-amounts">${lines}</div>${feeHtml}</td>`;
  }
  const { dirClass, dirArrow, amountDisplay, commodity } = ctx;
  return `<td class="num ${dirClass}" data-testid="txn-amount"><span class="amount__arrow">${dirArrow}</span> ${escapeText(amountDisplay)} <span class="amount__currency commodity-clickable" data-commodity="${escapeText(commodity)}" data-account-folder="${escapeText(accountFolder)}">${highlightNonAscii(escapeText(commodity))}</span>${feeHtml}</td>`;
}

function renderNormalRowHtml(
  ctx: TxnRowContext,
  state: AppState,
  showValueColumn: boolean,
  baseCurrency: string | undefined,
  chainInfo?: { txnIdB: string; isLinked: boolean; accountFolder: string; isASell: boolean },
): string {
  const { t, txnId, legId, expandKey, isExpanded, detailColSpan, commodity, childSuffix, other,
    accountFolder, explorerUrl, feePosting } = ctx;

  const valueCellHtml = buildValueCellHtml(t, state, showValueColumn, baseCurrency, commodity);
  const chainBtnHtml = buildChainBtnHtml(txnId, chainInfo);
  const rowClasses = withFadeInClass(buildRowClasses(ctx, !!chainInfo), txnId, state.justAddedTxnIds);
  const ruleIdAttr = extractRuleId(t.meta) ? ` data-rule-id="${escapeText(extractRuleId(t.meta)!)}"` : "";
  const feeHtml = feePosting ? `<div class="fee-line">fee ${escapeText(formatAmountSmart(feePosting.amount))} <span class="amount__currency">${highlightNonAscii(escapeText(feePosting.commodity))}</span></div>` : "";
  const detailHtml = isExpanded ? renderTxnDetailRow(t, txnId, expandKey, accountFolder, explorerUrl, detailColSpan) : "";

  return `
                            <tr data-testid="txn-row" data-txn-row-id="${escapeText(txnId)}" data-leg-id="${escapeText(legId)}" data-expand-key="${escapeText(expandKey)}" class="${rowClasses}">
                              <td>${formatDateCell(t)}</td>
                              <td class="cell-clickable" data-testid="txn-payee" data-rule-type="payee" data-narration="${escapeText(t.narration ?? "")}" data-source-payee="${escapeText(t.payee ?? "")}" data-display-payee="${escapeText(t.display_payee ?? "")}" data-account-folder="${escapeText(accountFolder)}">
                                ${renderPayeeCell(t)}
                              </td>
                              <td>
                                <div class="notes__main" data-testid="txn-notes">${highlightNonAscii(escapeText(t.narration ?? ""))}</div>
                              </td>
                              <td class="category cell-clickable" data-rule-type="category" data-narration="${escapeText(t.narration ?? "")}" data-account-folder="${escapeText(accountFolder)}"${ruleIdAttr}>${escapeText(other?.account ?? "—")}${childSuffix ? `<div class="category__account">${escapeText(childSuffix)}</div>` : ""}</td>
                              ${renderAmountCellHtml(ctx, accountFolder, feeHtml)}
                              ${valueCellHtml}
                              ${renderTxnActionsCell(ctx, state, chainBtnHtml)}
                            </tr>
                            ${detailHtml}`;
}

function renderTransactionRows(
  filteredTransactions: Transaction[],
  state: AppState,
  selectedAccount: string | undefined,
  allTransactions: Transaction[],
  isUncategorisedTxn: (t: Transaction) => boolean,
  tradeLinkMap: Map<string, { link: TradeLink; partnerId: string }>,
  renderedPartnerIds: Set<string>,
  baseCurrency: string | undefined,
  showValueColumn: boolean,
  txnIndexMap?: Map<Transaction, number>,
  chainInfo?: { txnIdB: string; isLinked: boolean; accountFolder: string; isASell: boolean },
  txnIdMap?: Map<string, Transaction[]>,
): string {
  if (filteredTransactions.length === 0) {
    return `<tr><td colspan="${showValueColumn ? 7 : 6}" class="empty">No transactions.</td></tr>`;
  }
  return filteredTransactions
    .map((t) => {
      const ctx = buildTxnRowContext(t, state, selectedAccount, tradeLinkMap, renderedPartnerIds, showValueColumn, txnIdMap);
      if (!ctx) return "";
      if (ctx.linkInfo && ctx.partnerTxn) {
        return renderSwapRowHtml(ctx, state, selectedAccount, showValueColumn, baseCurrency);
      }
      return renderNormalRowHtml(ctx, state, showValueColumn, baseCurrency, chainInfo);
    })
    .join("");
}

function buildGroupCategoryDisplay(g: TxGroup, selectedAccount: string | undefined): string {
  const categories = g.transactions.map((t) => {
    if (!selectedAccount) return "";
    const matched = t.postings.find((p) => matchesPrefix(p.account, selectedAccount));
    const other = t.postings.find((p) => !matchesPrefix(p.account, selectedAccount))
      ?? t.postings.find((p) => p !== matched);
    return other?.account ?? "";
  });
  const unique = [...new Set(categories.filter((c) => c))];
  if (unique.length === 1) return unique[0];
  if (unique.length > 1) return `${unique[0]} +${unique.length - 1}`;
  return "\u2014";
}

function buildGroupValueCellHtml(g: TxGroup, state: AppState, showValueColumn: boolean, baseCurrency: string | undefined): string {
  if (!showValueColumn) return "";
  let totalValue = 0;
  let hasValue = false;
  for (const t of g.transactions) {
    const valKey = `${t.datetime}|${t.postings[0]?.amount ?? 0}|${t.narration ?? ""}`;
    const val = state.transactionValues?.get(valKey);
    if (val != null) { totalValue += val; hasValue = true; }
  }
  if (hasValue) {
    const valDirClass = totalValue < 0 ? "amount--out" : "amount--in";
    return `<td class="num ${valDirClass}">${escapeText(formatAmount(Math.abs(totalValue), baseCurrency!))} <span class="amount__currency">${escapeText(baseCurrency!)}</span></td>`;
  }
  return `<td class="num">\u2014</td>`;
}

function buildGroupChainBtnHtml(
  g: TxGroup,
  itemIdx: number,
  items: TxRowItem[],
  groupPairIndices: Set<number>,
  state: AppState,
  selectedAccount: string | undefined,
): string {
  if (!groupPairIndices.has(itemIdx)) return "";
  const partnerItem = items[itemIdx + 1];
  if (partnerItem.kind !== "group-header") return "";
  const partnerGroup = partnerItem.group;
  const accountFolder = selectedAccount ? resolveAccountFolder(state, selectedAccount) : "";
  const sellGroup = g.netAmount < 0 ? g : partnerGroup;
  const buyGroup = g.netAmount < 0 ? partnerGroup : g;
  const sellIds = sortTxnIdsByAbsAmount(sellGroup.transactions, selectedAccount);
  const buyIds = sortTxnIdsByAbsAmount(buyGroup.transactions, selectedAccount);
  return `<button class="chain-btn"
    data-chain-group-sell="${escapeText(JSON.stringify(sellIds))}"
    data-chain-group-buy="${escapeText(JSON.stringify(buyIds))}"
    data-chain-account-folder="${escapeText(accountFolder)}"
    title="Link as trade pairs"
  >${CHAIN_LINK_SVG}</button>`;
}

function renderGroupHeaderRow(
  g: TxGroup,
  itemIdx: number,
  items: TxRowItem[],
  groupPairIndices: Set<number>,
  state: AppState,
  selectedAccount: string | undefined,
  showValueColumn: boolean,
  baseCurrency: string | undefined,
): string {
  const expanded = state.txExpandedGroups?.has(g.key) ?? false;
  const isNet = g.netAmount >= 0;
  const dirClass = isNet ? "amount--in" : "amount--out";
  const dirArrow = isNet ? "\u2190" : "\u2192";
  const netDisplay = formatAmount(Math.abs(g.netAmount), g.commodity);
  const toggleIcon = expanded ? "\u2212" : "+";
  const categoryDisplay = buildGroupCategoryDisplay(g, selectedAccount);
  const valueCellHtml = buildGroupValueCellHtml(g, state, showValueColumn, baseCurrency);
  const groupChainBtnHtml = buildGroupChainBtnHtml(g, itemIdx, items, groupPairIndices, state, selectedAccount);
  const isPair = groupPairIndices.has(itemIdx) || groupPairIndices.has(itemIdx - 1);

  return `<tr class="txGroup__header${isPair ? " txGroup__header--pair" : ""}" data-tx-group-toggle="${escapeText(g.key)}">
    <td>${escapeText(g.date)}</td>
    <td>${escapeText(g.venueName)}</td>
    <td class="txGroup__count">${g.narration ? `${escapeText(g.narration)} (${g.transactions.length})` : `${g.transactions.length} transactions`}</td>
    <td class="category">${escapeText(categoryDisplay)}</td>
    <td class="num ${dirClass}"><span class="amount__arrow">${dirArrow}</span> ${escapeText(netDisplay)} <span class="amount__currency">${escapeText(g.commodity)}</span>
      <div class="txGroup__breakdown">\u2190 ${escapeText(formatAmount(g.totalIn, g.commodity))} / \u2192 ${escapeText(formatAmount(g.totalOut, g.commodity))}</div>
    </td>
    ${valueCellHtml}
    <td class="txn-actions-col">${groupChainBtnHtml}<button class="txGroup__toggle" data-tx-group-toggle="${escapeText(g.key)}" title="${expanded ? "Collapse" : "Expand"}">${toggleIcon}</button></td>
  </tr>`;
}

function renderGroupedRows(
  items: TxRowItem[],
  state: AppState,
  selectedAccount: string | undefined,
  allTransactions: Transaction[],
  isUncategorisedTxn: (t: Transaction) => boolean,
  tradeLinkMap: Map<string, { link: TradeLink; partnerId: string }>,
  renderedPartnerIds: Set<string>,
  baseCurrency: string | undefined,
  showValueColumn: boolean,
  txnIndexMap: Map<Transaction, number>,
  txnIdMap?: Map<string, Transaction[]>,
): string {
  if (items.length === 0) {
    return `<tr><td colspan="${showValueColumn ? 7 : 6}" class="empty">No transactions.</td></tr>`;
  }
  // Detect potential trade pairs between adjacent single items
  const singleTxns = items.map((it) => it.kind === "single" ? it.transaction : null);
  const pairIndices = detectTradePairs(
    singleTxns.map((t) => t ?? { datetime: "", amount: 0, amount_commodity: "" })
  );
  for (const idx of pairIndices) {
    if (!singleTxns[idx] || !singleTxns[idx + 1]) pairIndices.delete(idx);
  }

  const chainInfoMap = new Map<number, { txnIdB: string; isLinked: boolean; accountFolder: string; isASell: boolean }>();
  for (const idx of pairIndices) {
    const txnA = singleTxns[idx]!;
    const txnB = singleTxns[idx + 1]!;
    const txnIdA = txnIdFromMeta(txnA.meta);
    const txnIdB = txnIdFromMeta(txnB.meta);
    const isLinked = !!(txnIdA && txnIdB && tradeLinkMap.has(txnIdA) && tradeLinkMap.get(txnIdA)?.partnerId === txnIdB);
    const accountFolder = selectedAccount ? resolveAccountFolder(state, selectedAccount) : "";
    chainInfoMap.set(idx, { txnIdB, isLinked, accountFolder, isASell: txnA.amount < 0 });
  }

  const groupPairIndices = detectGroupTradePairs(items);

  return items.map((item, itemIdx) => {
    switch (item.kind) {
      case "single":
        return renderTransactionRows([item.transaction], state, selectedAccount, allTransactions, isUncategorisedTxn, tradeLinkMap, renderedPartnerIds, baseCurrency, showValueColumn, txnIndexMap, chainInfoMap.get(itemIdx), txnIdMap);
      case "group-header":
        return renderGroupHeaderRow(item.group, itemIdx, items, groupPairIndices, state, selectedAccount, showValueColumn, baseCurrency);
      case "group-detail": {
        const html = renderTransactionRows([item.transaction], state, selectedAccount, allTransactions, isUncategorisedTxn, tradeLinkMap, renderedPartnerIds, baseCurrency, showValueColumn, txnIndexMap, undefined, txnIdMap);
        return html.replace(/<tr\b([^>]*)class="([^"]*)"/, '<tr$1class="txGroup__detail $2"');
      }
    }
  }).join("");
}

type ManualDraft = NonNullable<AppState["manualDraft"]>;

/** One-line balance status under the postings; explains the auto-fill. */
function manualBalanceText(m: ManualDraft, bal: ManualBalance): string {
  if (Number.isNaN(bal.topCash)) {
    return m.mode === "value" ? "Enter an amount to begin." : "Enter a quantity and price to begin.";
  }
  if (bal.blanks > 1) return "Enter an amount for all but one of the other accounts.";
  if (bal.blanks === 1) return `Balances — one row fills ${formatCash(bal.remainder)} ${m.cashCommodity}.`;
  const net = bal.topCash + bal.contraSum;
  if (Math.abs(net) < 1e-9) return "Balanced.";
  return `Out of balance by ${formatCash(net)} ${m.cashCommodity}.`;
}

/** Render the editable "other accounts" contra rows. Account inputs get the
 *  shared autocomplete attached after render; the single blank row shows the
 *  balancing remainder as its placeholder. */
function renderManualContraRows(m: ManualDraft, bal: ManualBalance, d: string): string {
  return m.contras
    .map((c, i) => {
      const isSingleBlank = bal.blanks === 1 && c.account.trim() !== "" && c.amount.trim() === "";
      const placeholder = isSingleBlank && Number.isFinite(bal.remainder) ? formatCash(bal.remainder) : "0.00";
      return `<div class="manualContra" data-contra-row="${i}">
        <input id="manualContraAccount-${i}" class="field__input field__input--mono manualContra__account" value="${escapeText(c.account)}" placeholder="expenses:unknown" autocomplete="off" autocapitalize="off" autocorrect="off" spellcheck="false" ${d} />
        <input id="manualContraAmount-${i}" class="field__input field__input--mono manualContra__amount" value="${escapeText(c.amount)}" placeholder="${escapeText(placeholder)}" inputmode="decimal" ${d} />
        <span class="manualContra__commodity">${escapeText(m.cashCommodity)}</span>
        <button class="manualContra__remove" data-contra-remove="${i}" title="Remove row" ${d}>✕</button>
      </div>`;
    })
    .join("");
}

function renderManualTxnModal(state: AppState): string {
  if (!state.manualDraft) return "";
  const m = state.manualDraft;
  const d = state.busy ? "disabled" : "";
  const bal = computeManualBalance(m);
  const valueField = `
      <label class="field"><div class="field__label">Amount (${escapeText(m.cashCommodity)})</div><input id="manualAmount" class="field__input field__input--mono" value="${escapeText(m.amount)}" placeholder="0.00" inputmode="decimal" ${d} /></label>`;
  const tradeFields = `
      <label class="field"><div class="field__label">Commodity</div><input id="manualTradeCommodity" class="field__input field__input--mono" value="${escapeText(m.tradeCommodity)}" placeholder="ACL" autocomplete="off" autocapitalize="characters" autocorrect="off" spellcheck="false" ${d} /></label>
      <div class="manualTrade__pair">
        <label class="field"><div class="field__label">Quantity</div><input id="manualQuantity" class="field__input field__input--mono" value="${escapeText(m.quantity)}" placeholder="100" inputmode="decimal" ${d} /></label>
        <label class="field"><div class="field__label">Price (per unit, ${escapeText(m.cashCommodity)})</div><input id="manualPrice" class="field__input field__input--mono" value="${escapeText(m.price)}" placeholder="0.00" inputmode="decimal" ${d} /></label>
      </div>
      <div class="field__hint">Use a negative quantity to record a sell.</div>`;
  return `<div class="modalOverlay" role="dialog" aria-modal="true" data-testid="manual-txn-modal">
    <div class="modal">
      <div class="modal__title">Add New (Manual)</div>
      <label class="field"><div class="field__label">Date</div><input id="manualDate" class="field__input" value="${escapeText(m.datetime)}" ${d} /></label>
      <label class="field"><div class="field__label">Payee</div><input id="manualPayee" class="field__input" value="${escapeText(m.payee)}" autocomplete="off" autocapitalize="off" autocorrect="off" spellcheck="false" ${d} /></label>
      <label class="field"><div class="field__label">Notes</div><input id="manualNarration" class="field__input" value="${escapeText(m.narration)}" ${d} /></label>
      <div class="field"><div class="field__label">Account</div><div class="manualAccount">${escapeText(m.account)}</div></div>
      <div class="openingMode">
        <label><input type="radio" name="manualMode" value="value" ${m.mode === "value" ? "checked" : ""} ${d} />Value</label>
        <label><input type="radio" name="manualMode" value="trade" ${m.mode === "trade" ? "checked" : ""} ${d} />Trade</label>
      </div>
      ${m.mode === "value" ? valueField : tradeFields}
      <div class="field"><div class="field__label">Other accounts</div>
        <div id="manualContras" class="manualContras">${renderManualContraRows(m, bal, d)}</div>
        <button id="manualAddContra" class="btn btn--small manualContras__add" ${d}>+ Add account</button>
      </div>
      <div id="manualBalance" class="manualBalance ${bal.balanceable ? "manualBalance--ok" : "manualBalance--off"}">${escapeText(manualBalanceText(m, bal))}</div>
      ${m.error ? `<div class="modal__error">${escapeText(m.error)}</div>` : ""}
      <div class="modal__actions"><button id="manualCancel" class="btn btn--secondary" ${d}>Cancel</button><button id="manualSave" class="btn" ${d}>Save</button></div>
    </div>
  </div>`;
}

/** Read every on-screen manual-modal field into the draft. Uses `?? existing`
 *  so the hidden mode's stashed values survive a Value/Trade toggle (their
 *  inputs aren't in the DOM, so the lookup returns undefined). */
function readManualDomIntoDraft(m: ManualDraft): void {
  const v = (id: string) => document.querySelector<HTMLInputElement>(`#${id}`)?.value;
  m.datetime = v("manualDate") ?? m.datetime;
  m.payee = v("manualPayee") ?? m.payee;
  m.narration = v("manualNarration") ?? m.narration;
  m.amount = v("manualAmount") ?? m.amount;
  m.tradeCommodity = v("manualTradeCommodity") ?? m.tradeCommodity;
  m.quantity = v("manualQuantity") ?? m.quantity;
  m.price = v("manualPrice") ?? m.price;
  m.contras.forEach((c, i) => {
    c.account = v(`manualContraAccount-${i}`) ?? c.account;
    c.amount = v(`manualContraAmount-${i}`) ?? c.amount;
  });
}

/** Recompute the balance line + the blank row's placeholder in place, without
 *  a full re-render (preserves focus/caret while the user is typing). */
function updateManualBalanceUi(state: AppState): void {
  const m = state.manualDraft;
  if (!m) return;
  readManualDomIntoDraft(m);
  const bal = computeManualBalance(m);
  const ind = document.querySelector<HTMLElement>("#manualBalance");
  if (ind) {
    ind.textContent = manualBalanceText(m, bal);
    ind.classList.toggle("manualBalance--ok", bal.balanceable);
    ind.classList.toggle("manualBalance--off", !bal.balanceable);
  }
  m.contras.forEach((c, i) => {
    const amtEl = document.querySelector<HTMLInputElement>(`#manualContraAmount-${i}`);
    if (!amtEl) return;
    const isSingleBlank = bal.blanks === 1 && c.account.trim() !== "" && c.amount.trim() === "";
    amtEl.placeholder = isSingleBlank && Number.isFinite(bal.remainder) ? formatCash(bal.remainder) : "0.00";
  });
}

function renderAddAccountModal(state: AppState): string {
  if (!state.addAccountDraft) return "";
  const d = state.busy ? "disabled" : "";
  return `<div class="modalOverlay" role="dialog" aria-modal="true" data-testid="add-account-modal">
    <div class="modal">
      <div class="modal__title">Add Account${state.selectedAccountSet ? ` for ${state.selectedAccountSet}` : ""}</div>
      <label class="field"><div class="field__label">Account Name (e.g., ethereum, bank:savings)</div><div class="field__prefix">assets:</div><input id="newAccountName" class="field__input" value="${escapeText(state.addAccountDraft.accountName)}" ${d} placeholder="ethereum" autocapitalize="off" autocorrect="off" spellcheck="false" /></label>
      <label class="field"><div class="field__label">Default Currency (optional)</div><input id="newAccountCurrency" class="field__input" value="${escapeText(state.addAccountDraft.currency)}" ${d} placeholder="USD" /></label>
      <label class="field"><div class="field__label">Opening Balance (optional)</div><input id="newAccountBalance" class="field__input" value="${escapeText(state.addAccountDraft.openingBalance)}" ${d} placeholder="0.00" /></label>
      ${state.addAccountDraft.error ? `<div class="modal__error">${escapeText(state.addAccountDraft.error)}</div>` : ""}
      <div class="modal__actions"><button id="addAccountCancel" class="btn btn--secondary" ${d}>Cancel</button><button id="addAccountSubmit" class="btn" ${d}>Add</button></div>
    </div>
  </div>`;
}

function renderOpeningBalanceModal(state: AppState): string {
  if (!state.openingBalanceDraft) return "";
  const d = state.busy ? "disabled" : "";
  const ob = state.openingBalanceDraft;
  return `<div class="modalOverlay" role="dialog" aria-modal="true" data-testid="opening-balance-modal">
    <div class="modal">
      <div class="modal__title">Set Opening Balance</div>
      <div class="modal__info">${escapeText(ob.accountName)}</div>
      <div class="openingMode">
        <label><input type="radio" name="openingMode" value="direct" ${ob.mode === "direct" ? "checked" : ""} />I know the opening balance</label>
        <label><input type="radio" name="openingMode" value="from-date" ${ob.mode === "from-date" ? "checked" : ""} />I know a balance at a date</label>
      </div>
      ${ob.mode === "direct" ? `
        <label class="field"><div class="field__label">Opening Balance</div><input id="openingAmount" class="field__input" value="${escapeText(ob.amount)}" ${d} placeholder="0.00" /></label>
        <label class="field"><div class="field__label">Currency</div><input id="openingCommodity" class="field__input" value="${escapeText(ob.commodity)}" ${d} placeholder="AUD" /></label>
      ` : `
        <label class="field"><div class="field__label">Date of known balance</div><input id="openingDate" class="field__input" type="date" value="${escapeText(ob.date)}" ${d} /></label>
        <label class="field"><div class="field__label">Known balance at that date</div><input id="openingKnownBalance" class="field__input" value="${escapeText(ob.knownBalance)}" ${d} placeholder="0.00" /></label>
        <label class="field"><div class="field__label">Currency</div><input id="openingCommodity" class="field__input" value="${escapeText(ob.commodity)}" ${d} placeholder="AUD" /></label>
        ${ob.calculatedOpening != null ? `<div class="modal__result">Calculated opening: ${escapeText(ob.calculatedOpening)} ${escapeText(ob.commodity)}</div>` : ""}
      `}
      ${ob.error ? `<div class="modal__error">${escapeText(ob.error)}</div>` : ""}
      <div class="modal__actions"><button id="openingBalanceCancel" class="btn btn--secondary" ${d}>Cancel</button><button id="openingBalanceSubmit" class="btn" ${d}>Save</button></div>
    </div>
  </div>`;
}

function renderSetPriceModal(state: AppState): string {
  if (!state.setPriceDraft) return "";
  const d = state.busy ? "disabled" : "";
  const sp = state.setPriceDraft;
  return `<div class="modalOverlay" role="dialog" aria-modal="true" data-testid="set-price-modal">
    <div class="modal">
      <div class="modal__title">Set Price</div>
      <label class="field"><div class="field__label">Date</div><input id="setPriceDate" class="field__input" value="${escapeText(sp.datetime)}" ${d} /></label>
      <label class="field"><div class="field__label">Commodity</div><input id="setPriceCommodity" class="field__input" value="${escapeText(sp.commodity)}" disabled /></label>
      <label class="field"><div class="field__label">Price</div><input id="setPriceAmount" class="field__input" value="${escapeText(sp.priceAmount)}" ${d} placeholder="0.00" /></label>
      <label class="field"><div class="field__label">Quote Currency</div><input id="setPriceQuoteCurrency" class="field__input" value="${escapeText(sp.quoteCurrency)}" ${d} placeholder="USD" /></label>
      ${sp.error ? `<div class="modal__error">${escapeText(sp.error)}</div>` : ""}
      <div class="modal__actions"><button id="setPriceCancel" class="btn btn--secondary" ${d}>Cancel</button><button id="setPriceSave" class="btn" ${d}>Save</button></div>
    </div>
  </div>`;
}

function renderTransformAiStatus(td: NonNullable<AppState["transformDraft"]>): string {
  if (td.aiStatus === "analyzing") {
    return `<div class="aiModal__steps" style="margin-bottom:8px">${(td.aiSteps ?? []).map((s, i) => { const isLast = i === (td.aiSteps ?? []).length - 1; return `<div class="aiModal__step ${isLast ? "aiModal__step--active" : "aiModal__step--done"}"><span class="aiModal__stepIcon">${isLast ? '<span class="aiModal__spinner"></span>' : "\u2713"}</span>${escapeText(s)}</div>`; }).join("")}</div>`;
  }
  if (td.aiStatus === "done") {
    return `<div style="font-size:12px;color:#059669;margin-bottom:8px">\u2713 AI generated transform — review and edit as needed</div>`;
  }
  if (td.aiStatus === "error") {
    return `<div class="modal__error">${escapeText(td.aiError ?? "AI generation failed")}</div>${td.aiRawOutput ? `<details style="margin-bottom:8px"><summary style="font-size:12px;cursor:pointer">Claude output</summary><pre style="font-size:11px;max-height:150px;overflow:auto;background:#1a1a2e;color:#e0e0e0;padding:8px;border-radius:4px">${escapeText(td.aiRawOutput)}</pre></details>` : ""}`;
  }
  return "";
}

function renderTransformCsvInfo(td: NonNullable<AppState["transformDraft"]>): string {
  let html = "";
  if (td.csvFilename) html += `<div class="modal__subtitle">Importing: ${escapeText(td.csvFilename)}</div>`;
  if (td.headers.length > 0) html += `<div class="modal__headers">Available columns: <code>${td.headers.map(h => escapeText(h)).join("</code>, <code>")}</code></div>`;
  if (td.csvTypes && td.csvTypes.length > 1) html += `<div class="modal__info" style="font-size:12px;color:#6b7280;margin-bottom:8px">Detected ${td.csvTypes.length} CSV formats: ${td.csvTypes.map(t => `<strong>${escapeText(t.pattern)}</strong> (${t.file_count} file${t.file_count === 1 ? "" : "s"})`).join(", ")}</div>`;
  return html;
}

function renderTransformModal(state: AppState): string {
  if (!state.transformDraft) return "";
  const d = state.busy ? "disabled" : "";
  const td = state.transformDraft;
  const aiAnalyzing = td.aiStatus === "analyzing";
  return `<div class="modalOverlay" role="dialog" aria-modal="true" data-testid="transform-modal">
    <div class="modal">
      <div class="modal__title">${td.sourcePath ? "Configure CSV Transform" : "Edit Transform"}</div>
      ${renderTransformCsvInfo(td)}
      <label class="field">
        <div class="field__label">Rhai Transform Script
          <button id="aiTransformBtn" class="ai-sparkle-btn${aiAnalyzing ? " ai-sparkle-btn--active" : ""}" title="Generate with AI" ${aiAnalyzing ? "disabled" : ""}><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M8 0L9.79 6.21L16 8L9.79 9.79L8 16L6.21 9.79L0 8L6.21 6.21L8 0Z" fill="currentColor"/></svg></button>
        </div>
        <textarea id="transformScript" class="field__textarea" rows="15" ${state.busy || aiAnalyzing ? "disabled" : ""}>${escapeText(td.script)}</textarea>
      </label>
      ${renderTransformAiStatus(td)}
      ${td.error ? `<div class="modal__error">${escapeText(td.error)}</div>` : ""}
      <div class="modal__actions"><button id="transformCancel" class="btn btn--secondary" ${d}>Cancel</button><button id="transformSave" class="btn" ${state.busy || aiAnalyzing ? "disabled" : ""}>${td.sourcePath ? "Save & Import" : "Save"}</button></div>
    </div>
  </div>`;
}

function renderSyncLogModal(state: AppState): string {
  if (!state.syncLogOpen) return "";
  return `<div class="modalOverlay" role="dialog" aria-modal="true" data-testid="sync-log-modal">
    <div class="modal modal--wide">
      <div class="modal__title">Sync Log</div>
      ${state.devices && state.devices.length > 0 ? `<div class="syncLogDevices"><strong>Known devices:</strong> ${state.devices.map((d) => `<span class="syncLogDevice">${escapeText(d.device_name)} (${escapeText(d.device_id)}) — last seen ${new Date(d.last_seen * 1000).toLocaleString()}</span>`).join(", ")}</div>` : ""}
      ${state.syncLog && state.syncLog.length > 0 ? `<table class="syncLogTable"><thead><tr><th>Time</th><th>Device</th><th>Event</th><th>Details</th></tr></thead><tbody>${state.syncLog.slice().reverse().map((e) => `<tr><td>${escapeText(new Date(e.timestamp * 1000).toLocaleString())}</td><td>${escapeText(e.device_id)}</td><td><span class="syncLogEvent__type">${escapeText(e.event_type)}</span></td><td>${escapeText(e.details)}</td></tr>`).join("")}</tbody></table>` : `<div class="syncLogEmpty">No sync events yet.</div>`}
      <div class="modal__actions"><button id="syncLogExport" class="btn btn--secondary">Export JSON</button><button id="syncLogClose" class="btn">Close</button></div>
    </div>
  </div>`;
}

function renderRelayPairingModal(state: AppState): string {
  if (!state.relayPairingOpen) return "";
  const d = state.busy ? "disabled" : "";
  let body: string;
  if (!state.relayPairingMode) {
    body = `<label class="field"><div class="field__label">Relay Server URL</div><input id="relayUrlInput" class="field__input" value="" placeholder="http://your-server:8384" ${d} /></label>
      <div class="modal__actions" style="margin-top:12px"><button id="relayPairCancel" class="btn btn--secondary" ${d}>Cancel</button><button id="relayCreateCode" class="btn btn--secondary" ${d}>Create Code</button><button id="relayEnterCode" class="btn" ${d}>Enter Code</button></div>`;
  } else if (state.relayPairingMode === "create") {
    body = `<div class="pairingCode"><div class="pairingCode__label">Share this code with the other device:</div><div class="pairingCode__value">${escapeText(state.relayPairingCode ?? "")}</div></div>
      ${state.relayPairingError ? `<div class="modal__error">${escapeText(state.relayPairingError)}</div>` : ""}
      <div class="modal__actions"><button id="relayPairDone" class="btn">Done</button></div>`;
  } else {
    body = `<label class="field"><div class="field__label">Enter the 6-digit pairing code</div><input id="relayJoinCodeInput" class="field__input" maxlength="6" placeholder="000000" ${d} /></label>
      ${state.relayPairingError ? `<div class="modal__error">${escapeText(state.relayPairingError)}</div>` : ""}
      <div class="modal__actions"><button id="relayPairCancel2" class="btn btn--secondary" ${d}>Cancel</button><button id="relayJoinSubmit" class="btn" ${d}>Join</button></div>`;
  }
  return `<div class="modalOverlay" role="dialog" aria-modal="true" data-testid="relay-pairing-modal"><div class="modal"><div class="modal__title">Pair Device</div>${body}</div></div>`;
}

function renderAiSuggestModal(state: AppState): string {
  if (!state.aiSuggest) return "";
  const ai = state.aiSuggest;
  const stepsHtml = ai.steps.length > 0 ? `
    <details class="aiModal__log" ${ai.status === "analyzing" ? "open" : ""}>
      <summary class="aiModal__logSummary">${ai.status === "analyzing" ? `<span class="aiModal__spinner aiModal__spinner--small"></span> Processing\u{2026}` : `\u2713 Completed (${ai.steps.length} steps)`}</summary>
      <div class="aiModal__steps">${ai.steps.map((s, i) => { const isLast = i === ai.steps.length - 1; const isDone = !isLast || ai.status !== "analyzing"; return `<div class="aiModal__step ${isDone ? "aiModal__step--done" : "aiModal__step--active"}"><span class="aiModal__stepIcon">${isDone ? "\u2713" : `<span class="aiModal__spinner"></span>`}</span>${escapeText(s)}</div>`; }).join("")}</div>
    </details>` : ai.status === "analyzing" ? `
    <div class="aiModal__steps"><div class="aiModal__step aiModal__step--active"><span class="aiModal__stepIcon"><span class="aiModal__spinner"></span></span>Starting\u{2026}</div></div>` : "";
  let bodyHtml = "";
  if (ai.status === "error") {
    bodyHtml = `<div class="modal__error">${escapeText(ai.error ?? "Unknown error")}</div>${ai.rawOutput ? `<details class="aiModal__rawOutput"><summary>Claude output</summary><pre class="aiModal__rawPre">${escapeText(ai.rawOutput)}</pre></details>` : ""}`;
  } else if (ai.status === "done") {
    bodyHtml = `<div class="aiModal__suggestions">${(ai.suggestions ?? []).map((s, i) => `
      <div class="aiModal__suggestion ${ai.appliedIndices?.has(i) ? "aiModal__suggestion--applied" : ""}" data-testid="ai-suggestion">
        <div class="aiModal__suggestionHeader"><span class="aiModal__suggestionPattern">${escapeText(s.pattern)}</span><span class="aiModal__suggestionArrow">\u2192</span><span class="aiModal__suggestionAccount">${escapeText(s.amount_account)}</span></div>
        ${s.payee ? `<div class="aiModal__suggestionPayee">Payee: ${escapeText(s.payee)}</div>` : ""}
        <div class="aiModal__suggestionExplanation">${escapeText(s.explanation)}</div>
        <div class="aiModal__suggestionActions">${ai.appliedIndices?.has(i) ? `<span style="color:#059669;font-size:12px">\u2713 Applied</span>` : `<button class="btn btn--small" data-ai-apply="${i}">Edit Rule</button><button class="btn btn--secondary btn--small" data-ai-skip="${i}">Skip</button>`}</div>
      </div>`).join("")}</div>
    ${(ai.suggestions ?? []).length === 0 ? `<div style="color:#6b7280;padding:12px">No suggestions were generated.</div>` : ""}
    ${ai.rawOutput ? `<details class="aiModal__rawOutput"><summary>Claude output</summary><pre class="aiModal__rawPre">${escapeText(ai.rawOutput)}</pre></details>` : ""}`;
  }
  return `<div class="modalOverlay" role="dialog" aria-modal="true" data-testid="ai-suggest-modal">
    <div class="modal aiModal">
      <div class="modal__title">AI Suggest</div>
      <div class="aiModal__body">${bodyHtml}${stepsHtml}</div>
      <div class="aiModal__bulkActions"><button id="aiSuggestClose" class="btn btn--secondary">${ai.status === "analyzing" ? "Cancel" : "Close"}</button></div>
    </div>
  </div>`;
}

function renderModals(state: AppState): string {
  return renderManualTxnModal(state)
    + renderAddAccountModal(state)
    + renderOpeningBalanceModal(state)
    + renderSetPriceModal(state)
    + renderTransformModal(state)
    + renderSyncLogModal(state)
    + renderRelayPairingModal(state)
    + renderAiSuggestModal(state);
}

async function toggleTxnExpand(state: AppState, row: HTMLTableRowElement, expandKey: string): Promise<void> {
  if (!state.txExpandedRows) state.txExpandedRows = new Set();
  if (state.txExpandedRows.has(expandKey)) {
    state.txExpandedRows.delete(expandKey);
    return;
  }
  state.txExpandedRows.add(expandKey);
  const categoryCell = row.querySelector<HTMLTableCellElement>('[data-rule-type="category"]');
  const accountFolder = categoryCell?.getAttribute("data-account-folder") ?? "";
  if (accountFolder && !state.accountConfigs?.has(accountFolder)) {
    try {
      const config = await invoke<{ explorer_url?: string }>("get_account_config", { accountFolder });
      if (!state.accountConfigs) state.accountConfigs = new Map();
      state.accountConfigs.set(accountFolder, config);
    } catch { /* ignore — no config available */ }
  }
}

type BalanceFormResult = { amount: string; commodity: string; error?: undefined } | { error: string; amount?: undefined; commodity?: undefined };

function readDirectBalanceForm(commodity: string): BalanceFormResult {
  const amount = document.querySelector<HTMLInputElement>("#openingAmount")?.value?.trim() ?? "";
  if (!amount) return { error: "Please enter an amount" };
  if (!commodity) return { error: "Please enter a currency" };
  return { amount, commodity };
}

function sumPostingsUpToDate(transactions: Transaction[], acctName: string, commodity: string, cutoffDate: string): number {
  let sum = 0;
  for (const txn of transactions) {
    if (txn.date > cutoffDate) continue;
    for (const p of txn.postings) {
      if (p.account === acctName && p.commodity === commodity) sum += p.amount;
    }
  }
  return sum;
}

function readFromDateBalanceForm(state: AppState, commodity: string, acctName: string): BalanceFormResult {
  const draft = state.openingBalanceDraft!;
  const date = document.querySelector<HTMLInputElement>("#openingDate")?.value?.trim() ?? "";
  const knownBalance = document.querySelector<HTMLInputElement>("#openingKnownBalance")?.value?.trim() ?? "";
  draft.date = date;
  draft.knownBalance = knownBalance;
  if (!date || !knownBalance) return { error: "Please enter both date and balance" };
  if (!commodity) return { error: "Please enter a currency" };

  const postingSum = sumPostingsUpToDate(state.parse?.transactions ?? [], acctName, commodity, date);
  const amount = (parseFloat(knownBalance) - postingSum).toFixed(2);
  draft.calculatedOpening = amount;
  return { amount, commodity };
}

async function handleGroupTradeLink(btn: HTMLButtonElement, state: AppState, groupSellJson: string): Promise<void> {
  const sellIds: string[] = JSON.parse(groupSellJson);
  const buyIds: string[] = JSON.parse(btn.getAttribute("data-chain-group-buy") ?? "[]");
  const accountFolder = btn.getAttribute("data-chain-account-folder") ?? "";
  const pairCount = Math.min(sellIds.length, buyIds.length);
  if (pairCount === 0) return;
  const orphanCount = Math.abs(sellIds.length - buyIds.length);

  state.status = `Linking ${pairCount} trade pair${pairCount === 1 ? "" : "s"}...`;
  const links = [];
  for (let i = 0; i < pairCount; i++) {
    links.push({ txn_id_a: sellIds[i], txn_id_b: buyIds[i], account_folder: accountFolder, is_a_sell: true });
  }
  const response = await invoke<PipelineResponse>("save_trade_links_bulk", {
    nowYyyymm: nowYYYYMM(), links, accountSet: state.selectedAccountSet ?? "",
  });
  await applyPipelineResponse(state, response);
  const orphanMsg = orphanCount > 0 ? ` (${orphanCount} unmatched remaining)` : "";
  state.status = `Linked ${pairCount} trade pair${pairCount === 1 ? "" : "s"}${orphanMsg}.`;
}

async function handleSingleTradeLink(btn: HTMLButtonElement, state: AppState): Promise<void> {
  const txnIdA = btn.getAttribute("data-chain-txn-a") ?? "";
  const txnIdB = btn.getAttribute("data-chain-txn-b") ?? "";
  const isLinked = btn.getAttribute("data-chain-linked") === "true";
  const accountFolder = btn.getAttribute("data-chain-account-folder") ?? "";
  const isASell = btn.getAttribute("data-chain-is-a-sell") === "true";
  if (!txnIdA || !txnIdB) return;

  if (isLinked) {
    const link = state.tradeLinks.find((l) =>
      (l.txn_id_a === txnIdA && l.txn_id_b === txnIdB) || (l.txn_id_a === txnIdB && l.txn_id_b === txnIdA)
    );
    if (link) {
      state.status = "Unlinking trade...";
      const response = await invoke<PipelineResponse>("delete_trade_link", {
        nowYyyymm: nowYYYYMM(), linkId: link.id, accountFolder, accountSet: state.selectedAccountSet ?? "",
      });
      await applyPipelineResponse(state, response);
      state.status = "Trade unlinked.";
    }
  } else {
    state.status = "Linking transactions...";
    const response = await invoke<PipelineResponse>("save_trade_link", {
      nowYyyymm: nowYYYYMM(), txnIdA, txnIdB, accountFolder, isASell, accountSet: state.selectedAccountSet ?? "",
    });
    await applyPipelineResponse(state, response);
    state.status = "Transactions linked.";
  }
}

async function refreshTradeLinks(state: AppState): Promise<void> {
  state.tradeLinks = await invoke<TradeLink[]>("get_trade_links");
  try {
    state.tradeSuggestions = await invoke<TradeSuggestion[]>("suggest_trade_links_cmd", {
      accountSet: state.selectedAccountSet ?? "", baseCurrency: state.displayConfig?.base_currency ?? null,
    });
  } catch { /* ignore */ }
}

function readOpeningBalanceForm(state: AppState): BalanceFormResult {
  const draft = state.openingBalanceDraft!;
  draft.error = undefined;
  const commodity = document.querySelector<HTMLInputElement>("#openingCommodity")?.value?.trim() ?? draft.commodity;
  draft.commodity = commodity;
  if (draft.mode === "direct") {
    draft.amount = document.querySelector<HTMLInputElement>("#openingAmount")?.value?.trim() ?? "";
    return readDirectBalanceForm(commodity);
  }
  return readFromDateBalanceForm(state, commodity, draft.accountName);
}

async function loadPreviewScope(state: AppState, filterPills: SearchPill[], accountPrefix: string): Promise<Transaction[]> {
  let txns = await filterBySearch(state.selectedAccountSet ?? "", filterPills, accountPrefix);
  if (txns.length === 0 && filterPills.length === 0) {
    txns = filterByAccountPrefix(state.parse?.transactions ?? [], accountPrefix);
  }
  return txns;
}

function readAndBuildRule(draft: AppState["ruleEditorDraft"] & {}) {
  const ruleKeywords = [...TRANSACTION_SEARCH_KEYWORDS, "field"] as string[];
  return buildRuleFromDraft({
    pattern: draft.pattern,
    matchField: draft.matchField,
    amountCondition: draft.amountCondition,
    feeCondition: draft.feeCondition,
    payeeCondition: draft.payeeCondition,
    narrationCondition: draft.narrationCondition,
    commodityCondition: draft.commodityCondition,
    metaCondition: draft.metaCondition,
    filterPills: draft.filterPills,
    comment: document.querySelector<HTMLInputElement>("#ruleComment")?.value ?? draft.comment,
    amountAccount: document.querySelector<HTMLInputElement>("#ruleAmountAccount")?.value?.trim() ?? "",
    feeAccount: document.querySelector<HTMLInputElement>("#ruleFeeAccount")?.value?.trim() ?? "",
  }, parseSmartInput, toFlatString, ruleKeywords);
}

async function invokeRuleSave(
  draft: AppState["ruleEditorDraft"] & {},
  saved: ReturnType<typeof buildRuleFromDraft>,
  accountSet: string,
): Promise<MutationResponse> {
  if (draft.ruleId) {
    return invoke<MutationResponse>("update_rule", {
      nowYyyymm: nowYYYYMM(), accountFolder: draft.accountFolder,
      rule: { id: draft.ruleId, ...saved },
      targetFolder: targetFolderForScope(draft.accountFolder, draft.ruleScope),
      accountSet,
    });
  }
  const targetFolder = targetFolderForScope(draft.accountFolder, draft.ruleScope);
  return invoke<MutationResponse>("save_rule",
    buildSaveRuleArgs(saved, targetFolder, accountSet, nowYYYYMM()));
}

function restoreFocusAfterRender(activeId: string | undefined, cursorPos: number | null): void {
  const toFocus = activeId ? document.getElementById(activeId) : null;
  if (toFocus && toFocus instanceof HTMLInputElement) {
    toFocus.focus();
    if (cursorPos != null) {
      const pos = Math.min(cursorPos, toFocus.value.length);
      toFocus.selectionStart = toFocus.selectionEnd = pos;
    }
  }
}

type RenderViewState = {
  balances: AccountBalance[];
  tree: TreeNode[];
  selectedAccount: string | undefined;
  selectedBalance: AccountBalance | undefined;
  selectedTotal: CommodityAmount | undefined;
  allTransactions: Transaction[];
  filteredTransactions: Transaction[];
  searchFiltered: Transaction[];
  tradeLinkMap: Map<string, { link: TradeLink; partnerId: string }>;
  uncategorisedCount: number;
  showValueColumn: boolean;
  issueGroups: IssueGroup[];
  totalIssueCount: number;
  issueCounts: Map<string, number>;
  isUncategorisedTxn: (t: Transaction) => boolean;
  baseCurrency: string | undefined;
  isReportsView: boolean;
  isPluginsView: boolean;
};

function resolveSelectedAccount(state: AppState, balances: AccountBalance[], tree: TreeNode[]): string | undefined {
  const renderDefaultAccount = state.displayConfig?.default_account;
  const requestedAccount = state.selectedAccount;
  if (balances.length === 0) return requestedAccount;

  const selectedStillVisible = requestedAccount ? !!findTreeNode(tree, requestedAccount) : false;
  const selected =
    (selectedStillVisible ? requestedAccount : undefined) ??
    (renderDefaultAccount && balances.find((b) => matchesPrefix(b.account, renderDefaultAccount))?.account) ??
    balances[0]?.account;
  state.selectedAccount = selected;
  if (!selectedStillVisible && selected !== requestedAccount) {
    state.drillPath = drillPathForAccount(selected, tree);
  }
  return selected;
}

function computeTransactionViewState(state: AppState, selectedAccount: string | undefined, baseCurrency: string | undefined) {
  const realTxns = state.searchFilteredTransactions ?? [];
  // Splice optimistic adds (this account, not yet superseded by a real row) in
  // front of the loaded window so a newly-added txn shows instantly.
  const allTransactions = [...pendingAddsForView(state.pendingAdds, selectedAccount, realTxns), ...realTxns];
  const isUncategorisedTxn = (t: Transaction) => {
    const other = t.postings.find((p) => p.account !== selectedAccount);
    return !other || t.postings.some((p) => p.account === "expenses:unknown");
  };
  const filteredTransactions = state.issueFilter === "uncategorised" && selectedAccount
    ? allTransactions.filter(isUncategorisedTxn)
    : allTransactions;
  const tradeLinkMap = new Map<string, { link: TradeLink; partnerId: string }>();
  for (const link of state.tradeLinks) {
    tradeLinkMap.set(link.txn_id_a, { link, partnerId: link.txn_id_b });
    tradeLinkMap.set(link.txn_id_b, { link, partnerId: link.txn_id_a });
  }
  const uncategorisedCount = selectedAccount ? allTransactions.filter(isUncategorisedTxn).length : 0;
  const sampleCommodity = selectedAccount && allTransactions.length > 0
    ? allTransactions.find((t) => t.postings.some((p) => matchesPrefix(p.account, selectedAccount!)))
        ?.postings.find((p) => matchesPrefix(p.account, selectedAccount!))?.commodity
    : undefined;
  const showValueColumn = !!(baseCurrency && state.transactionValues && sampleCommodity && sampleCommodity !== baseCurrency);
  return { allTransactions, searchFiltered: allTransactions, filteredTransactions, tradeLinkMap, uncategorisedCount, showValueColumn, isUncategorisedTxn };
}

/** Pick the balances/tree/selection for the active left-pane (Accounts vs the
 *  Categories complement partition) and lazily initialise its drill path. The
 *  Categories pane reuses the entire accounts render path over a different
 *  partition + selection slice, so surfacing them here keeps the topbar, header,
 *  drill-down and transaction table free of pane branching. */
function resolveActivePartition(
  state: AppState,
): { balances: AccountBalance[]; tree: TreeNode[]; selectedAccount: string | undefined } {
  const isCategoriesView = state.sidebarView === "categories";
  const balances = isCategoriesView
    ? filterCategoryBalances(state.parse?.balances ?? [], state.accountFoldersMap, state.showHidden)
    : filterSidebarBalances(
        state.parse?.balances ?? [], state.accountFoldersMap, state.accountSetMap, state.selectedAccountSet,
      );
  const tree = buildAccountTree(balances);
  // Categories open at the roots (no default-account selection); Accounts may
  // auto-select a default folder account.
  const selectedAccount = isCategoriesView
    ? state.selectedCategory
    : resolveSelectedAccount(state, balances, tree);

  if (isCategoriesView) {
    if (selectedAccount && !state._categoryDrillInitialized) {
      state.categoryDrillPath = drillPathForAccount(selectedAccount, tree, null);
      state._categoryDrillInitialized = true;
    }
  } else if (selectedAccount && !state._drillInitialized) {
    state.drillPath = drillPathForAccount(selectedAccount, tree);
    state._drillInitialized = true;
  }
  return { balances, tree, selectedAccount };
}

function computeRenderViewState(state: AppState): RenderViewState {
  const isReportsView = state.sidebarView === "reports";
  const isPluginsView = state.sidebarView === "plugins";
  const baseCurrency = state.displayConfig?.base_currency;
  const empty: RenderViewState = {
    balances: [], tree: [], selectedAccount: undefined, selectedBalance: undefined, selectedTotal: undefined,
    allTransactions: [], filteredTransactions: [], searchFiltered: [], tradeLinkMap: new Map(),
    uncategorisedCount: 0, showValueColumn: false, issueGroups: [], totalIssueCount: 0,
    issueCounts: new Map(), isUncategorisedTxn: () => false, baseCurrency, isReportsView, isPluginsView,
  };
  if (isReportsView || isPluginsView) return empty;

  const { balances, tree, selectedAccount } = resolveActivePartition(state);

  const selectedBalance = selectedAccount
    ? (state.prefixQuery
        ? { account: selectedAccount, totals: state.prefixQuery.aggregated_balance }
        : balances.find((b) => b.account === selectedAccount))
    : undefined;
  const selectedTotal = selectedBalance ? pickDisplayTotal(selectedBalance.totals) : undefined;
  const txState = computeTransactionViewState(state, selectedAccount, baseCurrency);
  const issueGroups = collectIssues(state, selectedAccount);

  return {
    balances, tree, selectedAccount, selectedBalance, selectedTotal,
    ...txState, issueGroups,
    totalIssueCount: issueGroups.reduce((sum, g) => sum + g.issues.length, 0),
    issueCounts: buildAccountIssueCounts(state),
    baseCurrency, isReportsView, isPluginsView,
  };
}

function collectAccountSuggestionsFrom(state: AppState): string[] {
  return collectAccountSuggestions({
    accountSetMap: state.accountSetMap,
    balances: state.parse?.balances,
    transactions: state.parse?.transactions,
    allAccounts: state.allAccounts,
  });
}

/** Distinct payees from the parsed ledger, for the manual modal's payee
 *  autocomplete. Includes display_payee so abbreviated names match too. */
function collectPayeeSuggestionsFrom(state: AppState): string[] {
  const payees = new Set<string>();
  for (const t of state.parse?.transactions ?? []) {
    if (t.payee) payees.add(t.payee);
    if (t.display_payee) payees.add(t.display_payee);
  }
  return [...payees].sort();
}

function renderVaultPickerScreen(state: AppState): void {
  app.innerHTML = renderVaultPicker(state);
  document.getElementById("vaultPickerOpen")?.addEventListener("click", async () => {
    const selected = await open({ directory: true, multiple: false });
    if (selected) {
      try {
        await invoke("set_root_dir", { path: selected });
        state.rootDir = selected as string;
        state.showVaultPicker = false;
        render(state);
        await normalStartup();
      } catch (e) {
        console.error("Failed to set root dir:", e);
      }
    }
  });
  app.querySelectorAll<HTMLButtonElement>(".vaultPicker__knownItem").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const rootPath = btn.dataset.root!;
      try {
        await invoke("set_root_dir", { path: rootPath });
        state.rootDir = rootPath;
        state.showVaultPicker = false;
        render(state);
        await normalStartup();
      } catch (e) {
        console.error("Failed to set root dir:", e);
      }
    });
  });
}

function render(state: AppState): void {
  const vs = computeRenderViewState(state);

  const diagnostics = state.parse?.diagnostics ?? [];
  const statusText =
    state.status ??
    (state.parse
      ? state.parse.ok
        ? `Parsed (${(state.parse?.transactions ?? []).length} transactions)`
        : `Parsed with ${diagnostics.length} diagnostics`
      : "Pick a file to begin.");

  if (state.showVaultPicker) {
    renderVaultPickerScreen(state);
    return;
  }

  let html: string;
  try {
    html = buildMainHtml(state, vs, statusText, diagnostics);
  } catch (renderErr) {
    console.error("Render failed:", renderErr);
    app.innerHTML = `<div style="padding:24px;color:red;font-family:monospace;white-space:pre-wrap"><h2>Render Error</h2>${String(renderErr)}\n${(renderErr as Error)?.stack ?? ""}</div>`;
    return;
  }

  // Preserve scroll position across re-renders. With morphdom this is
  // mostly free (the .content node survives), but we still capture for
  // the fallback first-render path.
  const contentEl = app.querySelector(".content");
  const scrollY = contentEl?.scrollTop ?? 0;

  morphIntoApp(html);

  // Restore scroll position (mostly a no-op in the morphdom path since
  // .content survives, but still needed for the first-render fallback).
  const restoredEl = app.querySelector(".content");
  if (restoredEl) restoredEl.scrollTop = scrollY;

  // attachRenderHandlers re-binds many per-element click handlers. With
  // morphdom preserving elements across renders, naive re-binding would
  // stack a fresh closure on every render. bindOnceDuring removes the
  // listeners attached during the previous render before this one
  // attaches its own — so each (element, event, handler-slot) carries
  // exactly the latest closure.
  bindOnceDuring(() => attachRenderHandlers(state, vs));

  // Reconcile the performance chart (created/updated/destroyed imperatively;
  // its mount survives morphdom re-renders via data-morph-preserve). On any
  // non-Performance view the mount isn't rendered, so this tears the chart down.
  syncPerformanceChart(state.performanceReport);
  syncPerformanceGrowthChart(state.performanceReport);

  // The fade-in animation is one-shot — clear the set so subsequent renders
  // (scroll, sort, filter) don't re-fade rows that already settled in.
  state.justAddedTxnIds = undefined;
}

// Non-intrusive DOM diff: unchanged nodes stay put, preserving focus,
// in-flight CSS transitions, and listeners. Architectural requirement:
// UI works on the local DOM; the new HTML describes what the DOM should
// look like, not a wholesale replacement. See morphInPlace + tests in
// src/dom-update.ts for the contract.
function morphIntoApp(html: string): void {
  if (!app.firstElementChild) {
    app.innerHTML = html;
    return;
  }
  morphInPlace(app, `<div id="${app.id}">${html}</div>`);
}

function buildAccountHeaderHtml(state: AppState, selectedAccount: string | undefined, selectedBalance: AccountBalance | undefined, selectedTotal: CommodityAmount | undefined, tree: TreeNode[]): string {
  if (!selectedAccount) return `<div class="accountHeader__name" data-testid="selected-account">No account</div>`;
  if (isFolderSelection(selectedAccount, tree)) {
    const setLabel = state.selectedAccountSet ?? state.accountSets[0] ?? "";
    const display = folderDisplayName(selectedAccount, setLabel.charAt(0).toUpperCase() + setLabel.slice(1));
    return `<div class="accountHeader__name" data-testid="selected-account">${escapeText(display)}</div>`;
  }
  const hasFolder = !!state.accountFoldersMap[selectedAccount];
  if (hasFolder) {
    const parts = selectedAccount.split(":");
    const institution = parts[1] ?? "";
    const accountSeg = parts.length > 2 ? parts.slice(2).join(":") : "";
    return `
      <div class="accountHeader__name" data-testid="selected-account">
        <span class="accountHeader__seg">${escapeText(institution)}</span>
        <span class="accountHeader__segSep">:</span>
        <span class="accountHeader__seg">${escapeText(accountSeg || "\u2026")}</span>
      </div>`;
  }
  const parts = selectedAccount.split(":");
  const afterRoot = parts.slice(1).join(":") || selectedAccount;
  return `<div class="accountHeader__name accountHeader__name--endTrunc" data-testid="selected-account"><bdo dir="ltr">${escapeText(afterRoot)}</bdo></div>`;
}

function buildBalanceHtml(state: AppState, selectedBalance: AccountBalance | undefined, selectedTotal: CommodityAmount | undefined, isFolder: boolean, folderTotal: number | undefined): string {
  const totals = selectedBalance?.totals ?? [];
  if (!selectedTotal) return "";
  const tv = state.accountTotalValue;
  const hasMultiple = totals.length > 1;
  const baseCurrency = (isFolder && state.displayConfig?.base_currency)
    ? state.displayConfig.base_currency
    : (tv ? tv.currency : selectedTotal.commodity);
  // Folder mode reads from treeBaseTotals so the header equals the sum of
  // visible sidebar rows by construction. Leaf accounts keep the existing
  // accountTotalValue path (driven by the backend prefix query).
  const baseAmount = isFolder && folderTotal !== undefined
    ? folderTotal
    : (tv ? tv.total : selectedTotal.amount);
  const mainAmount = isFolder ? formatFolderAmount(baseAmount, baseCurrency) : formatAmount(baseAmount, baseCurrency);
  const mainCurrency = baseCurrency;
  const balRows = hasMultiple ? (tv?.bycommodity ?? totals.map((t) => ({ commodity: t.commodity, amount: t.amount, value: null as number | null }))).map((t) => `<div class="accountHeader__balRow">
    <span class="accountHeader__balAmount">${escapeText(formatAmount(t.amount, t.commodity))}</span>
    <span class="accountHeader__balCurrency">${escapeText(t.commodity)}</span>
    ${t.value !== null && t.commodity !== mainCurrency ? `<span class="accountHeader__balValue">\u2248 ${escapeText(formatAmount(t.value, mainCurrency))} ${escapeText(mainCurrency)}</span>` : ""}
  </div>`).join("") : "";
  return `<div class="accountHeader__balance ${hasMultiple ? "accountHeader__balance--expandable" : ""} ${isFolder ? "accountHeader__balance--folder" : ""}" ${hasMultiple ? 'id="balanceToggle"' : ""}>
    ${escapeText(mainAmount)} <span class="accountHeader__currency">${escapeText(mainCurrency)}</span>
    ${hasMultiple ? `<span class="accountHeader__arrow">&#9662;</span>` : ""}
  </div>
  ${hasMultiple ? `<div class="accountHeader__allBalances" id="allBalances" style="display:none">${balRows}</div>` : ""}`;
}

function buildUnverifiedHtml(state: AppState, selectedAccount: string | undefined, tree: TreeNode[]): string {
  if (!selectedAccount) return "";
  const hasBalance = (state.parse?.balances ?? []).some((b) => b.account === selectedAccount);
  const hasOpening = new Set(state.parse?.accounts_with_opening ?? []).has(selectedAccount);
  const isLeaf = (findTreeNode(tree, selectedAccount)?.children.length ?? 0) === 0;
  if (!hasBalance || hasOpening || !isLeaf) return "";
  return `<div class="accountHeader__unverified"><span>Unverified balance — no opening balance set</span> <button class="btn btn--small" data-set-opening="${escapeText(selectedAccount)}">Set Opening Balance</button></div>`;
}

function buildStatusLineHtml(state: AppState, statusText: string, uncategorisedCount: number): string {
  return `<div class="statusLine">
    <span class="status" data-testid="parse-status">${escapeText(statusText)}</span>
    ${uncategorisedCount > 0 ? `<span class="pill">${uncategorisedCount} uncategorised</span>` : ""}
    ${state.issueFilter ? `<button id="clearIssueFilter" class="pill pill--filter" type="button">Filtered: ${escapeText(state.issueFilter)} ✕</button>` : ""}
    ${state.tradeLinkSelection ? `<button id="cancelTradeLink" class="pill pill--filter" type="button">Select another transaction to link ✕</button>` : ""}
  </div>`;
}

function buildAccountsTopbar(state: AppState, vs: RenderViewState, statusText: string): string {
  const { selectedAccount, selectedBalance, selectedTotal, tree, uncategorisedCount } = vs;
  return `
  <header class="topbar">
    <div class="topbar__left">
      <div class="accountHeader">
        ${renderNavButtons(state)}
        ${buildAccountHeaderHtml(state, selectedAccount, selectedBalance, selectedTotal, tree)}
        ${buildBalanceHtml(state, selectedBalance, selectedTotal, isFolderSelection(selectedAccount, tree), selectedAccount && state.treeBaseTotals ? folderHeaderTotal(selectedAccount, tree, state.treeBaseTotals) : undefined)}
        ${buildUnverifiedHtml(state, selectedAccount, tree)}
      </div>
    </div>
    <div class="topbar__right">
      ${buildStatusLineHtml(state, statusText, uncategorisedCount)}
      <div class="searchWrapper">
        ${renderSmartSearch("accountSearch", state.searchPills, state.searchText, {
          keywords: [...TRANSACTION_SEARCH_KEYWORDS],
          placeholder: "Search (e.g. account:eth amount:>100 fee:>0)",
        }, !!state.busy)}
        ${state.searchError ? `<div class="searchError">${escapeText(state.searchError)}</div>` : ""}
      </div>
    </div>
  </header>`;
}

function buildTxTableBody(state: AppState, vs: RenderViewState): string {
  const { selectedAccount, allTransactions, filteredTransactions, tradeLinkMap,
    showValueColumn, isUncategorisedTxn, baseCurrency } = vs;
  // Truly optimistic delete: txns the user has clicked but the pipeline
  // hasn't yet processed are filtered out of the rendered list entirely.
  // Without this, rapid clicks accumulate as faded rows occupying space
  // until the pipeline catches up.
  const visible = filterPendingDeletes(filteredTransactions, state.pendingDeletes);
  const sorted = state.txSort && shouldClientSortLoadedTransactions(state.txSort)
    ? sortTransactions(visible, state.txSort, selectedAccount)
    : visible;
  const expandedGroups = state.txExpandedGroups ?? new Set<string>();
  const pageSize = Math.max(1, sorted.length);
  const { items } = paginateWithGroups(sorted, selectedAccount, tradeLinkMap, expandedGroups, pageSize);
  vsItems = items;
  const txnIndexMap = new Map<Transaction, number>();
  for (let ti = 0; ti < allTransactions.length; ti++) txnIndexMap.set(allTransactions[ti], ti);
  const txnIdMap = buildTxnIdMap(allTransactions);
  const renderedPartnerIds = new Set<string>();
  return renderGroupedRows(items, state, selectedAccount, allTransactions, isUncategorisedTxn, tradeLinkMap, renderedPartnerIds, baseCurrency, showValueColumn, txnIndexMap, txnIdMap);
}

function buildIssuesPanelHtml(state: AppState, vs: RenderViewState): string {
  const { issueGroups, totalIssueCount } = vs;
  if (totalIssueCount === 0 || state.sidebarView !== "accounts") return "";
  return `
    <section class="issuesPanel ${state.issuesPanelOpen === true ? "issuesPanel--open" : ""}">
      <button class="issuesPanel__tab" id="issuesTab" type="button">
        ISSUES <span class="issuesPanel__badge">${totalIssueCount}</span>
      </button>
      ${state.issuesPanelOpen === true ? `
        <div class="issuesPanel__body">
          ${issueGroups.map((g) => buildIssueGroupHtml(g)).join("")}
        </div>
      ` : ""}
    </section>`;
}

function buildIssueGroupHtml(g: IssueGroup): string {
  const icon = g.severity === "error" ? "✕" : g.severity === "warning" ? "⚠" : "ℹ";
  const severityClass = `issueGroup--${g.severity}`;
  return `
    <details class="issueGroup ${severityClass}" open>
      <summary class="issueGroup__header">
        <span class="issueGroup__icon">${icon}</span>
        <span class="issueGroup__label">${escapeText(g.label)}</span>
        ${g.filterKind ? `<button class="issueGroup__filterBtn" data-filter-kind="${escapeText(g.filterKind)}" title="Filter table">&#x1F50D;</button>` : ""}
        ${g.revealPath ? `<button class="issueItem__reveal" data-reveal-path="${escapeText(g.revealPath)}" title="Reveal in Finder">&#x1F4C2;</button>` : ""}
        ${g.label === "Suggested Trades" && g.issues.length > 1 ? `<button class="btn btn--small issueGroup__bulkAction" id="linkAllTrades">Link All</button>` : ""}
        <span class="issueGroup__count">${g.issues.length}</span>
      </summary>
      <ul class="issueGroup__list">
        ${g.issues.map((issue) => {
          const acctMatch = issue.group === "Unverified Balance" ? issue.message.match(/^([^ ]+) —/) : null;
          return `
          <li class="issueItem issueItem--${issue.severity} ${issue.scrollToTxnId ? "issueItem--clickable" : ""}" ${issue.scrollToTxnId ? `data-scroll-to-txn="${escapeText(issue.scrollToTxnId)}"` : ""}>
            ${issue.message.startsWith("...and ") ? `<span class="issueItem__more">${escapeText(issue.message)}</span>` : escapeText(issue.message)}
            ${issue.revealPath && !g.revealPath ? `<button class="issueItem__reveal" data-reveal-path="${escapeText(issue.revealPath)}" title="Reveal in Finder">&#x1F4C2;</button>` : ""}
            ${acctMatch ? `<button class="btn btn--small issueItem__action" data-set-opening="${escapeText(acctMatch[1])}">Set Balance</button>` : ""}
            ${issue.tradeSuggestionIdx !== undefined ? `<button class="btn btn--small issueItem__action" data-accept-trade="${issue.tradeSuggestionIdx}">Link</button>` : ""}
          </li>`;
        }).join("")}
      </ul>
    </details>`;
}

function buildRuleEditorScopeHtml(state: AppState): string {
  const draft = state.ruleEditorDraft!;
  const leafAccount = folderToAccountName(draft.accountFolder);
  const leafParts = leafAccount.split(":");
  const localName = leafParts[leafParts.length - 1] || leafAccount;
  const hasInstitution = leafParts.length > 2;
  const institutionName = hasInstitution ? leafParts.slice(1, -1).join(":") : "";
  const scope = draft.ruleScope;
  const d = state.busy ? "disabled" : "";
  return `
    <div class="ruleEditorBar__row ruleEditorBar__scopeRow">
      <span class="ruleEditorBar__label">Scope</span>
      <label class="ruleEditorBar__radio"><input type="radio" name="ruleScope" value="local" ${scope === "local" ? "checked" : ""} ${d} /> ${escapeText(localName)}</label>
      ${hasInstitution ? `<label class="ruleEditorBar__radio"><input type="radio" name="ruleScope" value="institution" ${scope === "institution" ? "checked" : ""} ${d} /> ${escapeText(institutionName)}</label>` : ""}
      <label class="ruleEditorBar__radio"><input type="radio" name="ruleScope" value="global" ${scope === "global" ? "checked" : ""} ${d} /> All accounts</label>
    </div>`;
}

function buildRulePreviewBody(state: AppState, vs: RenderViewState): string {
  const { selectedAccount, allTransactions, isUncategorisedTxn, tradeLinkMap, baseCurrency } = vs;
  const draft = state.ruleEditorDraft!;
  const matches = draft.previewMatches;
  if (matches.length === 0) return `<tr><td colspan="6" class="empty">No matching transactions. Adjust the pattern above.</td></tr>`;

  let resolved = matches.map((m) => ({
    txn: draft.scopedTransactions[m.transactionIndex],
    shadowed: m.shadowed, shadowedByRuleId: m.shadowedByRuleId,
  })).filter((r) => r.txn);
  if (state.txSort) {
    const sorted = sortTransactions(resolved.map((r) => r.txn), state.txSort, selectedAccount);
    const metaMap = new Map(resolved.map((r) => [r.txn, r]));
    resolved = sorted.map((t) => metaMap.get(t)!);
  }
  const PAGE_SIZE = 50;
  const renderedPartnerIds = new Set<string>();
  const rows = resolved.slice(0, PAGE_SIZE).map((r) => {
    const rowHtml = renderTransactionRows([r.txn], state, selectedAccount, allTransactions, isUncategorisedTxn, tradeLinkMap, renderedPartnerIds, baseCurrency, false);
    return r.shadowed ? rowHtml.replace("<tr", `<tr class="rulePreview__shadowed" title="Shadowed by rule: ${escapeText(r.shadowedByRuleId ?? "")}"`) : rowHtml;
  }).join("");
  return matches.length > PAGE_SIZE
    ? rows + `<tr><td colspan="6" class="txTable__pageNote">Showing ${PAGE_SIZE} of ${matches.length} matches</td></tr>`
    : rows;
}

function buildRuleEditorViewHtml(state: AppState, vs: RenderViewState): string {
  const draft = state.ruleEditorDraft!;
  const d = state.busy ? "disabled" : "";
  const pills: SearchPill[] = [...draftConditionsToPills(draft), ...draft.filterPills];
  const searchHtml = renderSmartSearch("ruleSearch", pills, draft.pattern, {
    keywords: [...TRANSACTION_SEARCH_KEYWORDS, "field"],
    valueSuggestions: { field: ["narration", "payee", "meta", "commodity"] },
    placeholder: "*search pattern* field:narration commodity:ETH amount:>100",
  }, !!state.busy);
  const shadowedCount = draft.previewMatches.filter((m) => m.shadowed).length;
  return `
    <header class="topbar ruleEditorBar">
      <div class="ruleEditorBar__row">${searchHtml}</div>
      <div class="ruleEditorBar__divider"><span>Apply</span></div>
      <div class="ruleEditorBar__row">
        <label class="ruleEditorBar__field"><span class="ruleEditorBar__label">Comment</span><input id="ruleComment" class="ruleEditorBar__input" value="${escapeText(draft.comment)}" ${d} placeholder="(optional)" /></label>
      </div>
      <div class="ruleEditorBar__row">
        <label class="ruleEditorBar__field"><span class="ruleEditorBar__label">Amount Account</span><input id="ruleAmountAccount" class="ruleEditorBar__input ruleEditorBar__input--mono" value="${escapeText(draft.amountAccount)}" ${d} placeholder="expenses:unknown" /></label>
        <label class="ruleEditorBar__field"><span class="ruleEditorBar__label">Fee Account</span><input id="ruleFeeAccount" class="ruleEditorBar__input ruleEditorBar__input--mono" value="${escapeText(draft.feeAccount)}" ${d} placeholder="(leave empty if no fee)" /></label>
      </div>
      ${buildRuleEditorScopeHtml(state)}
      <div class="ruleEditorBar__actions">
        <span class="ruleEditorBar__matchCount">${draft.previewMatches.length} match${draft.previewMatches.length === 1 ? "" : "es"}${shadowedCount > 0 ? `, ${shadowedCount} shadowed` : ""}</span>
        ${draft.error ? `<span class="ruleEditorBar__error">${escapeText(draft.error)}</span>` : ""}
        ${draft.ruleId ? `<button id="ruleDelete" class="btn btn--danger btn--small" ${d}>Delete</button>` : ""}
        <button id="ruleCancel" class="btn btn--secondary btn--small" ${d}>Cancel</button>
        <button id="ruleSave" class="btn btn--small" ${d}>Apply</button>
      </div>
    </header>
    <section class="content">
      <div class="tableCard">
        <table class="txTable">
          <thead><tr>
            ${renderSortHeader("Date", "date", state.txSort, "col-date")}
            ${renderSortHeader("Party", "party", state.txSort, "col-party")}
            ${renderSortHeader("Notes", "notes", state.txSort, "col-notes")}
            ${renderSortHeader("Category", "category", state.txSort, "col-category")}
            ${renderSortHeader("Amount", "amount", state.txSort, "num col-amount")}
            <th class="col-actions"></th>
          </tr></thead>
          <tbody>${buildRulePreviewBody(state, vs)}</tbody>
        </table>
      </div>
    </section>`;
}

function buildActionBarHtml(state: AppState, selectedAccount: string | undefined): string {
  const d = state.busy ? "disabled" : "";
  const deleteBtn = selectedAccount && state.accountFoldersMap[selectedAccount] ? `
    <button id="deleteAccountBtn" class="actionBtn actionBtn--ghost actionBtn--danger" ${d}>
      <span class="actionBtn__icon" aria-hidden="true"><svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M8.75 1A2.75 2.75 0 006 3.75v.443c-.795.077-1.584.176-2.365.298a.75.75 0 10.23 1.482l.149-.022.841 10.518A2.75 2.75 0 007.596 19h4.807a2.75 2.75 0 002.742-2.53l.841-10.519.149.023a.75.75 0 00.23-1.482A41.03 41.03 0 0014 4.193V3.75A2.75 2.75 0 0011.25 1h-2.5zM10 4c.84 0 1.673.025 2.5.075V3.75c0-.69-.56-1.25-1.25-1.25h-2.5c-.69 0-1.25.56-1.25 1.25v.325C8.327 4.025 9.16 4 10 4zM8.58 7.72a.75.75 0 00-1.5.06l.3 7.5a.75.75 0 101.5-.06l-.3-7.5zm4.34.06a.75.75 0 10-1.5-.06l-.3 7.5a.75.75 0 101.5.06l.3-7.5z" clip-rule="evenodd"/></svg></span>
      Delete
    </button>` : "";
  return `<section class="actionBar">
    <button id="importFile" class="actionBtn" ${d}><span class="actionBtn__icon" aria-hidden="true"><svg viewBox="0 0 20 20" fill="currentColor"><path d="M10 2a1 1 0 0 1 1 1v7.59l2.3-2.3a1 1 0 1 1 1.4 1.42l-4 4a1 1 0 0 1-1.4 0l-4-4a1 1 0 0 1 1.4-1.42l2.3 2.3V3a1 1 0 0 1 1-1Z" /><path d="M3 14a1 1 0 0 1 1 1v1h12v-1a1 1 0 1 1 2 0v2a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1v-2a1 1 0 0 1 1-1Z" /></svg></span> Import</button>
    <button id="addNew" class="actionBtn" ${d}><span class="actionBtn__icon" aria-hidden="true"><svg viewBox="0 0 20 20" fill="currentColor"><path d="M10 2a1 1 0 0 1 1 1v6h6a1 1 0 1 1 0 2h-6v6a1 1 0 1 1-2 0v-6H3a1 1 0 1 1 0-2h6V3a1 1 0 0 1 1-1Z" /></svg></span> Add New</button>
    <button id="importRules" class="actionBtn" ${d}><span class="actionBtn__icon" aria-hidden="true"><svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M4.5 2A1.5 1.5 0 003 3.5v13A1.5 1.5 0 004.5 18h11a1.5 1.5 0 001.5-1.5V7.621a1.5 1.5 0 00-.44-1.06l-4.12-4.122A1.5 1.5 0 0011.378 2H4.5zM10 8a.75.75 0 01.75.75v1.5h1.5a.75.75 0 010 1.5h-1.5v1.5a.75.75 0 01-1.5 0v-1.5h-1.5a.75.75 0 010-1.5h1.5v-1.5A.75.75 0 0110 8z" clip-rule="evenodd"/></svg></span> Edit Import Rules</button>
    <button id="importPrices" class="actionBtn" ${d}><span class="actionBtn__icon" aria-hidden="true"><svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M12 7a1 1 0 110-2h5a1 1 0 011 1v5a1 1 0 11-2 0V8.414l-4.293 4.293a1 1 0 01-1.414 0L8 10.414l-4.293 4.293a1 1 0 01-1.414-1.414l5-5a1 1 0 011.414 0L11 10.586 14.586 7H12z" clip-rule="evenodd"/></svg></span> Import Prices</button>
    <select id="dateFilter" class="actionBtn actionBtn--ghost" ${d}>
      ${(["week", "month", "year", "all"] as DateFilter[]).map((f) => `<option value="${f}" ${f === state.dateFilter ? "selected" : ""}>${DATE_FILTER_LABELS[f]}</option>`).join("")}
    </select>
    <button id="showHiddenBtn" class="actionBtn actionBtn--ghost${state.showHidden ? " actionBtn--active" : ""}" ${d}>Show Ignored</button>
    ${deleteBtn}
    <button id="rebuildBtn" class="actionBtn actionBtn--ghost" ${d}><span class="actionBtn__icon" aria-hidden="true"><svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M15.312 4.688a7 7 0 0 0-9.9 0l-.706.707H7a1 1 0 0 1 0 2H3a1 1 0 0 1-1-1V2.395a1 1 0 1 1 2 0v2.293l.706-.707a9 9 0 0 1 12.728 0 1 1 0 0 1-1.414 1.414l-.708-.707ZM4.688 15.312a7 7 0 0 0 9.9 0l.706-.707H13a1 1 0 1 1 0-2h4a1 1 0 0 1 1 1v4a1 1 0 1 1-2 0v-2.293l-.706.707a9 9 0 0 1-12.728 0 1 1 0 0 1 1.414-1.414l.708.707Z" clip-rule="evenodd"/></svg></span> Rebuild</button>
    <button id="displayConfigBtn" class="actionBtn actionBtn--ghost" ${d} title="Edit display config"><span class="actionBtn__icon" aria-hidden="true"><svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M11.49 3.17c-.38-1.56-2.6-1.56-2.98 0a1.532 1.532 0 01-2.286.948c-1.372-.836-2.942.734-2.106 2.106.54.886.061 2.042-.947 2.287-1.561.379-1.561 2.6 0 2.978a1.532 1.532 0 01.947 2.287c-.836 1.372.734 2.942 2.106 2.106a1.532 1.532 0 012.287.947c.379 1.561 2.6 1.561 2.978 0a1.533 1.533 0 012.287-.947c1.372.836 2.942-.734 2.106-2.106a1.533 1.533 0 01.947-2.287c1.561-.379 1.561-2.6 0-2.978a1.532 1.532 0 01-.947-2.287c.836-1.372-.734-2.942-2.106-2.106a1.532 1.532 0 01-2.287-.947zM10 13a3 3 0 100-6 3 3 0 000 6z" clip-rule="evenodd"/></svg></span></button>
  </section>`;
}

function buildMainContentHtml(state: AppState, vs: RenderViewState): string {
  const { selectedAccount, showValueColumn } = vs;
  if (state.sidebarView === "reports" && state.selectedReport) {
    const body = state.reportLoading
      ? `<div class="reportView"><div class="reportView__empty">Generating report…</div></div>`
      : state.selectedReport === "performance"
      ? renderPerformanceReport(state)
      : state.selectedReport === "loss_harvest"
        ? renderLossHarvestReport(state)
        : state.cgtReport
          ? renderCgtReport(state)
          : state.incomeReport
            ? renderIncomeReport(state)
            : state.balancesReport
              ? renderBalancesReport(state)
              : renderMarkdownReport(state);
    return `<section class="content reportContent">${renderReportScope(state)}${body}</section>`;
  }
  if (state.sidebarView === "reports") {
    return `<section class="content"><div class="reportView__empty" style="padding-top:48px">Select a report from the sidebar to get started.</div></section>`;
  }
  if (state.sidebarView === "plugins") {
    return buildPluginsContentHtml(state);
  }
  // Accounts view — transaction table
  return `
    <section class="content">
      <div class="tableCard">
        <table class="txTable">
          <thead><tr>
            ${renderSortHeader("Date", "date", state.txSort, "col-date")}
            ${renderSortHeader("Party", "party", state.txSort, "col-party")}
            ${renderSortHeader("Notes", "notes", state.txSort, "col-notes")}
            ${renderSortHeader("Category", "category", state.txSort, "col-category")}
            ${renderSortHeader("Amount", "amount", state.txSort, "num col-amount")}
            ${showValueColumn ? `<th class="num col-value">Value</th>` : ""}
            <th class="col-actions"></th>
          </tr></thead>
          <tbody>${buildTxTableBody(state, vs)}</tbody>
        </table>
      </div>
    </section>`;
}

function buildPluginsContentHtml(state: AppState): string {
  const isDailyRun = state.pluginRunning === DAILY_SYNC_SENTINEL;
  const runningLabel = isDailyRun ? "daily price sync" : escapeText(state.pluginRunning ?? "");
  const startupToggle = `
        <label class="pluginStartupToggle" style="display:flex;align-items:center;gap:8px;margin:0 0 12px;cursor:pointer">
          <input type="checkbox" data-testid="update-prices-on-startup" ${state.updatePricesOnStartup ? "checked" : ""} ${state.pluginRunning ? "disabled" : ""}>
          <span>Update prices on startup <span style="color:var(--text-secondary)">— backfill missing prices when the app launches</span></span>
        </label>
        <button class="btn btn--secondary" data-testid="update-prices-now" ${state.pluginRunning ? "disabled" : ""} style="margin:0 0 20px">${isDailyRun ? "Updating prices…" : "Update prices now"}</button>`;
  const s = state.dailySummary;
  // Classify each outcome. Exit 2 = "partial success" (data written, with
  // warnings) — distinct from an outright failure, so it isn't shown as ✗.
  const kindOf = (o: DailyPluginOutcome): "skipped" | "ok" | "partial" | "failed" =>
    o.skipped_ran_today ? "skipped" : o.success ? "ok" : o.exit_code === 2 ? "partial" : "failed";
  const icon: Record<string, string> = { skipped: "⏭", ok: "✓", partial: "⚠", failed: "✗" };
  const nKind = (k: string) => s ? s.outcomes.filter(o => kindOf(o) === k).length : 0;
  const dailySummaryHtml = (!state.pluginRunning && s) ? `
        <div class="pluginResult">
          <div class="pluginResult__header">Daily price sync ${s.outcomes.length === 0
            ? "— no daily plugins configured"
            : `— ${nKind("ok") + nKind("partial")} updated${nKind("partial") ? ` (${nKind("partial")} with warnings)` : ""}, ${nKind("skipped")} up to date, ${nKind("failed")} failed`}</div>
          ${s.outcomes.map(o => { const k = kindOf(o); return `<div class="pluginResult__row" style="padding:2px 0">${icon[k]} ${escapeText(o.name)}${k === "skipped" ? " (already up to date)" : `${k === "partial" ? " — updated, with warnings" : ""} (${(o.duration_ms / 1000).toFixed(1)}s)`}</div>`; }).join("")}
        </div>` : "";
  return `
    <section class="content">
      <div style="padding:24px;max-width:640px">
        <h2 style="margin:0 0 16px">Plugins</h2>
        <p style="color:var(--text-secondary);margin:0 0 16px">Plugins sync external data (prices, exchange trades) into your sources folder. The pipeline rebuilds automatically when plugins write new files.</p>
        ${startupToggle}
        ${state.pluginRunning ? `
        <div class="pluginResult">
          <div class="pluginResult__header">Running ${runningLabel}…</div>
          <pre class="pluginResult__output" id="pluginLiveLog">${escapeText(state.pluginLog ?? "(waiting for output…)")}</pre>
        </div>` : ""}
        ${dailySummaryHtml}
        ${!state.pluginRunning && state.pluginResult ? `
        <div class="pluginResult ${state.pluginResult.success ? "pluginResult--success" : "pluginResult--error"}">
          <div class="pluginResult__header">${state.pluginResult.success ? "Plugin completed successfully" : "Plugin failed"} (${(state.pluginResult.duration_ms / 1000).toFixed(1)}s)</div>
          ${state.pluginResult.stdout ? `<pre class="pluginResult__output">${escapeText(state.pluginResult.stdout)}</pre>` : ""}
          ${state.pluginResult.stderr ? `<pre class="pluginResult__output pluginResult__output--error">${escapeText(state.pluginResult.stderr)}</pre>` : ""}
        </div>` : ""}
      </div>
    </section>`;
}

function buildMainHtml(state: AppState, vs: RenderViewState, statusText: string, diagnostics: Diagnostic[]): string {
  const { balances, tree, selectedAccount, selectedBalance, selectedTotal,
    allTransactions, filteredTransactions, searchFiltered, tradeLinkMap,
    uncategorisedCount, showValueColumn, issueGroups, totalIssueCount, issueCounts,
    isUncategorisedTxn, baseCurrency, isReportsView, isPluginsView } = vs;

  return `
    <div class="layout" data-testid="app-ready">
      ${renderSidebar(state, tree, selectedAccount, balances, issueCounts)}

      <main class="main">
        ${renderRebuildStrip(state.pendingMutations + (state.busy ? 1 : 0))}
        ${isTransactionPane(state) ? buildAccountsTopbar(state, vs, statusText) : ""}

        ${isTransactionPane(state) ? buildActionBarHtml(state, selectedAccount) : ""}

        ${
          import.meta.env.VITE_E2E === "1"
            ? `
              <section class="e2ePanel">
                <div class="e2ePanel__label">E2E</div>
                <input id="e2e-parse-path" class="e2ePanel__input" placeholder="/abs/path/to/file.transactions" />
                <button id="e2e-parse-run" class="btn btn--secondary" ${state.busy ? "disabled" : ""}>Parse path</button>
              </section>
            `
            : ""
        }

        ${state.sidebarView === "rule-editor" && state.ruleEditorDraft
          ? buildRuleEditorViewHtml(state, vs)
          : buildMainContentHtml(state, vs)}

        ${buildIssuesPanelHtml(state, vs)}
      </main>
    </div>
    ${renderTaxSettingsModal(state)}
    ${renderGlobalSettingsModal(state)}
    ${renderModals(state)}
  `;
}

function attachRenderHandlers(state: AppState, vs: RenderViewState): void {
  const { selectedAccount, allTransactions, filteredTransactions, searchFiltered,
    tradeLinkMap, uncategorisedCount, showValueColumn, isUncategorisedTxn,
    tree, balances, issueGroups, totalIssueCount, issueCounts,
    baseCurrency, isReportsView, isPluginsView } = vs;

  // ── Group 1: Reports & navigation ──
  attachReportNavHandlers(state, vs);
  // ── Group 2: Account sidebar, search, data ──
  attachAccountDataHandlers(state, vs);
  // ── Group 3: AI suggest & account management ──
  attachAiAccountHandlers(state, vs);
  // ── Group 4: Price, sync, relay, transform ──
  attachModalSyncHandlers(state);
  // ── Group 5–7: Rule editor, txn table, table controls (share closures) ──
  attachRuleAndTableHandlers(state, vs);
}

function attachReportNavHandlers(state: AppState, vs: RenderViewState): void {
  const accountSetSelect = document.querySelector<HTMLSelectElement>("#accountSetSelect");
  accountSetSelect?.addEventListener("change", async (e) => {
    state.selectedAccountSet = (e.target as HTMLSelectElement).value;
    localStorage.setItem("arimalo_selectedAccountSet", state.selectedAccountSet);
    state.selectedAccount = undefined;
    await loadDisplayConfig(state);
    await loadGeneratedLedger(state);
  });

  // Sidebar view switcher
  document.querySelectorAll<HTMLButtonElement>(".sidebarNav__item").forEach((btn) => {
    btn.addEventListener("click", () => {
      const view = btn.dataset.view as "accounts" | "categories" | "reports" | "plugins";
      if (view === state.sidebarView) return;
      state.ruleEditorDraft = undefined; // clear rule editor when switching views
      state.sidebarView = view;
      if (view === "accounts") {
        state.selectedReport = undefined;
        state.cgtReport = undefined;
        state.incomeReport = undefined;
        // Drop the categories transaction window so account rows reload cleanly.
        state.searchFilteredTransactions = undefined;
        state.txWindowStart = 0;
        pushNav(state);
        updateNavFlags(state);
        render(state);
        void Promise.all([loadPrefixQuery(state), runSearchFilter(state)]).then(() => render(state));
        return;
      }
      if (view === "categories") {
        state.selectedReport = undefined;
        state.cgtReport = undefined;
        state.incomeReport = undefined;
        // Reset the shared transaction window so stale account rows don't flash
        // before the category query returns (or stay if no category is selected).
        state.searchFilteredTransactions = undefined;
        state.searchFilteredCount = undefined;
        state.searchFilteredOffset = undefined;
        state.txWindowStart = 0;
        pushNav(state);
        updateNavFlags(state);
        render(state);
        void Promise.all([runSearchFilter(state)]).then(() => {
          render(state);
          return Promise.all([loadTransactionValues(state), loadAccountTotalValue(state)]);
        }).then(() => render(state));
        return;
      }
      if (view === "plugins") {
        pushNav(state);
        updateNavFlags(state);
        render(state);
        void loadPlugins(state).then(() => render(state));
        return;
      }

      // Entering reports from accounts should always show a concrete report view.
      state.selectedReport = state.selectedReport ?? "cgt";
      maybePrefillBalancesScope(state);
      pushNav(state);
      updateNavFlags(state);
      render(state);
      void loadReport(state).then(() => render(state));
    });
  });

  // Plugin run buttons
  document.querySelectorAll<HTMLButtonElement>("[data-run-plugin]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const pluginName = btn.dataset.runPlugin!;
      state.pluginRunning = pluginName;
      state.pluginResult = undefined;
      state.pluginLog = "";
      render(state);
      try {
        const result = await invoke<PluginRunResult>("run_plugin_cmd", { pluginName });
        state.pluginResult = result;
      } catch (e) {
        state.pluginResult = { success: false, stdout: "", stderr: String(e), duration_ms: 0 };
      }
      state.pluginRunning = undefined;
      await loadPlugins(state);
      render(state);
    });
  });

  // Plugin "Configure" — load current config + secrets and expand the editor.
  document.querySelectorAll<HTMLButtonElement>("[data-config-plugin]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const name = btn.dataset.configPlugin!;
      if (state.pluginConfigOpen === name) {
        state.pluginConfigOpen = undefined;
        render(state);
        return;
      }
      state.pluginConfigOpen = name;
      state.pluginConfigSaved = undefined;
      state.pluginConfigValues = {};
      state.pluginSecretValues = {};
      try {
        state.pluginConfigValues = (await invoke<Record<string, unknown>>("get_plugin_config", { pluginName: name })) ?? {};
        state.pluginSecretValues = (await invoke<Record<string, unknown>>("get_plugin_secrets", { pluginName: name })) ?? {};
      } catch (e) {
        console.error("Failed to load plugin config/secrets:", e);
      }
      render(state);
    });
  });

  // Plugin config: Close.
  document.querySelectorAll<HTMLButtonElement>("[data-cancel-plugin-config]").forEach((btn) => {
    btn.addEventListener("click", () => {
      state.pluginConfigOpen = undefined;
      render(state);
    });
  });

  // Plugin config: Save (writes config.json + secrets.json).
  document.querySelectorAll<HTMLButtonElement>("[data-save-plugin-config]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const name = btn.dataset.savePluginConfig!;
      const panel = btn.closest(".pluginItem");
      if (!panel) return;
      const config: Record<string, unknown> = { ...(state.pluginConfigValues ?? {}) };
      panel.querySelectorAll<HTMLInputElement>("[data-cfg-field]").forEach((inp) => {
        const key = inp.dataset.cfgField!;
        const t = inp.dataset.cfgType;
        if (t === "boolean") config[key] = inp.checked;
        else if (t === "integer" || t === "number") config[key] = inp.value === "" ? null : Number(inp.value);
        else config[key] = inp.value;
      });
      const secrets: Record<string, unknown> = { ...(state.pluginSecretValues ?? {}) };
      panel.querySelectorAll<HTMLInputElement>("[data-secret-field]").forEach((inp) => {
        secrets[inp.dataset.secretField!] = inp.value;
      });
      state.pluginConfigValues = config;
      state.pluginSecretValues = secrets;
      state.pluginConfigSaving = true;
      state.pluginConfigSaved = undefined;
      render(state);
      try {
        await invoke("save_plugin_config_cmd", { pluginName: name, config });
        if (Object.keys(secrets).length > 0) {
          await invoke("save_plugin_secrets_cmd", { pluginName: name, secrets });
        }
        state.pluginConfigSaved = name;
      } catch (e) {
        console.error("Failed to save plugin config:", e);
        alert("Failed to save plugin config: " + String(e));
      }
      state.pluginConfigSaving = false;
      render(state);
    });
  });

  // "Update prices on startup" toggle (persisted setting).
  const startupToggle = document.querySelector<HTMLInputElement>('[data-testid="update-prices-on-startup"]');
  if (startupToggle) {
    startupToggle.addEventListener("change", async () => {
      const enabled = startupToggle.checked;
      state.updatePricesOnStartup = enabled;
      try {
        await invoke("set_update_prices_on_startup", { enabled });
      } catch (e) {
        console.error("Failed to save update-prices-on-startup:", e);
      }
    });
  }

  // "Update prices now" — manually trigger the daily backfill (forced).
  const updateNowBtn = document.querySelector<HTMLButtonElement>('[data-testid="update-prices-now"]');
  if (updateNowBtn) {
    updateNowBtn.addEventListener("click", () => {
      void runDailySync(false);
    });
  }

  // Report menu items
  document.querySelectorAll<HTMLButtonElement>("[data-report]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const reportType = btn.dataset.report as "cgt" | "income" | "balances" | "performance" | "loss_harvest";
      state.selectedReport = reportType;
      state.reportPageSize = 50;
      state.cgtSort = undefined;
      state.cgtFilterText = undefined;
      state.incomeSort = undefined;
      state.balancesSort = undefined;
      state.balancesFilterText = undefined;
      maybePrefillBalancesScope(state);
      pushNav(state);
      updateNavFlags(state);
      await loadReport(state);
      render(state);
    });
  });

  // Tax Savings position/parcel view toggle (both datasets are already loaded,
  // so this is a pure re-render — no refetch).
  document.querySelectorAll<HTMLButtonElement>("[data-loss-view]").forEach((btn) => {
    btn.addEventListener("click", () => {
      state.lossHarvestView = btn.getAttribute("data-loss-view") as "position" | "parcel";
      render(state);
    });
  });

  // Report date mode toggle
  document.querySelectorAll<HTMLButtonElement>("[data-date-mode]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const mode = btn.getAttribute("data-date-mode") as "fy" | "custom";
      state.reportDateMode = mode;
      // Clear custom report data when switching back to FY
      if (mode === "fy") {
        state.cgtReport = undefined;
        state.incomeReport = undefined;
        state.performanceReport = undefined;
        state.lossHarvestReport = undefined;
        void loadReport(state).then(() => render(state));
      } else {
        render(state);
      }
    });
  });

  // Report year selector (FY mode)
  const reportYearSelect = document.querySelector<HTMLSelectElement>("#reportYearSelect");
  reportYearSelect?.addEventListener("change", async (e) => {
    state.selectedReportYear = parseInt((e.target as HTMLSelectElement).value);
    state.reportPageSize = 50;
    state.cgtSort = undefined;
    state.cgtFilterText = undefined;
    state.incomeSort = undefined;
    await loadReport(state);
    render(state);
  });

  // Custom date range apply
  document.querySelector("#reportDateApply")?.addEventListener("click", async () => {
    const from = (document.querySelector("#reportDateFrom") as HTMLInputElement)?.value;
    const to = (document.querySelector("#reportDateTo") as HTMLInputElement)?.value;
    if (!from || !to) return;
    state.reportDateFrom = from;
    state.reportDateTo = to;
    await loadCustomReport(state);
    render(state);
  });

  // Base account scope
  document.querySelector("#reportBaseScope")?.addEventListener("change", async (e) => {
    state.reportBaseScope = (e.target as HTMLInputElement).value || undefined;
    if (state.reportDateMode === "custom") {
      await loadCustomReport(state);
    } else {
      await loadReport(state);
    }
    render(state);
  });

  // Export CSV (top-right of any report page)
  document.querySelectorAll<HTMLButtonElement>("[data-export-csv]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const reportType = btn.getAttribute("data-export-csv") as "cgt" | "income" | "balances" | null;
      if (!reportType) return;
      // CSV export only supports the cached FY snapshots, not custom ranges.
      if (state.reportDateMode === "custom") {
        await confirm("CSV export is only available in FY mode.", { title: "Export CSV", kind: "warning" });
        return;
      }
      const fy =
        (reportType === "cgt" && state.cgtReport?.financial_year) ||
        (reportType === "income" && state.incomeReport?.financial_year) ||
        (state.selectedReportYear !== undefined ? String(state.selectedReportYear) : undefined);
      if (!fy) return;
      const accountSet = state.selectedAccountSet ?? "";
      const defaultName = `${reportType}-${fy}.csv`;
      let destPath: string | null;
      try {
        destPath = await save({
          defaultPath: defaultName,
          filters: [{ name: "CSV", extensions: ["csv"] }],
        });
      } catch {
        return;
      }
      if (!destPath) return;
      try {
        await invoke("export_report_csv_cmd", {
          accountSet,
          reportType,
          financialYear: String(fy),
          destPath,
          baseCurrency: state.displayConfig?.base_currency ?? "AUD",
          baseAccountScope: (state.reportBaseScope ?? "").trim() || null,
        });
      } catch (e) {
        await confirm(`Export failed: ${e}`, { title: "Export CSV", kind: "error" });
      }
    });
  });

  // Tax settings button
  document.querySelector("#taxSettingsBtn")?.addEventListener("click", async () => {
    try {
      const cfg = await invoke<TaxConfig>("get_tax_config", { accountSet: state.selectedAccountSet ?? "" });
      state.taxConfig = cfg;
    } catch { /* use defaults */ }
    try {
      state.availableReportAccounts = await invoke<{ income: string[]; expenses: string[] }>("list_report_accounts_cmd", { accountSet: state.selectedAccountSet ?? "" });
    } catch { /* ignore */ }
    state.taxSettingsOpen = true;
    render(state);
  });

  // Tax settings save/cancel
  document.querySelector("#taxSettingsSave")?.addEventListener("click", async () => {
    const month = parseInt((document.querySelector("#taxFyMonth") as HTMLInputElement)?.value ?? "6");
    const day = parseInt((document.querySelector("#taxFyDay") as HTMLInputElement)?.value ?? "30");
    const percent = parseInt((document.querySelector("#taxCgtPercent") as HTMLInputElement)?.value ?? "50");
    const months = parseInt((document.querySelector("#taxCgtMonths") as HTMLInputElement)?.value ?? "12");
    const marginalRate = parseInt((document.querySelector("#taxMarginalRate") as HTMLInputElement)?.value ?? "47");
    // Collect excluded accounts from unchecked checkboxes
    const excludedIncome: string[] = [];
    const excludedExpenses: string[] = [];
    document.querySelectorAll<HTMLInputElement>('.accountToggle__input[data-account-group="income"]').forEach((cb) => {
      if (!cb.checked) excludedIncome.push(cb.value);
    });
    document.querySelectorAll<HTMLInputElement>('.accountToggle__input[data-account-group="expenses"]').forEach((cb) => {
      if (!cb.checked) excludedExpenses.push(cb.value);
    });
    const cfg: TaxConfig = {
      financial_year_end_month: month,
      financial_year_end_day: day,
      cgt_discount_percent: percent,
      cgt_discount_holding_months: months,
      non_taxable_accounts: excludedIncome,
      non_deductible_accounts: excludedExpenses,
      marginal_tax_rate_percent: marginalRate,
    };
    try {
      await invoke("save_tax_config", { accountSet: state.selectedAccountSet ?? "", config: cfg });
      state.taxConfig = cfg;
    } catch (err) {
      console.error("Failed to save tax config:", err);
    }
    state.taxSettingsOpen = false;
    await loadReport(state);
    render(state);
  });

  document.querySelector("#taxSettingsCancel")?.addEventListener("click", () => {
    state.taxSettingsOpen = false;
    render(state);
  });

  // Global Settings: open from the sidebar gear (seed a working draft so Cancel
  // discards and Save persists).
  document.querySelector("#globalSettingsBtn")?.addEventListener("click", () => {
    state.globalSettingsDraft = [...(state.extraPrimaryAccountPrefixes ?? [])];
    state.globalSettingsOpen = true;
    render(state);
  });

  // Add the input value to the draft (trimmed, de-duplicated, non-empty).
  const addGlobalPrefix = () => {
    const input = document.querySelector<HTMLInputElement>("#globalPrefixInput");
    const value = input?.value.trim() ?? "";
    if (!value) return;
    const draft = state.globalSettingsDraft ?? [];
    if (!draft.includes(value)) draft.push(value);
    state.globalSettingsDraft = draft;
    render(state);
  };
  document.querySelector("#globalPrefixAdd")?.addEventListener("click", addGlobalPrefix);
  document
    .querySelector<HTMLInputElement>("#globalPrefixInput")
    ?.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        addGlobalPrefix();
      }
    });

  // Remove a prefix from the draft.
  document.querySelectorAll<HTMLButtonElement>("[data-prefix-remove]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const target = btn.getAttribute("data-prefix-remove");
      state.globalSettingsDraft = (state.globalSettingsDraft ?? []).filter((p) => p !== target);
      render(state);
    });
  });

  // Save the draft → persist, refresh the open report (live path picks up the
  // new prefixes immediately), close.
  document.querySelector("#globalSettingsSave")?.addEventListener("click", async () => {
    const prefixes = state.globalSettingsDraft ?? [];
    try {
      await invoke("set_extra_primary_account_prefixes", { prefixes });
      state.extraPrimaryAccountPrefixes = prefixes;
    } catch (err) {
      console.error("Failed to save included-account prefixes:", err);
    }
    state.globalSettingsOpen = false;
    state.globalSettingsDraft = undefined;
    await loadReport(state);
    render(state);
  });

  document.querySelector("#globalSettingsCancel")?.addEventListener("click", () => {
    state.globalSettingsOpen = false;
    state.globalSettingsDraft = undefined;
    render(state);
  });

  // Report row click-through (income report rows)
  document.querySelectorAll<HTMLTableRowElement>(".reportRow--clickable").forEach((row) => {
    row.addEventListener("click", () => {
      const account = row.getAttribute("data-goto-account") ?? "";
      if (!account) return;
      state.sidebarView = "accounts";
      state.selectedAccount = account;
      state.selectedReport = undefined;
      state.prefixQuery = undefined;
      state.search = "";
      state.searchPills = [];
      state.searchText = "";
      pushNav(state);
      updateNavFlags(state);
      render(state);
    });
  });

  // CGT report date links: navigate to buy/sell transaction in accounts view.
  // The report's data-goto-account may not match the transaction's actual account
  // (e.g. trade links across exchanges), so resolve via a meta query first.
  document.querySelectorAll<HTMLAnchorElement>(".reportLink").forEach((link) => {
    link.addEventListener("click", async (e) => {
      e.stopPropagation();
      const account = link.getAttribute("data-goto-account") ?? "";
      const txnId = link.getAttribute("data-goto-txn") ?? "";
      if (!account || !txnId) return;

      let resolvedAccount = account;
      try {
        const result = await invoke<QueryResult>("query_search", {
          accountSet: state.selectedAccountSet ?? "",
          search: `meta:${txnId}`,
          limit: 1,
        });
        if (result.transactions.length > 0) {
          resolvedAccount = result.transactions[0].postings[0]?.account ?? account;
        }
      } catch {
        // Fallback to the provided account
      }

      state.sidebarView = "accounts";
      state.selectedAccount = resolvedAccount;
      state.selectedReport = undefined;
      state.prefixQuery = undefined;
      state.transactionValues = undefined;
      state.accountTotalValue = undefined;
      state.searchFilteredTransactions = undefined;
      state.searchFilteredCount = undefined;
      state.searchFilteredOffset = undefined;
      state.txWindowStart = 0;
      state.txExpandedGroups = new Set();
      state.search = `meta:${txnId}`;
      state.searchPills = [{ key: "meta", value: txnId }];
      state.searchText = "";
      pushNav(state);
      updateNavFlags(state);
      render(state);
      await Promise.all([loadPrefixQuery(state), runSearchFilter(state)]);
      render(state);
      await Promise.all([loadTransactionValues(state), loadAccountTotalValue(state), _loadTreeBaseCurrencyTotals(state)]);
      render(state);
    });
  });
}

type TransformNeed = { path: string; suggestion: SuggestTransformResponse };

/** Probe each selected file: split into files that already have a matching
 *  transform (ready to import) and files that still need one. OFX files are
 *  parsed natively by the pipeline, so they always come back ready. */
async function partitionImportFiles(
  paths: string[],
  accountFolder: string,
  accountName: string,
  currency: string | null,
): Promise<{ ready: string[]; needsTransform: TransformNeed[] }> {
  const ready: string[] = [];
  const needsTransform: TransformNeed[] = [];
  for (const filePath of paths) {
    const suggestion = await invoke<SuggestTransformResponse>("suggest_transform", {
      sourcePath: filePath,
      accountFolder,
      accountName,
      currency,
    });
    if (suggestion.needs_transform) needsTransform.push({ path: filePath, suggestion });
    else ready.push(filePath);
  }
  return { ready, needsTransform };
}

/** Import every ready file in a single pipeline run; returns a status line
 *  (empty when there was nothing ready). */
async function importReadyCsvFiles(
  state: AppState,
  accountFolder: string,
  ready: string[],
): Promise<string> {
  if (ready.length === 0) return "";
  const plural = ready.length > 1 ? "s" : "";
  state.status = `Importing ${ready.length} file${plural}...`;
  render(state);
  const response = await invoke<MutationResponse>("import_csv_files_to_account", {
    nowYyyymm: nowYYYYMM(),
    sourcePaths: ready,
    accountFolder,
    accountSet: state.selectedAccountSet ?? "",
  });
  await applyMutationAndRefresh(state, response);
  return `Imported ${ready.length} file${plural}`;
}

/** Open the transform editor for the first file that needs one. The remaining
 *  needy files are stashed on the draft so that saving the transform imports
 *  them too (see the transformSave handler). */
function openTransformEditorForFirst(
  state: AppState,
  accountFolder: string,
  accountName: string,
  currency: string | null,
  needsTransform: TransformNeed[],
  importedMsg: string,
): void {
  const first = needsTransform[0];
  const rest = needsTransform.slice(1).map((n) => n.path);
  state.transformDraft = {
    csvFilename: first.suggestion.csv_filename,
    sourcePath: first.path,
    accountFolder,
    script: first.suggestion.suggestion ?? "",
    headers: first.suggestion.headers ?? [],
    error: undefined,
    currency: currency ?? undefined,
    accountName,
    pendingPaths: rest,
  };
  const moreMsg = rest.length > 0 ? ` (then ${rest.length} more will import with the same transform)` : "";
  const prefix = importedMsg ? `${importedMsg}; ` : "";
  state.status = `${prefix}${first.suggestion.csv_filename} needs a transform${moreMsg}`;
}

/** Import a set of CSV/OFX paths into one account: batch-import those whose
 *  format already has a transform (or needs none), and open the editor for the
 *  first that doesn't (stashing the rest to import after it's saved). */
async function importCsvPaths(
  state: AppState,
  accountFolder: string,
  accountName: string,
  currency: string | null,
  paths: string[],
): Promise<void> {
  const { ready, needsTransform } = await partitionImportFiles(paths, accountFolder, accountName, currency);
  const importedMsg = await importReadyCsvFiles(state, accountFolder, ready);
  if (needsTransform.length > 0) {
    openTransformEditorForFirst(state, accountFolder, accountName, currency, needsTransform, importedMsg);
  } else {
    state.status = importedMsg || "No files selected";
  }
}

function attachAccountDataHandlers(state: AppState, vs: RenderViewState): void {
  const { selectedAccount, allTransactions, filteredTransactions, searchFiltered,
    tradeLinkMap, uncategorisedCount, showValueColumn, isUncategorisedTxn,
    tree, balances, baseCurrency } = vs;

  // Balance accordion toggle
  document.getElementById("balanceToggle")?.addEventListener("click", () => {
    const panel = document.getElementById("allBalances");
    const arrow = document.querySelector(".accountHeader__arrow");
    if (!panel) return;
    const open = panel.style.display !== "none";
    panel.style.display = open ? "none" : "block";
    if (arrow) arrow.classList.toggle("accountHeader__arrow--open", !open);
  });

  // Click-to-edit account friendly name
  const nameEl = document.querySelector<HTMLDivElement>(".accountHeader__name");
  nameEl?.addEventListener("dblclick", () => {
    if (!state.selectedAccount || state.busy) return;
    const current = state.accountPropertiesMap[state.selectedAccount]?.name ?? "";
    const input = prompt("Friendly name for this account:", current);
    if (input === null) return;
    const friendlyName = input.trim();
    if (friendlyName === current) return;
    state.busy = true;
    state.status = "Updating account name...";
    render(state);
    invoke<PipelineResponse>("update_account_properties", {
      nowYyyymm: nowYYYYMM(),
      accountName: state.selectedAccount,
      friendlyName,
      accountSet: state.selectedAccountSet ?? "",
    }).then(async (response) => {
      await applyPipelineResponse(state, response);
      state.busy = false;
      state.status = friendlyName ? `Renamed to "${friendlyName}"` : "Name cleared";
      render(state);
    }).catch((err) => {
      state.busy = false;
      state.status = `Failed: ${err}`;
      render(state);
    });
  });

  const importButton = document.querySelector<HTMLButtonElement>("#importFile");
  importButton?.addEventListener("click", async () => {
    if (!state.selectedAccount) {
      state.status = "Select an account first";
      render(state);
      return;
    }

    const accountName = state.selectedAccount;
    const accountFolder = resolveAccountFolder(state, accountName);

    const selected = await open({
      multiple: true,
      filters: [{ name: "CSV or OFX", extensions: ["csv", "ofx"] }],
    });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    if (paths.length === 0) return;

    state.busy = true;
    state.status = "Checking transforms...";
    render(state);

    try {
      // Currency comes from the selected account's balance (same for every file).
      const selectedBalance = state.parse?.balances?.find((b) => b.account === accountName);
      const currency = selectedBalance?.totals?.[0]?.commodity ?? null;
      await importCsvPaths(state, accountFolder, accountName, currency, paths);
    } catch (err) {
      state.status = `Error: ${String(err)}`;
    } finally {
      state.busy = false;
      render(state);
    }
  });

  const addNew = document.querySelector<HTMLButtonElement>("#addNew");
  addNew?.addEventListener("click", () => {
    const account = state.selectedAccount ?? "assets";
    const cashCommodity =
      (state.parse?.balances ?? []).find((b) => b.account === account)?.totals[0]?.commodity ?? "USD";
    state.manualDraft = {
      datetime: nowYYYYMMDD(),
      payee: "",
      narration: "",
      account,
      cashCommodity,
      mode: "value",
      amount: "",
      tradeCommodity: "",
      quantity: "",
      price: "",
      contras: [{ account: "", amount: "" }],
      error: undefined,
    };
    render(state);
  });

  const editRulesBtn = document.querySelector<HTMLButtonElement>("#importRules");
  editRulesBtn?.addEventListener("click", async () => {
    if (!state.selectedAccount) {
      state.status = "Select an account first";
      render(state);
      return;
    }

    const accountFolder = resolveAccountFolder(state, state.selectedAccount);

    try {
      const script = await invoke<string | null>("read_transform", { accountFolder });
      if (script == null) {
        state.status = "No transform found for this account";
        render(state);
        return;
      }
      state.transformDraft = {
        csvFilename: "",
        sourcePath: "",
        accountFolder,
        script,
        headers: [],
        error: undefined,
      };
      render(state);
    } catch (err) {
      state.status = `Error: ${String(err)}`;
      render(state);
    }
  });

  const importPricesBtn = document.querySelector<HTMLButtonElement>("#importPrices");
  importPricesBtn?.addEventListener("click", async () => {
    const selected = await open({
      multiple: true,
      filters: [{ name: "Prices", extensions: ["txt", "csv"] }],
    });
    if (!selected) return;
    const filePaths = Array.isArray(selected) ? selected : [selected];
    if (filePaths.length === 0) return;

    const merge = await confirm("Merge with existing prices?\n\nOK = Merge (keep existing, add new)\nCancel = Replace (overwrite)", { title: "Import Prices" });

    state.busy = true;
    state.status = merge ? "Merging prices..." : "Importing prices...";
    render(state);

    try {
      let totalCount = 0;
      const allCommodities: string[] = [];
      for (const filePath of filePaths) {
        const response = await invoke<PriceImportResponse>("import_prices", {
          sourcePath: filePath,
          merge,
        });
        totalCount += response.total_count;
        for (const c of response.commodities) {
          if (!allCommodities.includes(c)) allCommodities.push(c);
        }
      }
      const mode = merge ? "Merged" : "Imported";
      state.status = `${mode} ${totalCount} prices for ${allCommodities.join(", ")}`;
    } catch (err) {
      state.status = `Error: ${String(err)}`;
    } finally {
      state.busy = false;
      render(state);
    }
  });

  const dateFilter = document.querySelector<HTMLSelectElement>("#dateFilter");
  dateFilter?.addEventListener("change", async (e) => {
    state.dateFilter = (e.target as HTMLSelectElement).value as DateFilter;
    resetSearchPaging(state);
    await runSearchFilter(state);
    render(state);
  });

  document.getElementById("showHiddenBtn")?.addEventListener("click", async () => {
    state.showHidden = !state.showHidden;
    await invoke("set_show_hidden", { show: state.showHidden });
    await loadGeneratedLedger(state);
  });

  // Accounts/Categories page smart search bar (both render the same topbar search)
  if (isTransactionPane(state)) {
    let acctSearchTimer: ReturnType<typeof setTimeout> | null = null;
    let prevPillCount = state.searchPills.length;
    cleanupAccountSearch?.();
    cleanupAccountSearch = attachSmartSearch("accountSearch", state.searchPills, {
      keywords: [...TRANSACTION_SEARCH_KEYWORDS],
      placeholder: "Search (e.g. account:eth amount:>100 fee:>0)",
    }, (change) => {
      state.searchPills = change.pills;
      state.searchText = change.text;
      state.search = searchFromPills(change.pills, change.text);
      state.txWindowStart = 0;
      const pillsChanged = change.pills.length !== prevPillCount;
      prevPillCount = change.pills.length;
      const text = change.text.trim();
      // Only re-render when: pills changed, text cleared, or text has 2+ chars
      if (!pillsChanged && text.length === 1) return;
      if (acctSearchTimer) clearTimeout(acctSearchTimer);
      const savedCursor = (document.activeElement as HTMLInputElement)?.selectionStart ?? null;
      acctSearchTimer = setTimeout(async () => {
        await runSearchFilter(state);
        render(state);
        const restored = document.querySelector<HTMLInputElement>("#accountSearchInput");
        if (restored) {
          restored.focus();
          if (savedCursor != null) {
            const pos = Math.min(savedCursor, restored.value.length);
            restored.selectionStart = restored.selectionEnd = pos;
          }
        }
      }, 300);
    });
  }

  document.querySelectorAll<HTMLButtonElement>('[data-testid="account-item"]').forEach((el) => {
    el.addEventListener("click", async () => {
      const account = el.getAttribute("data-account") ?? "";
      selectActiveAccount(state, account);
      pushNav(state);
      updateNavFlags(state);
      render(state);
      await Promise.all([loadPrefixQuery(state), runSearchFilter(state)]);
      render(state);
      await Promise.all([loadTransactionValues(state), loadAccountTotalValue(state), _loadTreeBaseCurrencyTotals(state)]);
      render(state);
    });
  });

  // Drill-down navigation
  document.querySelectorAll<HTMLButtonElement>('[data-testid="drilldown-item"]').forEach((el) => {
    el.addEventListener("click", async () => {
      const name = el.getAttribute("data-drill") ?? "";
      const group = el.getAttribute("data-group") ?? "";
      drillIntoActive(state, name, group);
      state.prefixQuery = undefined;
      state.transactionValues = undefined;
      state.accountTotalValue = undefined;
      state.searchFilteredTransactions = undefined;
      state.searchFilteredCount = undefined;
      state.searchFilteredOffset = undefined;
      state.txWindowStart = 0;
      state.txExpandedGroups = new Set();
      pushNav(state);
      updateNavFlags(state);
      render(state);
      await Promise.all([loadPrefixQuery(state), runSearchFilter(state)]);
      render(state);
      await Promise.all([loadTransactionValues(state), loadAccountTotalValue(state)]);
      render(state);
    });
  });

  document.querySelector<HTMLButtonElement>('[data-testid="sidebar-folder-root"]')?.addEventListener("click", async () => {
    state.drillPath = [];
    state.selectedAccount = ROOT_FOLDER_PATH;
    state.prefixQuery = undefined;
    state.transactionValues = undefined;
    state.accountTotalValue = undefined;
    resetSearchPaging(state);
    pushNav(state);
    updateNavFlags(state);
    render(state);
    await Promise.all([loadPrefixQuery(state), runSearchFilter(state)]);
    render(state);
    await Promise.all([loadTransactionValues(state), loadAccountTotalValue(state)]);
    render(state);
  });

  document.querySelector<HTMLButtonElement>('[data-testid="drilldown-back"]')?.addEventListener("click", async () => {
    drillBackActive(state);
    state.prefixQuery = undefined;
    state.transactionValues = undefined;
    state.accountTotalValue = undefined;
    resetSearchPaging(state);
    pushNav(state);
    updateNavFlags(state);
    render(state);
    await Promise.all([loadPrefixQuery(state), runSearchFilter(state)]);
    render(state);
    await Promise.all([loadTransactionValues(state), loadAccountTotalValue(state)]);
    render(state);
  });

  const rebuildBtn = document.querySelector<HTMLButtonElement>("#rebuildBtn");
  rebuildBtn?.addEventListener("click", async () => {
    await rebuildPipeline(state);
    await loadDisplayConfig(state);
  });

  const displayConfigBtn = document.querySelector<HTMLButtonElement>("#displayConfigBtn");
  displayConfigBtn?.addEventListener("click", async () => {
    try {
      await invoke("open_display_config", { accountSet: state.selectedAccountSet ?? "" });
    } catch (err) {
      state.status = `Error: ${String(err)}`;
      render(state);
    }
  });
}

function attachAiAccountHandlers(state: AppState, vs: RenderViewState): void {
  const { selectedAccount } = vs;

  // ── AI Suggest (per-row sparkle) ──
  // Row buttons only (see ROW_AI_SPARKLE_SELECTOR): the transform modal's
  // "Generate with AI" button shares .ai-sparkle-btn but must not bind here.
  document.querySelectorAll<HTMLButtonElement>(ROW_AI_SPARKLE_SELECTOR).forEach((btn) => {
    btn.addEventListener("click", async () => {
      if (!state.selectedAccount) return;
      const txnId = btn.dataset.aiTxnId ?? "";
      const accountFolder = resolveAccountFolder(state, state.selectedAccount);
      const txnData = JSON.stringify({
        date: btn.dataset.aiDate ?? "",
        datetime: btn.dataset.aiDatetime ?? "",
        payee: btn.dataset.aiPayee ?? "",
        narration: btn.dataset.aiNarration ?? "",
        amount: btn.dataset.aiAmount ?? "",
        commodity: btn.dataset.aiCommodity ?? "",
        txn_id: txnId,
      }, null, 2);

      state.aiSuggest = { status: "analyzing", txnId, steps: [] };
      render(state);

      try {
        await invoke("ai_suggest_categorisation", {
          accountName: state.selectedAccount,
          accountFolder,
          accountSet: state.selectedAccountSet ?? "",
          uncategorisedJson: txnData,
        });
      } catch (err) {
        if (!state.aiSuggest) return;
        state.aiSuggest = { status: "error", txnId, steps: [], error: String(err) };
        render(state);
      }
    });
  });

  // AI Suggest modal: Edit Rule — opens the rule editor with suggestion pre-populated
  document.querySelectorAll<HTMLButtonElement>("[data-ai-apply]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const idx = parseInt(btn.dataset.aiApply!, 10);
      const suggestion = state.aiSuggest?.suggestions?.[idx];
      if (!suggestion || !state.selectedAccount) return;
      const accountFolder = resolveAccountFolder(state, state.selectedAccount);
      try {
        const allRules = await invoke<RuleInfo[]>("get_rules", { accountFolder });
        const allTxns = state.parse?.transactions ?? [];
        const scopeAccount = state.selectedAccount;
        const localTxns = filterByAccountPrefix(allTxns, scopeAccount);
        const pattern = suggestion.pattern;
        const rawMatchField = suggestion.match_field ?? "";
        // When AI says match_field:"commodity", convert to a commodity:VALUE filter pill
        // by looking up the original transaction's commodity.
        let matchField = rawMatchField;
        const filterPills: SearchPill[] = [];
        if (rawMatchField === "commodity" && state.aiSuggest?.txnId) {
          const origTxn = allTxns.find((t) => t.meta?.includes(state.aiSuggest!.txnId));
          if (origTxn) {
            filterPills.push({ key: "commodity", value: origTxn.amount_commodity });
            matchField = ""; // pattern matches narration, commodity is a filter pill
          }
        }
        const preview = computeRulePreviewFromDraft({ pattern, matchField, ruleId: "" }, allRules, localTxns, selectedAccount);
        state.ruleEditorDraft = {
          ruleId: "", accountFolder, accountName: scopeAccount, pattern,
          amountAccount: suggestion.amount_account,
          feeAccount: "", comment: "#ai", matchField,
          amountCondition: "", feeCondition: "", payeeCondition: "",
          narrationCondition: "", commodityCondition: "", metaCondition: "",
          filterPills, ruleScope: "local", allRules, previewMatches: preview,
          scopedTransactions: localTxns, previousView: "accounts",
        };
        state.aiSuggest = undefined;
        state.sidebarView = "rule-editor";
      } catch (err) {
        state.status = `Error opening rule editor: ${String(err)}`;
      }
      render(state);
    });
  });

  // AI Suggest modal: Skip individual
  document.querySelectorAll<HTMLButtonElement>("[data-ai-skip]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const idx = parseInt(btn.dataset.aiSkip!, 10);
      if (!state.aiSuggest?.appliedIndices) state.aiSuggest!.appliedIndices = new Set();
      state.aiSuggest!.appliedIndices.add(idx);
      render(state);
    });
  });

  // AI Suggest modal: Close
  const aiSuggestClose = document.querySelector<HTMLButtonElement>("#aiSuggestClose");
  aiSuggestClose?.addEventListener("click", () => {
    state.aiSuggest = undefined;
    render(state);
  });

  const e2eRun = document.querySelector<HTMLButtonElement>("#e2e-parse-run");
  e2eRun?.addEventListener("click", async () => {
    await rebuildPipeline(state);
  });

  const manualCancel = document.querySelector<HTMLButtonElement>("#manualCancel");
  manualCancel?.addEventListener("click", () => {
    state.manualDraft = undefined;
    render(state);
  });

  const manualSave = document.querySelector<HTMLButtonElement>("#manualSave");
  manualSave?.addEventListener("click", async () => {
    if (!state.manualDraft) return;
    state.manualDraft.error = undefined;
    readManualDomIntoDraft(state.manualDraft);
    try {
      await addManualTransaction(state);
    } catch (err) {
      if (state.manualDraft) {
        state.manualDraft.error = String(err);
        state.busy = false;
        render(state);
      }
    }
  });

  // Manual modal: autocomplete + structured-entry wiring (re-run after each render).
  if (state.manualDraft) {
    const md = state.manualDraft;
    const payeeInput = document.querySelector<HTMLInputElement>("#manualPayee");
    if (payeeInput) {
      attachAccountInput(
        payeeInput,
        { suggestions: collectPayeeSuggestionsFrom(state), allowCreate: true, revertOnBlur: false },
        (value) => { if (state.manualDraft) state.manualDraft.payee = value; },
      );
    }
    // Each "other account" row gets the shared account autocomplete (+ Add new).
    md.contras.forEach((_c, i) => {
      const acctInput = document.querySelector<HTMLInputElement>(`#manualContraAccount-${i}`);
      if (acctInput) {
        attachAccountInput(
          acctInput,
          { suggestions: collectAccountSuggestionsFrom(state), allowCreate: true, revertOnBlur: false },
          (value) => {
            if (state.manualDraft?.contras[i]) {
              state.manualDraft.contras[i].account = value;
              updateManualBalanceUi(state);
            }
          },
        );
      }
    });
    // Value/Trade toggle — capture on-screen values before the swap re-render.
    document.querySelectorAll<HTMLInputElement>('input[name="manualMode"]').forEach((radio) => {
      radio.addEventListener("change", () => {
        if (!state.manualDraft) return;
        readManualDomIntoDraft(state.manualDraft);
        state.manualDraft.mode = radio.value === "trade" ? "trade" : "value";
        state.manualDraft.error = undefined;
        render(state);
      });
    });
    document.querySelector<HTMLButtonElement>("#manualAddContra")?.addEventListener("click", () => {
      if (!state.manualDraft) return;
      readManualDomIntoDraft(state.manualDraft);
      state.manualDraft.contras.push({ account: "", amount: "" });
      render(state);
    });
    document.querySelectorAll<HTMLButtonElement>("[data-contra-remove]").forEach((btn) => {
      btn.addEventListener("click", () => {
        if (!state.manualDraft) return;
        readManualDomIntoDraft(state.manualDraft);
        const idx = Number(btn.getAttribute("data-contra-remove"));
        state.manualDraft.contras.splice(idx, 1);
        if (state.manualDraft.contras.length === 0) state.manualDraft.contras.push({ account: "", amount: "" });
        render(state);
      });
    });
    // Live balance + placeholder update without a re-render (keeps focus).
    ["manualAmount", "manualQuantity", "manualPrice"].forEach((id) => {
      document.querySelector<HTMLInputElement>(`#${id}`)?.addEventListener("input", () => updateManualBalanceUi(state));
    });
    md.contras.forEach((_c, i) => {
      document
        .querySelector<HTMLInputElement>(`#manualContraAccount-${i}`)
        ?.addEventListener("input", () => updateManualBalanceUi(state));
      document
        .querySelector<HTMLInputElement>(`#manualContraAmount-${i}`)
        ?.addEventListener("input", () => updateManualBalanceUi(state));
    });
  }

  // Add Account handlers
  const addAccountBtn = document.querySelector<HTMLButtonElement>("#addAccountBtn");
  addAccountBtn?.addEventListener("click", () => {
    state.addAccountDraft = {
      accountName: "",
      currency: "",
      openingBalance: "",
      error: undefined,
    };
    render(state);
  });

  const addAccountCancel = document.querySelector<HTMLButtonElement>("#addAccountCancel");
  addAccountCancel?.addEventListener("click", () => {
    state.addAccountDraft = undefined;
    render(state);
  });

  const addAccountSubmit = document.querySelector<HTMLButtonElement>("#addAccountSubmit");
  addAccountSubmit?.addEventListener("click", async () => {
    if (!state.addAccountDraft) return;
    state.addAccountDraft.error = undefined;

    const accountName = document.querySelector<HTMLInputElement>("#newAccountName")?.value ?? "";
    const currency = document.querySelector<HTMLInputElement>("#newAccountCurrency")?.value ?? "";
    const openingBalance = document.querySelector<HTMLInputElement>("#newAccountBalance")?.value ?? "";
    state.addAccountDraft.accountName = accountName;
    state.addAccountDraft.currency = currency;
    state.addAccountDraft.openingBalance = openingBalance;

    try {
      await addAccountDeclaration(state);
    } catch (err) {
      if (state.addAccountDraft) {
        state.addAccountDraft.error = String(err);
        state.busy = false;
        render(state);
      }
    }
  });

  // Add Account modal: autocomplete the account-name input to surface
  // existing siblings under "assets:". The user typically types a brand-
  // new name here, so revertOnBlur is off — the modal's own Add button
  // is the commit point. Picking a suggestion canonicalizes the input
  // value (and warns implicitly: the backend will reject duplicates).
  const newAccountInput = document.querySelector<HTMLInputElement>("#newAccountName");
  if (newAccountInput) {
    attachAccountInput(newAccountInput, {
      suggestions: collectAccountSuggestionsFrom(state),
      allowCreate: true,
      prefix: "assets:",
      revertOnBlur: false,
    }, (value) => {
      if (state.addAccountDraft) {
        state.addAccountDraft.accountName = value;
      }
    });
  }

  // Editable account segment handlers — inline editing (prompt() doesn't work in WKWebView)
  // (account header segment editing removed — account structure is managed via folder operations)

  // Delete Account button handler
  const deleteAccountBtn = document.querySelector<HTMLButtonElement>("#deleteAccountBtn");
  deleteAccountBtn?.addEventListener("click", async () => {
    const accountName = state.selectedAccount;
    if (!accountName) return;
    const folder = state.accountFoldersMap[accountName];
    if (!folder) return;
    const confirmed = await confirm(
      `Delete account "${accountName}" and all its data?\n\nThis will permanently remove the folder "${folder}" and all CSV files, transforms, and rules inside it.`,
      { title: "Delete Account", kind: "warning" },
    );
    if (!confirmed) return;
    state.busy = true;
    render(state);
    try {
      const resp = await invoke<PipelineResponse>("delete_account_folder", {
        nowYyyymm: nowYYYYMM(),
        folder,
        accountSet: state.selectedAccountSet ?? "",
      });
      await applyPipelineResponse(state, resp);
      state.selectedAccount = undefined;
      localStorage.removeItem("selectedAccount");
    } catch (err) {
      alert("Delete failed: " + String(err));
    }
    state.busy = false;
    render(state);
  });

  // Opening Balance handlers
  document.querySelectorAll<HTMLButtonElement>("[data-set-opening]").forEach((btn) => {
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const accountName = btn.getAttribute("data-set-opening") ?? "";
      if (!accountName) return;
      // Guess commodity from first balance total
      const bal = state.parse?.balances?.find((b) => b.account === accountName);
      const commodity = bal?.totals?.[0]?.commodity ?? "AUD";
      state.openingBalanceDraft = {
        accountName,
        mode: "direct",
        amount: "",
        commodity,
        date: nowYYYYMMDD(),
        knownBalance: "",
      };
      render(state);
    });
  });

  document.querySelectorAll<HTMLInputElement>('input[name="openingMode"]').forEach((radio) => {
    radio.addEventListener("change", () => {
      if (!state.openingBalanceDraft) return;
      state.openingBalanceDraft.mode = radio.value as "direct" | "from-date";
      state.openingBalanceDraft.error = undefined;
      state.openingBalanceDraft.calculatedOpening = undefined;
      render(state);
    });
  });

  const openingBalanceCancel = document.querySelector<HTMLButtonElement>("#openingBalanceCancel");
  openingBalanceCancel?.addEventListener("click", () => {
    state.openingBalanceDraft = undefined;
    render(state);
  });

  const openingBalanceSubmit = document.querySelector<HTMLButtonElement>("#openingBalanceSubmit");
  openingBalanceSubmit?.addEventListener("click", async () => {
    if (!state.openingBalanceDraft) return;
    const result = readOpeningBalanceForm(state);
    if (result.error) {
      state.openingBalanceDraft.error = result.error;
      render(state);
      return;
    }
    state.busy = true;
    state.status = "Setting opening balance...";
    render(state);
    try {
      const response = await invoke<PipelineResponse>("set_opening_balance", {
        nowYyyymm: nowYYYYMM(),
        accountName: state.openingBalanceDraft.accountName,
        amount: result.amount,
        commodity: result.commodity,
        accountSet: state.selectedAccountSet ?? "",
      });
      await applyPipelineResponse(state, response);
      state.busy = false;
      state.openingBalanceDraft = undefined;
      state.status = "Opening balance saved";
    } catch (err) {
      state.busy = false;
      if (state.openingBalanceDraft) state.openingBalanceDraft.error = String(err);
      state.status = `Failed: ${err}`;
    }
    render(state);
  });
}

function attachModalSyncHandlers(state: AppState): void {
  // Set-price modal handlers
  document.getElementById("setPriceCancel")?.addEventListener("click", () => {
    state.setPriceDraft = undefined;
    render(state);
  });
  document.getElementById("setPriceSave")?.addEventListener("click", async () => {
    if (!state.setPriceDraft) return;
    const datetime = document.getElementById("setPriceDate") as HTMLInputElement | null;
    const amount = document.getElementById("setPriceAmount") as HTMLInputElement | null;
    const quoteCurrency = document.getElementById("setPriceQuoteCurrency") as HTMLInputElement | null;
    const dateVal = datetime?.value?.trim() ?? state.setPriceDraft.datetime;
    const amountVal = amount?.value?.trim() ?? "";
    const quoteVal = quoteCurrency?.value?.trim() || "USD";

    state.setPriceDraft.datetime = dateVal;
    state.setPriceDraft.priceAmount = amountVal;
    state.setPriceDraft.quoteCurrency = quoteVal;

    if (!amountVal) {
      state.setPriceDraft.error = "Price is required.";
      render(state);
      return;
    }
    if (isNaN(Number(amountVal))) {
      state.setPriceDraft.error = "Price must be a number.";
      render(state);
      return;
    }

    state.busy = true;
    state.status = "Setting price...";
    render(state);

    try {
      await invoke("set_price", {
        commodity: state.setPriceDraft.commodity,
        datetime: dateVal,
        priceAmount: amountVal,
        quoteCurrency: quoteVal,
      });
      const savedCommodity = state.setPriceDraft.commodity;
      state.setPriceDraft = undefined;
      state.busy = false;
      state.status = `Price set: ${savedCommodity} = ${amountVal} ${quoteVal}`;
      await loadTransactionValues(state);
      render(state);
    } catch (err) {
      state.busy = false;
      if (state.setPriceDraft) {
        state.setPriceDraft.error = String(err);
      }
      state.status = `Failed: ${err}`;
      render(state);
    }
  });

  // Sync handlers
  const syncViewLogBtn = document.querySelector<HTMLButtonElement>("#syncViewLogBtn");
  syncViewLogBtn?.addEventListener("click", async () => {
    try {
      const syncLog: SyncEvent[] = await invoke("get_sync_log");
      const devices: DeviceInfo[] = await invoke("list_devices");
      state.syncLog = syncLog;
      state.devices = devices;
    } catch {
      // May fail if metadata not initialized, show empty
    }
    state.syncLogOpen = true;
    render(state);
  });

  const syncLogClose = document.querySelector<HTMLButtonElement>("#syncLogClose");
  syncLogClose?.addEventListener("click", () => {
    state.syncLogOpen = false;
    render(state);
  });

  const syncLogExport = document.querySelector<HTMLButtonElement>("#syncLogExport");
  syncLogExport?.addEventListener("click", () => {
    const data = JSON.stringify(
      { syncLog: state.syncLog ?? [], devices: state.devices ?? [] },
      null,
      2,
    );
    const blob = new Blob([data], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `sync-log-${new Date().toISOString().slice(0, 10)}.json`;
    a.click();
    URL.revokeObjectURL(url);
  });

  attachRelaySyncHandlers(state);
}

function attachRelaySyncHandlers(state: AppState): void {
  // Relay handlers
  const relayPairBtn = document.querySelector<HTMLButtonElement>("#relayPairBtn");
  relayPairBtn?.addEventListener("click", () => {
    state.relayPairingOpen = true;
    state.relayPairingMode = undefined;
    state.relayPairingCode = undefined;
    state.relayPairingError = undefined;
    render(state);
  });

  const relaySyncBtn = document.querySelector<HTMLButtonElement>("#relaySyncBtn");
  relaySyncBtn?.addEventListener("click", async () => {
    state.busy = true;
    state.status = "Syncing with relay...";
    render(state);
    try {
      const result = await invoke<RelaySyncResponse>("sync_with_relay_cmd");
      const devices: DeviceInfo[] = await invoke("list_devices");
      const syncLog: SyncEvent[] = await invoke("get_sync_log");
      state.devices = devices;
      state.syncLog = syncLog;
      state.status = `Synced: ${result.blobs_uploaded} up, ${result.blobs_downloaded} down`;
    } catch (err) {
      state.status = `Sync failed: ${String(err)}`;
    }
    state.busy = false;
    render(state);
  });

  // Root folder change handler
  const changeRootBtn = document.querySelector<HTMLButtonElement>("#changeRootBtn");
  changeRootBtn?.addEventListener("click", async () => {
    const selected = await open({ directory: true, multiple: false });
    if (selected) {
      try {
        await invoke("set_root_dir", { path: selected });
        state.rootDir = selected as string;
        try {
          state.knownRoots = await invoke<string[]>("get_known_roots");
        } catch { /* ignore */ }
        // Reload everything with new root
        state.parse = undefined;
        state.selectedAccount = undefined;
        render(state);
        await normalStartup();
      } catch (e) {
        console.error("Failed to change root dir:", e);
      }
    }
  });

  const relayPairCancel = document.querySelector<HTMLButtonElement>("#relayPairCancel");
  relayPairCancel?.addEventListener("click", () => {
    state.relayPairingOpen = false;
    state.pendingRelayConfig = undefined;
    render(state);
  });

  const relayCreateCode = document.querySelector<HTMLButtonElement>("#relayCreateCode");
  relayCreateCode?.addEventListener("click", async () => {
    const relayUrl = document.querySelector<HTMLInputElement>("#relayUrlInput")?.value?.trim() ?? "";
    if (!relayUrl) {
      state.relayPairingError = "Enter a relay URL";
      render(state);
      return;
    }
    state.busy = true;
    state.status = "Creating pairing code...";
    render(state);
    try {
      const result = await invoke<PairInitiateResult>("pair_initiate", { relayUrl });
      state.relayPairingMode = "create";
      state.relayPairingCode = result.pairing_code;
      state.pendingRelayConfig = { relay_url: relayUrl, group_id: result.group_id };
      state.status = undefined;
    } catch (err) {
      state.relayPairingError = String(err);
      state.status = `Pairing failed: ${String(err)}`;
    }
    state.busy = false;
    render(state);
  });

  const relayEnterCode = document.querySelector<HTMLButtonElement>("#relayEnterCode");
  relayEnterCode?.addEventListener("click", () => {
    const relayUrl = document.querySelector<HTMLInputElement>("#relayUrlInput")?.value?.trim() ?? "";
    if (!relayUrl) {
      state.relayPairingError = "Enter a relay URL";
      render(state);
      return;
    }
    state.relayPairingMode = "join";
    // Store URL temporarily for the join step
    (state as Record<string, unknown>)._relayUrl = relayUrl;
    render(state);
  });

  const relayPairCancel2 = document.querySelector<HTMLButtonElement>("#relayPairCancel2");
  relayPairCancel2?.addEventListener("click", () => {
    state.relayPairingOpen = false;
    state.relayPairingMode = undefined;
    state.pendingRelayConfig = undefined;
    render(state);
  });

  const relayJoinSubmit = document.querySelector<HTMLButtonElement>("#relayJoinSubmit");
  relayJoinSubmit?.addEventListener("click", async () => {
    const code = document.querySelector<HTMLInputElement>("#relayJoinCodeInput")?.value?.trim() ?? "";
    if (!code || code.length !== 6) {
      state.relayPairingError = "Enter a 6-digit code";
      render(state);
      return;
    }
    const relayUrl = (state as Record<string, unknown>)._relayUrl as string;
    state.busy = true;
    state.status = "Joining...";
    render(state);
    try {
      const groupId = await invoke<string>("pair_join", { relayUrl, pairingCode: code });
      await invoke("save_relay_config", { relayUrl, groupId });
      state.relayConfig = { relay_url: relayUrl, group_id: groupId };
      state.relayPairingOpen = false;
      state.relayPairingMode = undefined;
      state.status = "Paired successfully";
    } catch (err) {
      state.relayPairingError = String(err);
      state.status = `Join failed: ${String(err)}`;
    }
    state.busy = false;
    render(state);
  });

  const relayPairDone = document.querySelector<HTMLButtonElement>("#relayPairDone");
  relayPairDone?.addEventListener("click", async () => {
    if (state.pendingRelayConfig) {
      try {
        await invoke("save_relay_config", {
          relayUrl: state.pendingRelayConfig.relay_url,
          groupId: state.pendingRelayConfig.group_id,
        });
        state.relayConfig = state.pendingRelayConfig;
      } catch (err) {
        state.status = `Failed to save relay config: ${String(err)}`;
      }
      state.pendingRelayConfig = undefined;
    }
    state.relayPairingOpen = false;
    state.relayPairingMode = undefined;
    render(state);
  });

  // Transform modal handlers
  const transformCancel = document.querySelector<HTMLButtonElement>("#transformCancel");
  transformCancel?.addEventListener("click", () => {
    state.transformDraft = undefined;
    render(state);
  });

  const aiTransformBtn = document.querySelector<HTMLButtonElement>("#aiTransformBtn");
  aiTransformBtn?.addEventListener("click", async () => {
    if (!state.transformDraft || state.transformDraft.aiStatus === "analyzing") return;
    state.transformDraft.aiStatus = "analyzing";
    state.transformDraft.aiSteps = [];
    state.transformDraft.aiError = undefined;
    state.transformDraft.aiRawOutput = undefined;
    render(state);

    try {
      await invoke("ai_suggest_transform", {
        accountFolder: state.transformDraft.accountFolder,
        extraCsvPath: state.transformDraft.sourcePath || null,
        currency: state.transformDraft.currency ?? null,
      });
    } catch (err) {
      if (state.transformDraft) {
        state.transformDraft.aiStatus = "error";
        state.transformDraft.aiError = String(err);
        render(state);
      }
    }
  });

  const transformSave = document.querySelector<HTMLButtonElement>("#transformSave");
  transformSave?.addEventListener("click", async () => {
    if (!state.transformDraft) return;
    state.transformDraft.error = undefined;

    const script = document.querySelector<HTMLTextAreaElement>("#transformScript")?.value ?? "";
    state.transformDraft.script = script;

    state.busy = true;
    const isEditOnly = !state.transformDraft.sourcePath;
    state.status = isEditOnly ? "Saving transform..." : "Saving transform & importing CSV...";
    render(state);

    try {
      if (isEditOnly) {
        await invoke("save_transform", {
          accountFolder: state.transformDraft.accountFolder,
          script,
        });
        state.status = "Transform saved";
        state.transformDraft = undefined;
      } else {
        const draft = state.transformDraft;
        const response = await invoke<MutationResponse>("save_transform_and_rebuild_cmd", {
          nowYyyymm: nowYYYYMM(),
          sourcePath: draft.sourcePath,
          accountFolder: draft.accountFolder,
          script,
          accountSet: state.selectedAccountSet ?? "",
        });
        await applyMutationAndRefresh(state, response);
        const pending = draft.pendingPaths ?? [];
        state.transformDraft = undefined;
        if (pending.length > 0 && draft.accountName) {
          // Transform now exists — import the other selected files (same format)
          // in one run; any with a still-unknown format reopens the editor.
          await importCsvPaths(state, draft.accountFolder, draft.accountName, draft.currency ?? null, pending);
        } else {
          state.status = `Imported CSV`;
        }
      }
    } catch (err) {
      if (state.transformDraft) {
        state.transformDraft.error = String(err);
      }
      state.status = `Error: ${String(err)}`;
    } finally {
      state.busy = false;
      render(state);
    }
  });
}

function attachRuleAndTableHandlers(state: AppState, vs: RenderViewState): void {
  const { selectedAccount, allTransactions, filteredTransactions, searchFiltered,
    tradeLinkMap, uncategorisedCount, showValueColumn, isUncategorisedTxn,
    tree, balances, issueGroups, totalIssueCount, issueCounts,
    baseCurrency, isReportsView, isPluginsView } = vs;

  function buildExistingRuleDraft(
    rule: RuleInfo, accountFolder: string, scopeAccount: string,
    defaultScope: "local" | "institution" | "global",
    allRules: RuleInfo[], localTxns: Transaction[], previousView: "accounts" | "reports",
  ) {
    const preview = computeRulePreviewFromDraft(
      { pattern: rule.pattern, matchField: rule.match_field ?? "", ruleId: rule.id,
        amountCondition: rule.amount_condition ?? "", feeCondition: rule.fee_condition ?? "",
        payeeCondition: rule.payee_condition ?? "" },
      allRules, localTxns, selectedAccount,
    );
    return {
      ruleId: rule.id, accountFolder, accountName: scopeAccount,
      pattern: rule.pattern,
      amountAccount: rule.amount_account ?? "", feeAccount: rule.fee_account ?? "",
      comment: rule.comment ?? "", matchField: rule.match_field ?? "",
      amountCondition: rule.amount_condition ?? "", feeCondition: rule.fee_condition ?? "",
      payeeCondition: rule.payee_condition ?? "",
      narrationCondition: rule.narration_condition ?? "",
      commodityCondition: rule.commodity_condition ?? "",
      metaCondition: rule.meta_condition ?? "",
      filterPills: [] as SearchPill[], ruleScope: defaultScope, allRules, previewMatches: preview,
      scopedTransactions: localTxns, previousView,
    };
  }

  async function buildNewRuleDraft(
    row: HTMLTableRowElement, categoryCell: HTMLTableCellElement, narration: string,
    accountFolder: string, scopeAccount: string,
    defaultScope: "local" | "institution" | "global",
    allRules: RuleInfo[], st: AppState, previousView: "accounts" | "reports",
  ) {
    const currentValue = (categoryCell.textContent?.trim() ?? "");
    const payeeCell = row.querySelector<HTMLTableCellElement>('[data-rule-type="payee"]');
    const displayPayee = payeeCell?.getAttribute("data-display-payee") ?? "";
    const sourcePayee = payeeCell?.getAttribute("data-source-payee") ?? "";
    const carriedFilters = (st.searchPills ?? []).filter((p) => p.key !== "field");
    const extracted = extractRuleConfigPills(carriedFilters);
    const { amountCondition, feeCondition, filterPills } = extracted;
    const { pattern, matchField, payeeCondition } = derivePatternAndPayeeCondition(
      narration, displayPayee, sourcePayee, extracted.payeeCondition,
    );
    const scopedTxns = await filterBySearch(st.selectedAccountSet ?? "", filterPills, scopeAccount);
    const preview = computeRulePreviewFromDraft(
      { pattern, matchField, ruleId: "", amountCondition, feeCondition, payeeCondition },
      allRules, scopedTxns, selectedAccount,
    );
    return {
      ruleId: "", accountFolder, accountName: scopeAccount, pattern,
      amountAccount: (currentValue === "\u2014" ? "" : currentValue) || "expenses:unknown",
      feeAccount: "", comment: "", matchField, amountCondition, feeCondition, payeeCondition,
      narrationCondition: extracted.narrationCondition,
      commodityCondition: extracted.commodityCondition,
      metaCondition: extracted.metaCondition,
      filterPills, ruleScope: defaultScope, allRules, previewMatches: preview,
      scopedTransactions: scopedTxns, previousView,
    };
  }

  function computeDefaultRuleScope(scopeAccount: string, leafAccount: string): "local" | "institution" | "global" {
    if (scopeAccount === leafAccount) return "local";
    if (accountPrefixForScope(leafAccount, "institution").startsWith(scopeAccount)) return "institution";
    return "global";
  }

  // Open rule editor for a given transaction row
  async function openRuleEditorForRow(row: HTMLTableRowElement, st: AppState) {
    const categoryCell = row.querySelector<HTMLTableCellElement>('[data-rule-type="category"]');
    if (!categoryCell) return;
    const ruleId = categoryCell.getAttribute("data-rule-id") ?? "";
    const accountFolder = categoryCell.getAttribute("data-account-folder") ?? "";
    const narration = categoryCell.getAttribute("data-narration") ?? "";
    if (!accountFolder) return;

    try {
      const allRules = await invoke<RuleInfo[]>("get_rules", { accountFolder });
      const previousView: "accounts" | "reports" = st.sidebarView === "reports" ? "reports" : "accounts";
      const scopeAccount = st.selectedAccount ?? "";
      const localTxns = filterByAccountPrefix(st.parse?.transactions ?? [], scopeAccount);
      const defaultScope = computeDefaultRuleScope(scopeAccount, folderToAccountName(accountFolder));

      const matchedRule = ruleId ? allRules.find((r) => r.id === ruleId) : undefined;
      st.ruleEditorDraft = matchedRule && !isDefaultRule(matchedRule)
        ? buildExistingRuleDraft(matchedRule, accountFolder, scopeAccount, defaultScope, allRules, localTxns, previousView)
        : await buildNewRuleDraft(row, categoryCell, narration, accountFolder, scopeAccount, defaultScope, allRules, st, previousView);
      st.sidebarView = "rule-editor";
      render(st);
    } catch (err) { st.status = `Error loading rules: ${String(err)}`; render(st); }
  }

  // Right-click-to-copy on payee/address elements
  document.querySelectorAll<HTMLElement>(".copyable").forEach((el) => {
    el.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      e.stopPropagation();
      const text = el.getAttribute("data-copy") ?? el.textContent ?? "";
      copyToClipboard(text);
      el.classList.add("copyable--copied");
      setTimeout(() => el.classList.remove("copyable--copied"), 1200);
    });
  });

  // Right-click on commodity selects the text so the user can Cmd+C
  document.querySelectorAll<HTMLSpanElement>(".commodity-clickable").forEach((el) => {
    el.addEventListener("contextmenu", (e) => {
      e.stopPropagation();
      const sel = window.getSelection();
      if (sel) {
        const range = document.createRange();
        range.selectNodeContents(el);
        sel.removeAllRanges();
        sel.addRange(range);
      }
    });
  });

  // Right-click on any cell in a transaction row — opens rule editor (delegated)
  {
    const txTable = document.querySelector(".txTable");
    txTable?.addEventListener("contextmenu", async (e) => {
      const target = e.target as HTMLElement;
      const row = target.closest<HTMLTableRowElement>("[data-txn-row-id]");
      if (!row) return;
      // Let specific copy handlers (payee address, commodity) handle their own right-click
      if (target.closest(".copyable, .commodity-clickable")) return;
      e.preventDefault();
      if (state.busy) return;
      await openRuleEditorForRow(row, state);
    });
  }

  // Left-click anywhere on a transaction row — expand/collapse detail (delegated)
  {
    const txTable = document.querySelector(".txTable");
    txTable?.addEventListener("click", async (e) => {
      const target = e.target as HTMLElement;
      const row = target.closest<HTMLTableRowElement>("[data-txn-row-id]");
      if (!row) return;
      if (target.closest(".cell-clickable, .commodity-clickable, .value-clickable, .ai-sparkle-btn, .txn-delete-btn, .dateLink, [data-tx-group-toggle], button, a, input")) return;
      if (target.closest(".txRow__detail")) return;
      if (state.busy) return;
      const expandKey = row.getAttribute("data-expand-key") ?? "";
      if (!expandKey) return;
      await toggleTxnExpand(state, row, expandKey);
      render(state);
    });
  }

  // Click on explorer link in detail row
  {
    const txTable = document.querySelector(".txTable");
    txTable?.addEventListener("click", async (e) => {
      const target = e.target as HTMLElement;
      const link = target.closest<HTMLElement>("[data-open-explorer]");
      if (!link) return;
      e.preventDefault();
      e.stopPropagation();
      const url = link.getAttribute("data-open-explorer") ?? "";
      if (url) {
        try { await invoke("open_url", { url }); } catch (err) { state.status = `Error opening URL: ${String(err)}`; render(state); }
      }
    });
  }

  // "Edit Rule" button in detail row
  {
    const txTable = document.querySelector(".txTable");
    txTable?.addEventListener("click", async (e) => {
      const target = e.target as HTMLElement;
      const btn = target.closest<HTMLElement>("[data-edit-rule-from-detail]");
      if (!btn) return;
      e.stopPropagation();
      const expandKey = btn.getAttribute("data-edit-rule-from-detail") ?? "";
      const row = txTable?.querySelector<HTMLTableRowElement>(`[data-expand-key="${CSS.escape(expandKey)}"]`);
      if (!row || state.busy) return;
      await openRuleEditorForRow(row, state);
    });
  }

  // Collect unique account names from current state for autocomplete
  const collectAccountSuggestions = () => collectAccountSuggestionsFrom(state);

  // Shared inline-edit helper: creates an input in the element, calls onSave with the new value.
  function startCellEdit(
    el: HTMLElement,
    opts: { initialValue: string; placeholder?: string; width?: string; select?: boolean; suggestions?: string[] },
    onSave: (value: string) => Promise<void>,
  ) {
    if (state.busy || el.querySelector(".cell-edit-input")) return;
    const originalHtml = el.innerHTML;

    const wrapper = document.createElement("div");
    wrapper.className = "cell-edit-wrapper";
    wrapper.style.position = "relative";

    const input = document.createElement("input");
    input.type = "text";
    input.className = "cell-edit-input";
    input.value = opts.initialValue;
    input.setAttribute("autocomplete", "off");
    input.setAttribute("autocapitalize", "off");
    input.setAttribute("autocorrect", "off");
    input.setAttribute("spellcheck", "false");
    if (opts.placeholder) input.placeholder = opts.placeholder;
    if (opts.width) input.style.width = opts.width;

    wrapper.appendChild(input);
    el.innerHTML = "";
    el.appendChild(wrapper);
    input.focus();
    if (opts.select) input.select();

    if (opts.suggestions && opts.suggestions.length > 0) {
      // Account autocomplete: delegate dropdown, blur tolerance, "+ Add"
      // gating, and keyboard navigation to the shared component.
      let controller: AccountInputController | null = null;
      controller = attachAccountInput(input, {
        suggestions: opts.suggestions,
        allowCreate: true,
      }, async (value, kind) => {
        controller?.destroy();
        if (kind === "cancel" || kind === "revert" || !value || value === opts.initialValue) {
          el.innerHTML = originalHtml;
          return;
        }
        await onSave(value);
      });
      return;
    }

    // Plain text edit (e.g. commodity rename). Enter saves, Escape cancels,
    // blur saves whatever is typed.
    let saved = false;
    async function saveEdit() {
      if (saved) return;
      saved = true;
      const value = input.value.trim();
      if (!value || value === opts.initialValue || value === opts.placeholder) {
        el.innerHTML = originalHtml;
        return;
      }
      await onSave(value);
    }
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") { e.preventDefault(); saveEdit(); }
      else if (e.key === "Escape") { e.preventDefault(); saved = true; el.innerHTML = originalHtml; }
    });
    input.addEventListener("blur", () => saveEdit());
  }

  // Left-click on payee/category cells — inline edit
  // Payee clicks save a LABEL (_labels.json) for payee renaming.
  // Category clicks save a RULE (_rules.json) for account categorization.
  document.querySelectorAll<HTMLTableCellElement>(".cell-clickable").forEach((el) => {
    el.addEventListener("click", (e) => {
      // Don't intercept clicks on commodity elements inside the cell
      if ((e.target as HTMLElement).closest(".commodity-clickable")) return;
      e.stopPropagation(); // Prevent row click (rule editor) from firing
      const ruleType = el.getAttribute("data-rule-type") as "payee" | "category";
      const narration = el.getAttribute("data-narration") ?? "";
      const accountFolder = el.getAttribute("data-account-folder") ?? "";
      if (!narration) return;
      const primaryCopyable = el.querySelector(".copyable");
      const originalValue = primaryCopyable
        ? (primaryCopyable.getAttribute("data-copy") ?? primaryCopyable.textContent ?? "").trim()
        : (el.textContent?.trim() ?? "");

      const cellSuggestions = ruleType === "category" ? collectAccountSuggestions() : undefined;
      startCellEdit(el, { initialValue: originalValue === "\u2014" ? "" : originalValue, select: true, suggestions: cellSuggestions }, async (value) => {
        const label = ruleType === "payee" ? "Saving label..." : "Saving rule...";
        mutationQueue.enqueue(state, label, async () => {
          let response: MutationResponse;
          if (ruleType === "payee") {
            const sourcePayee = el.getAttribute("data-source-payee") ?? "";
            const pattern = sourcePayee ? `*${sourcePayee}*` : `*${narration}*`;
            const matchField = sourcePayee ? "payee" : null;
            response = await invoke<MutationResponse>("save_label", {
              nowYyyymm: nowYYYYMM(), accountFolder: targetFolderForScope(accountFolder, "institution"), pattern,
              payee: value, commodity: null, matchField, accountSet: state.selectedAccountSet ?? "",
            });
            state.status = "Label saved";
          } else {
            const row = el.closest<HTMLTableRowElement>("[data-txn-row-id]");
            const txnId = row?.getAttribute("data-txn-row-id") ?? "";
            const legId = row?.getAttribute("data-leg-id") ?? "";
            const { pattern, matchField } = categoryEditRuleMatch(legId, txnId, narration);
            response = await invoke<MutationResponse>("save_rule", {
              nowYyyymm: nowYYYYMM(), accountFolder, pattern,
              payee: null, commodity: null, matchField, amountCondition: null, feeCondition: null, amountAccount: value, feeAccount: null, comment: null, accountSet: state.selectedAccountSet ?? "",
            });
            state.status = "Rule saved";
          }
          await applyMutationResponse(state, response);
        }).catch((err) => {
          state.status = `Error: ${String(err)}`;
          render(state);
        });
      });
    });
  });

  // Value cell click — open set-price modal
  document.querySelectorAll<HTMLTableCellElement>(".value-clickable").forEach((el) => {
    el.addEventListener("click", (e) => {
      e.stopPropagation();
      if (state.busy) return;
      const commodity = el.getAttribute("data-value-commodity") ?? "";
      const datetime = el.getAttribute("data-value-datetime") ?? "";
      if (!commodity || !datetime) return;
      state.setPriceDraft = {
        datetime: datetime.slice(0, 10),
        commodity,
        priceAmount: "",
        quoteCurrency: state.displayConfig?.base_currency ?? "USD",
      };
      render(state);
      setTimeout(() => document.getElementById("setPriceAmount")?.focus(), 50);
    });
  });

  // Commodity rename — inline-edit, saves as a commodity rule
  document.querySelectorAll<HTMLSpanElement>(".commodity-clickable").forEach((el) => {
    el.addEventListener("click", (e) => {
      e.stopPropagation(); // Prevent row click (rule editor) from firing
      const oldCommodity = el.getAttribute("data-commodity") ?? "";
      const accountFolder = el.getAttribute("data-account-folder") ?? "";
      if (!oldCommodity) return;

      startCellEdit(el, { initialValue: oldCommodity, placeholder: oldCommodity, width: "6em" }, async (value) => {
        mutationQueue.enqueue(state, "Saving commodity rule...", async () => {
          const response = await invoke<MutationResponse>("save_rule", {
            nowYyyymm: nowYYYYMM(), accountFolder, pattern: oldCommodity,
            payee: null, commodity: value, matchField: "commodity", amountCondition: null, feeCondition: null, amountAccount: null, feeAccount: null, comment: null, accountSet: state.selectedAccountSet ?? "",
          });
          await applyMutationResponse(state, response);
          state.status = `Commodity "${oldCommodity}" renamed to "${value}"`;
        }).catch((err) => {
          state.status = `Error: ${String(err)}`;
          render(state);
        });
      });
    });
  });

  // Transaction delete/hide buttons — routed through the mutation queue
  // so rapid clicks don't pile up parallel pipeline rebuilds, and the
  // rebuild strip stays lit across the whole batch.
  document.querySelectorAll<HTMLButtonElement>(".txn-delete-btn").forEach((btn) => {
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const isManual = btn.getAttribute("data-txn-is-manual") === "true";
      const label = isManual ? "Deleting transaction..." : "Hiding transaction...";
      const datetime = btn.getAttribute("data-txn-datetime") ?? "";
      const payee = btn.getAttribute("data-txn-payee") ?? "";
      const narration = btn.getAttribute("data-txn-narration") ?? "";
      const txnId = btn.getAttribute("data-txn-id") ?? "";
      const rowAccountFolder = btn.getAttribute("data-account-folder") ?? "";
      pushDebug("info", `delete:click txnId=${txnId.slice(0, 16)} folder=${rowAccountFolder.slice(0, 40)}`);
      // Step 1: remove the row from the DOM IMMEDIATELY. No waiting on
      // a render cycle — the user sees the row vanish in the same frame
      // as the click. Plus add to pendingDeletes so any deferred render
      // that fires before the backend rebuild lands also filters it
      // out (and the renderer's morphdom keying preserves identity for
      // every other row, so siblings don't shuffle).
      const tr = btn.closest("tr");
      const detail = tr?.nextElementSibling?.matches('[data-txn-detail-for]')
        ? tr.nextElementSibling
        : null;
      tr?.remove();
      detail?.remove();
      // Without pruning, a row the user previously expanded still has
      // its expandKey in state.txExpandedRows after the delete. If the
      // row ever re-appears (multi-leg sibling, "Show Ignored" toggled
      // back on, or a rule that didn't catch it), it renders expanded
      // — a "hidden" row surfaces under whatever the user clicks next.
      if (txnId) {
        pruneExpandedKeysForTxn(state.txExpandedRows, txnId);
        state.pendingDeletes.add(txnId);
      }
      // The strip is outside the txn table and survives renders, so
      // setting its class directly gives a same-frame "working..." signal.
      const strip = document.querySelector('[data-testid="rebuild-strip"]');
      strip?.classList.add("rebuildStrip--active");
      strip?.setAttribute("aria-hidden", "false");
      mutationQueue.enqueue(state, label, async () => {
        if (isManual) {
          const selAcct = state.selectedAccount ?? "";
          const acctFolder = selAcct ? resolveAccountFolder(state, selAcct) : "";
          const response = await invoke<PipelineResponse>("delete_manual_transaction", {
            nowYyyymm: nowYYYYMM(), datetime, payee, narration,
            accountFolder: acctFolder,
            accountSet: state.selectedAccountSet ?? "",
          });
          await applyPipelineResponse(state, response);
        } else {
          pushDebug("info", `delete:invoke hide_transaction folder=${rowAccountFolder.slice(0, 40)}`);
          const response = await invoke<MutationResponse>("hide_transaction", {
            nowYyyymm: nowYYYYMM(), txnId,
            accountFolder: rowAccountFolder,
            accountSet: state.selectedAccountSet ?? "",
          });
          pushDebug("info", `delete:invoked ok=${response.ok}`);
          await applyMutationResponse(state, response);
          pushDebug("info", `delete:applied searchFiltered=${state.searchFilteredTransactions?.length ?? 0}`);
        }
        state.status = isManual ? "Transaction deleted." : "Transaction hidden.";
      }).catch((err) => {
        // Pipeline rejected — un-mark so the row reappears at full opacity.
        if (txnId) state.pendingDeletes.delete(txnId);
        state.status = `Error: ${err}`;
        render(state);
      }).finally(() => {
        // After the rebuild lands the txn is gone from data; the entry
        // is now redundant. Clear it so the set can't grow unbounded
        // across long sessions.
        if (txnId) state.pendingDeletes.delete(txnId);
      });
    });
  });

  // Trade link buttons
  document.querySelectorAll<HTMLButtonElement>("[data-link-txn]").forEach((btn) => {
    btn.addEventListener("click", async (e) => {
      e.stopPropagation();
      const txnId = btn.getAttribute("data-link-txn") ?? "";
      console.warn(`[trade-link] clicked txnId="${txnId}", current selection="${state.tradeLinkSelection ?? "none"}"`);
      if (!txnId) return;
      if (state.tradeLinkSelection && state.tradeLinkSelection !== txnId) {
        // Second click: link the two transactions
        console.warn(`[trade-link] linking ${state.tradeLinkSelection} <-> ${txnId}`);
        state.busy = true;
        state.status = "Linking transactions...";
        render(state);
        try {
          const linkParams = tradeLinkParams(state, state.tradeLinkSelection, txnId);
          const response = await invoke<PipelineResponse>("save_trade_link", {
            nowYyyymm: nowYYYYMM(),
            txnIdA: state.tradeLinkSelection,
            txnIdB: txnId,
            accountFolder: linkParams.accountFolder,
            isASell: linkParams.isASell,
            accountSet: state.selectedAccountSet ?? "",
          });
          await applyPipelineResponse(state, response);
          const links = await invoke<TradeLink[]>("get_trade_links");
          state.tradeLinks = links;
          state.tradeLinkSelection = undefined;
          state.status = "Transactions linked.";
          // Refresh suggestions
          try {
            const suggestions = await invoke<TradeSuggestion[]>("suggest_trade_links_cmd", {
              accountSet: state.selectedAccountSet ?? "",
              baseCurrency: state.displayConfig?.base_currency ?? null,
            });
            state.tradeSuggestions = suggestions;
          } catch (e) { console.warn("trade suggestions failed:", e); }
        } catch (err) {
          state.status = `Error: ${err}`;
        } finally {
          state.busy = false;
          render(state);
        }
      } else {
        // First click: start selection
        state.tradeLinkSelection = txnId;
        render(state);
      }
    });
  });

  document.querySelectorAll<HTMLButtonElement>("[data-unlink-id]").forEach((btn) => {
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const linkId = btn.getAttribute("data-unlink-id") ?? "";
      if (!linkId) return;
      mutationQueue.enqueue(state, "Unlinking trade...", async () => {
        const link = state.tradeLinks.find((l) => l.id === linkId);
        const unlinkFolder = link
          ? tradeLinkParams(state, link.txn_id_a, link.txn_id_b).accountFolder
          : "";
        const response = await invoke<PipelineResponse>("delete_trade_link", {
          nowYyyymm: nowYYYYMM(),
          linkId,
          accountFolder: unlinkFolder,
          accountSet: state.selectedAccountSet ?? "",
        });
        await applyPipelineResponse(state, response);
        const links = await invoke<TradeLink[]>("get_trade_links");
        state.tradeLinks = links;
        state.status = "Trade unlinked.";
        try {
          const suggestions = await invoke<TradeSuggestion[]>("suggest_trade_links_cmd", {
            accountSet: state.selectedAccountSet ?? "",
            baseCurrency: state.displayConfig?.base_currency ?? null,
          });
          state.tradeSuggestions = suggestions;
        } catch (e) { console.warn("trade suggestions failed:", e); }
      }).catch((err) => {
        state.status = `Error: ${err}`;
        render(state);
      });
    });
  });

  // Chain connector buttons (between potential trade pairs — single or group)
  document.querySelectorAll<HTMLButtonElement>(".chain-btn").forEach((btn) => {
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const groupSellJson = btn.getAttribute("data-chain-group-sell");
      mutationQueue.enqueue(state, "Linking trades...", async () => {
        if (groupSellJson) {
          await handleGroupTradeLink(btn, state, groupSellJson);
        } else {
          await handleSingleTradeLink(btn, state);
        }
        await refreshTradeLinks(state);
      }).catch((err) => {
        state.status = `Error: ${err}`;
        render(state);
      });
    });
  });

  // Accept trade suggestion from issues panel
  document.querySelectorAll<HTMLButtonElement>("[data-accept-trade]").forEach((btn) => {
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const idx = parseInt(btn.getAttribute("data-accept-trade") ?? "", 10);
      const suggestion = state.tradeSuggestions[idx];
      if (!suggestion) return;
      mutationQueue.enqueue(state, "Linking trade...", async () => {
        const suggestParams = tradeLinkParams(state, suggestion.txn_id_a, suggestion.txn_id_b);
        const response = await invoke<PipelineResponse>("save_trade_link", {
          nowYyyymm: nowYYYYMM(),
          txnIdA: suggestion.txn_id_a,
          txnIdB: suggestion.txn_id_b,
          accountFolder: suggestParams.accountFolder,
          isASell: suggestParams.isASell,
          accountSet: state.selectedAccountSet ?? "",
        });
        await applyPipelineResponse(state, response);
        const links = await invoke<TradeLink[]>("get_trade_links");
        state.tradeLinks = links;
        state.status = "Trade linked.";
        try {
          const suggestions = await invoke<TradeSuggestion[]>("suggest_trade_links_cmd", {
            accountSet: state.selectedAccountSet ?? "",
            baseCurrency: state.displayConfig?.base_currency ?? null,
          });
          state.tradeSuggestions = suggestions;
        } catch (e) { console.warn("trade suggestions failed:", e); }
      }).catch((err) => {
        state.status = `Error: ${err}`;
        render(state);
      });
    });
  });

  // Link All suggested trades
  const linkAllBtn = document.querySelector<HTMLButtonElement>("#linkAllTrades");
  linkAllBtn?.addEventListener("click", (e) => {
    e.stopPropagation();
    const suggestions = state.tradeSuggestions ?? [];
    if (suggestions.length === 0) return;
    const label = `Linking ${suggestions.length} trade${suggestions.length === 1 ? "" : "s"}...`;
    mutationQueue.enqueue(state, label, async () => {
      const links = suggestions.map((s) => {
        const params = tradeLinkParams(state, s.txn_id_a, s.txn_id_b);
        return {
          txn_id_a: s.txn_id_a,
          txn_id_b: s.txn_id_b,
          account_folder: params.accountFolder,
          is_a_sell: params.isASell,
        };
      });
      const response = await invoke<PipelineResponse>("save_trade_links_bulk", {
        nowYyyymm: nowYYYYMM(),
        links,
        accountSet: state.selectedAccountSet ?? "",
      });
      await applyPipelineResponse(state, response);
      const updatedLinks = await invoke<TradeLink[]>("get_trade_links");
      state.tradeLinks = updatedLinks;
      state.status = `Linked ${suggestions.length} trade${suggestions.length === 1 ? "" : "s"}.`;
      try {
        const newSuggestions = await invoke<TradeSuggestion[]>("suggest_trade_links_cmd", {
          accountSet: state.selectedAccountSet ?? "",
          baseCurrency: state.displayConfig?.base_currency ?? null,
        });
        state.tradeSuggestions = newSuggestions;
      } catch { /* ignore */ }
    }).catch((err) => {
      state.status = `Error: ${err}`;
      render(state);
    });
  });

  const cancelTradeLink = document.querySelector<HTMLButtonElement>("#cancelTradeLink");
  cancelTradeLink?.addEventListener("click", () => {
    state.tradeLinkSelection = undefined;
    render(state);
  });

  attachTableControlHandlers(state, vs);
}

function attachTableControlHandlers(state: AppState, vs: RenderViewState): void {
  const { selectedAccount, allTransactions, filteredTransactions, searchFiltered,
    tradeLinkMap, uncategorisedCount, showValueColumn, isUncategorisedTxn,
    tree, balances, issueGroups, totalIssueCount, issueCounts,
    baseCurrency, isReportsView, isPluginsView } = vs;

  // Issues panel handlers
  const issuesTab = document.querySelector<HTMLButtonElement>("#issuesTab");
  issuesTab?.addEventListener("click", () => {
    state.issuesPanelOpen = state.issuesPanelOpen === false ? true : false;
    render(state);
  });

  // Infinite scroll — fetch the next page when the user nears the bottom.
  {
    vsCleanup?.();
    vsCleanup = null;
    const contentEl = document.querySelector(".content");
    const loaded = state.searchFilteredTransactions?.length ?? 0;
    const hasMore = state.searchFilteredCount !== undefined
      && loaded > 0
      && loaded < state.searchFilteredCount;
    if (contentEl && hasMore) {
      let loading = false;
      const onScroll = async () => {
        if (loading) return;
        const el = contentEl as HTMLElement;
        const scrollBottom = el.scrollTop + el.clientHeight;
        if (scrollBottom < el.scrollHeight - 200) return;
        loading = true;
        const scrollY = el.scrollTop;
        state.txWindowStart += TX_WINDOW;
        await runSearchFilter(state);
        render(state);
        const restored = document.querySelector(".content") as HTMLElement | null;
        if (restored) restored.scrollTop = scrollY;
        loading = false;
      };
      contentEl.addEventListener("scroll", onScroll, { passive: true });
      vsCleanup = () => contentEl.removeEventListener("scroll", onScroll);
    }
  }

  document.querySelector<HTMLButtonElement>("#reportShowMore")?.addEventListener("click", () => {
    state.reportPageSize += 50;
    render(state);
  });

  // Navigation back/forward buttons
  document.querySelector<HTMLButtonElement>("#navBack")?.addEventListener("click", () => {
    navigateBack(state, render);
    // If navigating back to reports, reload the report data
    if (state.sidebarView === "reports" && state.selectedReport) {
      void loadReport(state).then(() => render(state));
    }
  });
  document.querySelector<HTMLButtonElement>("#navForward")?.addEventListener("click", () => {
    navigateForward(state, render);
    if (state.sidebarView === "reports" && state.selectedReport) {
      void loadReport(state).then(() => render(state));
    }
  });

  // Transaction aggregation group toggle
  // Group toggle — delegated to txTable so it works after virtual-scroll tbody updates
  {
    const txTable = document.querySelector(".txTable");
    txTable?.addEventListener("click", (e) => {
      const target = e.target as HTMLElement;
      const toggle = target.closest<HTMLElement>("[data-tx-group-toggle]");
      if (!toggle) return;
      e.stopPropagation();
      const key = toggle.getAttribute("data-tx-group-toggle");
      if (!key) return;
      if (!state.txExpandedGroups) state.txExpandedGroups = new Set();
      if (state.txExpandedGroups.has(key)) {
        state.txExpandedGroups.delete(key);
      } else {
        state.txExpandedGroups.add(key);
      }
      render(state);
    });
  }

  // CGT commodity-group toggle (per section: data-cgt-group is "{section}:{commodity}")
  document.querySelectorAll<HTMLTableRowElement>("[data-cgt-group]").forEach((row) => {
    row.addEventListener("click", () => {
      const key = row.getAttribute("data-cgt-group");
      if (!key) return;
      if (!state.cgtExpandedGroups) state.cgtExpandedGroups = new Set();
      if (state.cgtExpandedGroups.has(key)) {
        state.cgtExpandedGroups.delete(key);
      } else {
        state.cgtExpandedGroups.add(key);
      }
      render(state);
    });
  });

  // Income commodity-group toggle (per section: data-income-group is "{section}:{commodity}")
  document.querySelectorAll<HTMLTableRowElement>("[data-income-group]").forEach((row) => {
    row.addEventListener("click", (ev) => {
      // Don't toggle when clicking an inner navigation link (date/category cells)
      if ((ev.target as HTMLElement).closest("a.reportLink")) return;
      const key = row.getAttribute("data-income-group");
      if (!key) return;
      if (!state.incomeExpandedGroups) state.incomeExpandedGroups = new Set();
      if (state.incomeExpandedGroups.has(key)) {
        state.incomeExpandedGroups.delete(key);
      } else {
        state.incomeExpandedGroups.add(key);
      }
      render(state);
    });
  });

  const incomeFilter = document.getElementById("incomeFilter") as HTMLInputElement | null;
  if (incomeFilter) {
    incomeFilter.addEventListener("input", () => {
      state.incomeFilterText = incomeFilter.value;
      render(state);
      // Keep focus + caret position after re-render
      const next = document.getElementById("incomeFilter") as HTMLInputElement | null;
      if (next) {
        next.focus();
        const len = next.value.length;
        next.setSelectionRange(len, len);
      }
    });
  }

  // Balances commodity row toggle — expand to show per-leaf-account breakdown
  document.querySelectorAll<HTMLTableRowElement>("[data-balance-commodity]").forEach((row) => {
    const toggle = () => {
      const key = row.getAttribute("data-balance-commodity");
      if (!key) return;
      if (!state.balancesExpanded) state.balancesExpanded = new Set();
      if (state.balancesExpanded.has(key)) {
        state.balancesExpanded.delete(key);
      } else {
        state.balancesExpanded.add(key);
      }
      render(state);
    };
    row.addEventListener("click", toggle);
    row.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter" || ev.key === " ") {
        ev.preventDefault();
        toggle();
      }
    });
  });

  // Tax Savings holding row toggle — expand to show the FIFO parcel breakdown
  document.querySelectorAll<HTMLTableRowElement>("[data-loss-commodity]").forEach((row) => {
    const toggle = () => {
      const key = row.getAttribute("data-loss-commodity");
      if (!key) return;
      if (!state.lossHarvestExpanded) state.lossHarvestExpanded = new Set();
      if (state.lossHarvestExpanded.has(key)) {
        state.lossHarvestExpanded.delete(key);
      } else {
        state.lossHarvestExpanded.add(key);
      }
      render(state);
    };
    row.addEventListener("click", toggle);
    row.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter" || ev.key === " ") {
        ev.preventDefault();
        toggle();
      }
    });
  });

  attachDebouncedFilterInput(state, "#cgtFilter", (s, v) => {
    s.cgtFilterText = v;
    s.reportPageSize = 50;
  });

  attachDebouncedFilterInput(state, "#balancesFilter", (s, v) => {
    s.balancesFilterText = v;
  });

  // Sort header clicks (shared across accounts, rule editor, and report views)
  document.querySelectorAll<HTMLTableCellElement>("th.sortable").forEach((th) => {
    th.addEventListener("click", () => {
      const col = th.getAttribute("data-sort-col");
      if (!col) return;
      const scope = th.getAttribute("data-sort-scope");
      applySortToggle(state, scope, col);
      if (state.searchFilteredTransactions) {
        state.txWindowStart = 0;
        runSearchFilter(state).then(() => render(state));
        return;
      }
      render(state);
    });
  });

  document.querySelectorAll<HTMLButtonElement>(".issueGroup__filterBtn").forEach((el) => {
    el.addEventListener("click", (e) => {
      e.stopPropagation();
      const kind = el.getAttribute("data-filter-kind") ?? "";
      state.issueFilter = state.issueFilter === kind ? undefined : kind;
      render(state);
    });
  });

  const clearIssueFilter = document.querySelector<HTMLButtonElement>("#clearIssueFilter");
  clearIssueFilter?.addEventListener("click", () => {
    state.issueFilter = undefined;
    render(state);
  });

  // Clickable dates in transaction table: add date as search pill
  document.querySelectorAll<HTMLAnchorElement>(".dateLink").forEach((el) => {
    el.addEventListener("click", (e) => {
      e.preventDefault();
      const date = el.getAttribute("data-date-pill") ?? "";
      if (!date) return;
      // Replace existing date pill or add new one
      state.searchPills = state.searchPills.filter((p) => p.key !== "date");
      state.searchPills.push({ key: "date", value: date });
      state.search = searchFromPills(state.searchPills, state.searchText);
      state.txWindowStart = 0;
      render(state);
    });
  });

  // Click issue to scroll to transaction
  document.querySelectorAll<HTMLLIElement>("[data-scroll-to-txn]").forEach((el) => {
    el.addEventListener("click", (e) => {
      if ((e.target as HTMLElement).closest("button")) return; // don't intercept button clicks
      const txnId = el.getAttribute("data-scroll-to-txn") ?? "";
      const row = document.querySelector<HTMLTableRowElement>(`[data-txn-row-id="${CSS.escape(txnId)}"]`);
      if (row) {
        row.scrollIntoView({ behavior: "smooth", block: "center" });
        row.classList.add("txRow--highlight");
        setTimeout(() => row.classList.remove("txRow--highlight"), 2000);
      }
    });
  });

  document.querySelectorAll<HTMLButtonElement>(".issueItem__reveal").forEach((el) => {
    el.addEventListener("click", async (e) => {
      e.stopPropagation();
      const path = el.getAttribute("data-reveal-path") ?? "";
      if (path) {
        try {
          await invoke("reveal_in_finder", { path });
        } catch (err) {
          state.status = `Reveal failed: ${String(err)}`;
          render(state);
        }
      }
    });
  });

  attachRuleEditorPageHandlers(state);
}

function attachRuleEditorPageHandlers(state: AppState): void {
  // Rule editor page — smart search bar + live preview
  if (state.sidebarView === "rule-editor" && state.ruleEditorDraft) {
    let previewTimer: ReturnType<typeof setTimeout> | null = null;

    async function updatePreview() {
      if (!state.ruleEditorDraft) return;
      const activeId = document.activeElement?.id;
      const cursorPos = (document.activeElement as HTMLInputElement)?.selectionStart ?? null;

      const draft = state.ruleEditorDraft;
      const ruleKeywords = [...TRANSACTION_SEARCH_KEYWORDS, "field"] as string[];
      const { pills: textPills, text: cleanPattern } = parseSmartInput(draft.pattern, ruleKeywords);
      const { conditions, filterPills: effectiveFilterPills } = mergeRuleConditions(draft, textPills);
      const accountPrefix = accountPrefixForScope(draft.accountName, draft.ruleScope);
      const scopedTxns = await loadPreviewScope(state, effectiveFilterPills, accountPrefix);
      draft.scopedTransactions = scopedTxns;
      const previewPattern = !cleanPattern.trim() && effectiveFilterPills.length > 0 ? "*" : cleanPattern;
      draft.previewMatches = computeRulePreviewFromDraft(
        { pattern: previewPattern, ...conditions, ruleId: draft.ruleId },
        draft.allRules, scopedTxns, draft.accountName,
      );

      render(state);
      restoreFocusAfterRender(activeId, cursorPos);
    }

    // Build initial pills from draft state
    const initialPills: SearchPill[] = [
      ...draftConditionsToPills(state.ruleEditorDraft),
      ...state.ruleEditorDraft.filterPills,
    ];

    let prevRulePillCount = initialPills.length;
    cleanupRuleSearch?.();
    cleanupRuleSearch = attachSmartSearch("ruleSearch", initialPills, {
      keywords: [...TRANSACTION_SEARCH_KEYWORDS, "field"],
      valueSuggestions: { field: ["narration", "payee", "meta", "commodity"] },
    }, (change) => {
      if (!state.ruleEditorDraft) return;
      // Convert pills back to draft fields — rule-config pills are extracted,
      // everything else is a transaction filter (same keywords as accounts search)
      state.ruleEditorDraft.pattern = change.text;
      const pillExtracted = extractRuleConfigPills(change.pills);
      state.ruleEditorDraft.matchField = pillExtracted.matchField;
      state.ruleEditorDraft.amountCondition = pillExtracted.amountCondition;
      state.ruleEditorDraft.feeCondition = pillExtracted.feeCondition;
      state.ruleEditorDraft.payeeCondition = pillExtracted.payeeCondition;
      state.ruleEditorDraft.narrationCondition = pillExtracted.narrationCondition;
      state.ruleEditorDraft.commodityCondition = pillExtracted.commodityCondition;
      state.ruleEditorDraft.metaCondition = pillExtracted.metaCondition;
      state.ruleEditorDraft.filterPills = pillExtracted.filterPills;
      const pillsChanged = change.pills.length !== prevRulePillCount;
      prevRulePillCount = change.pills.length;
      const text = change.text.trim();
      // Only update preview when: pills changed, text cleared, or text has 2+ chars
      if (!pillsChanged && text.length === 1) return;
      if (previewTimer) clearTimeout(previewTimer);
      previewTimer = setTimeout(updatePreview, 300);
    });

    // Scope radio buttons — update scope and refresh preview
    document.querySelectorAll<HTMLInputElement>('input[name="ruleScope"]').forEach((radio) => {
      radio.addEventListener("change", () => {
        if (state.ruleEditorDraft) {
          state.ruleEditorDraft.ruleScope = radio.value as "local" | "institution" | "global";
          updatePreview();
        }
      });
    });

    // Sync plain input fields to state on every keystroke so render() never loses them
    (["ruleComment", "ruleAmountAccount", "ruleFeeAccount"] as const).forEach((id) => {
      const el = document.querySelector<HTMLInputElement>(`#${id}`);
      if (!el) return;
      el.addEventListener("input", () => {
        if (!state.ruleEditorDraft) return;
        if (id === "ruleComment") state.ruleEditorDraft.comment = el.value;
        else if (id === "ruleAmountAccount") state.ruleEditorDraft.amountAccount = el.value;
        else if (id === "ruleFeeAccount") state.ruleEditorDraft.feeAccount = el.value;
      });
    });

  }

  // Rule editor: account inputs use the shared autocomplete component.
  // The component sets input.value on commit; we mirror that into the
  // draft directly because programmatic value assignment doesn't fire
  // the "input" event that the sync handler above relies on.
  const ruleAccountSuggestions = collectAccountSuggestionsFrom(state);
  ([
    ["ruleAmountAccount", "amountAccount"],
    ["ruleFeeAccount", "feeAccount"],
  ] as const).forEach(([inputId, draftKey]) => {
    const input = document.querySelector<HTMLInputElement>(`#${inputId}`);
    if (!input) return;
    attachAccountInput(input, {
      suggestions: ruleAccountSuggestions,
      allowCreate: true,
    }, (value) => {
      if (state.ruleEditorDraft) {
        state.ruleEditorDraft[draftKey] = value;
      }
    });
  });

  // Rule editor page handlers (cancel/save/delete)
  const ruleCancel = document.querySelector<HTMLButtonElement>("#ruleCancel");
  ruleCancel?.addEventListener("click", () => {
    const prevView = state.ruleEditorDraft?.previousView ?? "accounts";
    state.ruleEditorDraft = undefined;
    state.sidebarView = prevView;
    render(state);
  });

  const ruleSave = document.querySelector<HTMLButtonElement>("#ruleSave");
  ruleSave?.addEventListener("click", () => {
    if (!state.ruleEditorDraft) return;
    const draft = state.ruleEditorDraft;
    const saved = readAndBuildRule(draft);
    // Optimistic close: navigate back to the previous view in the same
    // frame as the click. The pipeline rebuild then runs in the
    // background and the morphdom-driven refreshSearch updates the txn
    // rows in place when it lands. Without this, the user stares at
    // the rule editor for the full ~7s rebuild on a large vault.
    const prevView = draft.previousView ?? "accounts";
    state.ruleEditorDraft = undefined;
    state.sidebarView = prevView;
    render(state);
    mutationQueue.enqueue(state, "Saving rule...", async () => {
      const response = await invokeRuleSave(draft, saved, state.selectedAccountSet ?? "");
      await applyMutationResponse(state, response);
      state.status = "Rule saved";
    }).catch((err) => {
      // Re-open the editor with the in-flight draft so the user
      // doesn't lose their unsaved edits.
      state.ruleEditorDraft = { ...draft, error: String(err) };
      state.sidebarView = "rule-editor";
      state.status = `Error: ${String(err)}`;
      render(state);
    });
  });

  const ruleDelete = document.querySelector<HTMLButtonElement>("#ruleDelete");
  ruleDelete?.addEventListener("click", () => {
    if (!state.ruleEditorDraft) return;
    const draft = state.ruleEditorDraft;
    const prevView = draft.previousView ?? "accounts";
    state.ruleEditorDraft = undefined;
    state.sidebarView = prevView;
    render(state);
    mutationQueue.enqueue(state, "Deleting rule...", async () => {
      const response = await invoke<MutationResponse>("delete_rule", {
        nowYyyymm: nowYYYYMM(),
        accountFolder: draft.accountFolder,
        ruleId: draft.ruleId,
        accountSet: state.selectedAccountSet ?? "",
      });
      await applyMutationResponse(state, response);
      state.status = "Rule deleted";
    }).catch((err) => {
      state.ruleEditorDraft = { ...draft, error: String(err) };
      state.sidebarView = "rule-editor";
      state.status = `Error: ${String(err)}`;
      render(state);
    });
  });

  // Click overlay background to dismiss any modal
  document.querySelectorAll<HTMLDivElement>(".modalOverlay").forEach((overlay) => {
    overlay.addEventListener("click", (e) => {
      if (e.target !== overlay) return;
      state.manualDraft = undefined;
      state.addAccountDraft = undefined;
      state.openingBalanceDraft = undefined;
      state.transformDraft = undefined;
      state.ruleEditorDraft = undefined;
      state.syncLogOpen = false;
      state.relayPairingOpen = false;
      render(state);
    });
  });
}

type AppState = {
  busy: boolean;
  pendingMutations: number;
  pendingDeletes: Set<string>;
  /** Optimistically-added txns shown immediately, before the backend rebuild lands. */
  pendingAdds: Transaction[];
  justAddedTxnIds?: Set<string>;
  parse?: ParseResponse;
  selectedAccount?: string;
  search: string;
  searchPills: SearchPill[];
  searchText: string;
  dateFilter: DateFilter;
  showHidden: boolean;
  status?: string;
  pipelineWarnings: string[];
  drillPath: string[];
  _drillInitialized?: boolean;
  /** Categories pane (non-folder-backed accounts) — parallel to selectedAccount/drillPath
   *  so switching panes never clobbers the other's selection. */
  selectedCategory?: string;
  categoryDrillPath: string[];
  _categoryDrillInitialized?: boolean;
  accountSets: string[];
  selectedAccountSet?: string;
  accountSetMap: Record<string, string[]>;
  /** Every account known across the whole vault (load_account_tree("")),
   *  the global pool for account-name autocomplete. */
  allAccounts: string[];
  accountFoldersMap: Record<string, string>;
  accountPropertiesMap: Record<string, AccountProperties>;
  displayConfig?: DisplayConfig;
  manualDraft?: {
    datetime: string;
    payee: string;
    narration: string;
    account: string; // read-only context: the account "Add New" was opened from
    cashCommodity: string; // the account's native commodity; contras/value are in this
    mode: "value" | "trade";
    amount: string; // value mode
    tradeCommodity: string; // trade mode
    quantity: string;
    price: string;
    contras: { account: string; amount: string }[]; // the "other accounts" rows
    error?: string;
  };
  addAccountDraft?: {
    accountName: string;
    currency: string;
    openingBalance: string;
    error?: string;
  };
  transformDraft?: {
    csvFilename: string;
    sourcePath: string;
    accountFolder: string;
    script: string;
    headers: string[];
    error?: string;
    aiStatus?: "idle" | "analyzing" | "done" | "error";
    aiSteps?: string[];
    aiError?: string;
    aiRawOutput?: string;
    csvTypes?: CsvTypeInfo[];
    currency?: string;
    // Multi-file import: the account context + the other selected files still
    // waiting on this transform, imported automatically once it's saved.
    accountName?: string;
    pendingPaths?: string[];
  };
  syncLogOpen?: boolean;
  syncLog?: SyncEvent[];
  devices?: DeviceInfo[];
  relayConfig?: RelayConfig;
  relayPairingOpen?: boolean;
  relayPairingMode?: "create" | "join";
  relayPairingCode?: string;
  relayPairingError?: string;
  pendingRelayConfig?: RelayConfig;
  accountGaps?: AccountGap[];
  searchError?: string;
  ruleEditorDraft?: {
    ruleId: string;
    accountFolder: string;
    accountName: string;
    pattern: string;
    amountAccount: string;
    feeAccount: string;
    comment: string;
    matchField: string;
    amountCondition: string;
    feeCondition: string;
    payeeCondition: string;
    narrationCondition: string;
    commodityCondition: string;
    metaCondition: string;
    filterPills: SearchPill[];
    ruleScope: "local" | "institution" | "global";
    error?: string;
    allRules: RuleInfo[];
    previewMatches: PreviewMatch[];
    scopedTransactions: Transaction[];
    previousView: "accounts" | "reports";
  };
  issueFilter?: string;
  issuesPanelOpen?: boolean;
  sourceFolderPaths?: Record<string, string>;
  openingBalanceDraft?: {
    accountName: string;
    mode: "direct" | "from-date";
    amount: string;
    commodity: string;
    date: string;
    knownBalance: string;
    calculatedOpening?: string;
    error?: string;
  };
  setPriceDraft?: {
    datetime: string;
    commodity: string;
    priceAmount: string;
    quoteCurrency: string;
    error?: string;
  };
  transactionValues?: Map<string, number | null>;
  accountTotalValue?: { total: number; currency: string; bycommodity: { commodity: string; amount: number; value: number | null }[] };
  prefixQuery?: QueryResult;
  treeBaseTotals?: Map<string, number>;
  tradeLinks: TradeLink[];
  tradeSuggestions: TradeSuggestion[];
  tradeLinkSelection?: string;
  metadataReady?: boolean;
  sidebarView: "accounts" | "categories" | "reports" | "rule-editor" | "plugins";
  pluginsList?: PluginInfo[];
  pluginRunning?: string;
  pluginResult?: PluginRunResult;
  pluginLog?: string;
  pluginConfigOpen?: string;
  pluginConfigValues?: Record<string, unknown>;
  pluginSecretValues?: Record<string, unknown>;
  pluginConfigSaving?: boolean;
  pluginConfigSaved?: string;
  updatePricesOnStartup?: boolean;
  extraPrimaryAccountPrefixes?: string[];
  globalSettingsOpen?: boolean;
  globalSettingsDraft?: string[];
  dailySummary?: DailyRunSummary;
  selectedReport?: "cgt" | "income" | "balances" | "performance" | "loss_harvest";
  selectedReportYear?: number;
  reportYears?: number[];
  reportMarkdown?: string;
  cgtReport?: CgtReport;
  incomeReport?: IncomeTaxReport;
  balancesReport?: BalancesReport;
  performanceReport?: PerformanceReport;
  lossHarvestReport?: LossHarvestReport;
  /** True while loadReport is fetching/generating — drives the loading screen. */
  reportLoading?: boolean;
  taxConfig?: TaxConfig;
  taxSettingsOpen?: boolean;
  reportsBuilding?: boolean;
  reportDateMode?: "fy" | "custom";
  reportDateFrom?: string;
  reportDateTo?: string;
  reportBaseScope?: string;
  txSort?: SortState;
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
  rootDir?: string;
  knownRoots: string[];
  showVaultPicker?: boolean;
  txWindowStart: number;
  reportPageSize: number;
  navCanGoBack?: boolean;
  navCanGoForward?: boolean;
  availableReportAccounts?: { income: string[]; expenses: string[] };
  aiSuggest?: {
    status: "analyzing" | "done" | "error";
    txnId: string;
    steps: string[];
    suggestions?: AiRuleSuggestion[];
    rawOutput?: string;
    appliedIndices?: Set<number>;
    error?: string;
  };
  aiAppliedPatterns?: Set<string>;
  txExpandedGroups?: Set<string>;
  txExpandedRows?: Set<string>;
  accountConfigs?: Map<string, { explorer_url?: string }>;
  searchFilteredTransactions?: Transaction[];
  searchFilteredCount?: number;
  searchFilteredOffset?: number;
  refreshView?: () => Promise<void>;
  refreshSearch?: () => Promise<void>;
};

const savedNav = loadNavState();

const state: AppState = {
  busy: false,
  pendingMutations: 0,
  pendingDeletes: new Set<string>(),
  pendingAdds: [],
  search: savedNav?.search ?? "",
  searchPills: savedNav?.searchPills ?? [],
  searchText: savedNav?.searchText ?? "",
  dateFilter: "all",
  showHidden: false,
  drillPath: savedNav?.drillPath ?? [],
  _drillInitialized: !!savedNav?.drillPath,
  selectedCategory: savedNav?.selectedCategory,
  categoryDrillPath: savedNav?.categoryDrillPath ?? [],
  _categoryDrillInitialized: !!savedNav?.categoryDrillPath,
  pipelineWarnings: [],
  accountSets: [],
  accountSetMap: {},
  allAccounts: [],
  accountFoldersMap: {},
  accountPropertiesMap: {},
  tradeLinks: [],
  tradeSuggestions: [],
  txSort: savedNav?.txSort ?? { column: "date", direction: "asc" },
  sidebarView: savedNav?.sidebarView ?? "accounts",
  selectedAccount: savedNav?.selectedAccount,
  selectedReport: savedNav?.selectedReport,
  // Intentionally NOT restored from a prior session: reports open on the current
  // FY (loadReport defaults it). In-session back/forward still restores years.
  selectedReportYear: undefined,
  cgtSort: savedNav?.cgtSort,
  cgtFilterText: savedNav?.cgtFilterText,
  incomeSort: savedNav?.incomeSort,
  incomeFilterText: savedNav?.incomeFilterText,
  knownRoots: [],
  txWindowStart: 0,
  reportPageSize: 50,
};

state.refreshView = async () => {
  await Promise.all([loadPrefixQuery(state), runSearchFilter(state)]);
};

// Lighter refresh for mutations: just the visible search window. The
// prefix query (used for sidebar account counts) is intentionally NOT
// kicked off here — it returns ALL transactions for the selected
// account prefix, which on a large vault means a multi-second IPC
// deserialize stall. A mutation that only changes the categorisation
// of one txn doesn't need the prefix query to be live-fresh.
state.refreshSearch = async () => {
  await runSearchFilter(state);
};

const mutationQueue = new MutationQueue({ render: () => render(state) });

// Seed the history with the initial state
pushNav(state);
updateNavFlags(state);
render(state);

// Diagnostic hook for MCP/devtools: snapshot the bits of state most useful
// for debugging the optimistic-delete + morphdom flow.
(window as unknown as { __diag?: () => unknown }).__diag = () => ({
  pendingDeletes: [...state.pendingDeletes],
  pendingMutations: state.pendingMutations,
  searchFiltered: state.searchFilteredTransactions?.length ?? 0,
  selectedAccount: state.selectedAccount,
  status: state.status,
  debugLog: debugLog.slice(-30),
});

// Global keyboard shortcuts for navigation
document.addEventListener("keydown", (e) => {
  if (e.altKey && e.key === "ArrowLeft") {
    e.preventDefault();
    navigateBack(state, render);
    if (state.sidebarView === "reports" && state.selectedReport) {
      void loadReport(state).then(() => render(state));
    }
  } else if (e.altKey && e.key === "ArrowRight") {
    e.preventDefault();
    navigateForward(state, render);
    if (state.sidebarView === "reports" && state.selectedReport) {
      void loadReport(state).then(() => render(state));
    }
  }
});

async function normalStartup(): Promise<void> {
  await _normalStartup(state as any, {
    invoke,
    listen: listen as any,
    render: () => render(state),
    getItem: (k) => localStorage.getItem(k),
    nowYYYYMM,
    onPipelineEvent: (payload: any) => {
      pushDebug("info", `pipeline-rebuilt event received: output_files_written=${payload.output_files_written}, total_written=${payload.total_written}`);
      state.pipelineWarnings = payload.warnings ?? [];
      if (payload.owner_accounts) state.accountSetMap = payload.owner_accounts;
      if (payload.account_folders) state.accountFoldersMap = payload.account_folders;
      if (payload.account_properties) {
        state.accountPropertiesMap = { ...state.accountPropertiesMap, ...payload.account_properties };
      }
      if (payload.output_files_written > 0) {
        state.reportsBuilding = true;
        state.status = `Auto-rebuilt (${payload.total_written} transactions)`;
        pushDebug("info", "pipeline-rebuilt: calling loadGeneratedLedger (with sync renders)");
        loadGeneratedLedger(state);
        void loadAllAccounts(state);
      } else {
        state.status = `Rebuilt — no changes`;
        const t0 = performance.now();
        render(state);
        const dt = performance.now() - t0;
        pushDebug("info", `pipeline-rebuilt: no-op render took ${dt.toFixed(0)}ms`);
      }
    },
  });

  // Load prefix query, search filter, the global account pool, and total
  // account value after startup
  await Promise.all([loadPrefixQuery(state), runSearchFilter(state), loadAllAccounts(state)]);
  render(state);
  await Promise.all([loadAccountTotalValue(state), _loadTreeBaseCurrencyTotals(state)]);
  render(state);

  // If restored nav state was on reports, load report data
  if (state.sidebarView === "reports" && state.selectedReport) {
    void loadReport(state).then(() => render(state));
  }

  // Listen for background report generation completion
  listen("reports-rebuilt", () => {
    state.reportsBuilding = false;
    if (state.sidebarView === "reports" && state.selectedReport) {
      void loadReport(state).then(() => render(state));
    } else {
      render(state);
    }
  }).catch(() => {});

  // Listen for AI suggest progress steps
  listen<string>("ai-suggest-step", (event) => {
    if (!state.aiSuggest || state.aiSuggest.status !== "analyzing") return;
    state.aiSuggest.steps.push(event.payload);
    render(state);
  }).catch(() => {});

  // Listen for AI suggest result from background thread
  listen<AiSuggestResponse>("ai-suggest-result", (event) => {
    if (!state.aiSuggest) return; // modal was cancelled
    const response = event.payload;
    const txnId = state.aiSuggest.txnId;
    const steps = state.aiSuggest.steps; // preserve the log
    if (response.success && !response.error) {
      state.aiSuggest = { status: "done", txnId, steps, suggestions: response.suggestions, rawOutput: response.raw_output, appliedIndices: new Set() };
    } else {
      state.aiSuggest = { status: "error", txnId, steps, error: response.error ?? "Claude returned an error", rawOutput: response.raw_output };
    }
    render(state);
  }).catch(() => {});

  // Listen for AI transform progress steps
  listen<string>("ai-transform-step", (event) => {
    if (!state.transformDraft || state.transformDraft.aiStatus !== "analyzing") return;
    (state.transformDraft.aiSteps ??= []).push(event.payload);
    render(state);
  }).catch(() => {});

  // Listen for AI transform result
  listen<AiTransformResponse>("ai-transform-result", (event) => {
    if (!state.transformDraft) return;
    const response = event.payload;
    if (response.success && !response.error) {
      state.transformDraft.script = response.script;
      state.transformDraft.aiStatus = "done";
      state.transformDraft.aiRawOutput = response.raw_output;
      state.transformDraft.csvTypes = response.csv_types;
    } else {
      state.transformDraft.aiStatus = "error";
      state.transformDraft.aiError = response.error ?? "AI generation failed";
      state.transformDraft.aiRawOutput = response.raw_output;
      state.transformDraft.csvTypes = response.csv_types;
    }
    render(state);
  }).catch(() => {});

  // Listen for plugin live log lines (one per stdout/stderr line).
  // Append directly to the live <pre> to avoid a full re-render per line.
  listen<{ plugin: string; stream: string; line: string }>("plugin-log", (event) => {
    if (!state.pluginRunning) return;
    const isDailyRun = state.pluginRunning === DAILY_SYNC_SENTINEL;
    // During a daily batch, accept every plugin's lines and prefix the source;
    // otherwise only the single running plugin's lines.
    if (!isDailyRun && event.payload.plugin !== state.pluginRunning) return;
    const prefix = isDailyRun ? `[${event.payload.plugin}] ` : "";
    const tag = event.payload.stream === "stderr" ? "" : "[out] ";
    state.pluginLog = (state.pluginLog ?? "") + prefix + tag + event.payload.line + "\n";
    const el = document.getElementById("pluginLiveLog");
    if (el) {
      el.textContent = state.pluginLog;
      el.scrollTop = el.scrollHeight;
    } else {
      render(state);
    }
  }).catch(() => {});
}

// On startup, if "Update prices on startup" is enabled, backfill missing daily
// prices in the background. Idempotent: each daily plugin fetches only the gap,
// and plugins that already succeeded today are skipped, so this never blocks
// the UI and re-launches the same day do nothing.
// Run the daily price-backfill batch. `skipIfSucceededToday` is true for the
// automatic startup run (don't re-fetch what already ran today) and false for
// the manual "Update prices now" button (the user asked for it explicitly).
async function runDailySync(skipIfSucceededToday: boolean): Promise<void> {
  if (state.pluginRunning) return;
  state.pluginRunning = DAILY_SYNC_SENTINEL;
  state.pluginLog = "";
  state.dailySummary = undefined;
  state.pluginResult = undefined;
  render(state);
  try {
    state.dailySummary = await invoke<DailyRunSummary>("run_daily_plugins_cmd", {
      skipIfSucceededToday,
    });
  } catch (e) {
    console.error("Daily price sync failed:", e);
  }
  state.pluginRunning = undefined;
  await loadPlugins(state);
  render(state);
}

async function maybeRunStartupPriceSync(): Promise<void> {
  let enabled = false;
  try {
    enabled = await invoke<boolean>("get_update_prices_on_startup");
  } catch {
    return; // non-Tauri context
  }
  if (enabled) await runDailySync(true);
}

(async () => {
  // Check if a root folder is configured
  try {
    const hasRoot = await invoke<boolean>("has_root_dir");
    if (!hasRoot) {
      // Load known roots for returning users
      try {
        state.knownRoots = await invoke<string[]>("get_known_roots");
      } catch { /* ignore */ }
      state.showVaultPicker = true;
      render(state);
      return; // Don't proceed with normal startup
    }
    state.rootDir = await invoke<string | null>("get_root_dir") ?? undefined;
    state.knownRoots = await invoke<string[]>("get_known_roots");
  } catch {
    // Not available in non-Tauri contexts — proceed normally
  }
  await normalStartup();
  // Background backfill of daily prices (no-op unless the setting is enabled).
  void maybeRunStartupPriceSync();
})();
