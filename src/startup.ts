/**
 * Startup orchestration — extracted for testability.
 *
 * The key invariant is that independent backend calls run in parallel.
 * The test suite verifies this by counting sequential "invoke rounds":
 *
 *   Round 1: list_account_sets
 *   Round 2: rebuild_pipeline ‖ get_display_config ‖ init_metadata
 *            ‖ get_relay_config ‖ get_account_gaps
 *   Round 3: get_pipeline_warnings ‖ convert_to_base_currency
 *            ‖ list_devices ‖ get_sync_log ‖ get_trade_links
 *            ‖ suggest_trade_links_cmd ‖ get_source_folder_path×N
 */

import { postingPriceValue } from "./posting-value";
import type { Posting } from "./types";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface StartupDeps {
  invoke: <T = unknown>(cmd: string, args?: Record<string, any>) => Promise<T>;
  listen: <T>(event: string, handler: (e: { payload: T }) => void) => Promise<unknown>;
  render: () => void;
  getItem: (key: string) => string | null;
  nowYYYYMM: () => string;
  onPipelineEvent: (payload: any) => void;
}

export interface StartupState {
  accountSets: string[];
  selectedAccountSet?: string;
  displayConfig?: { base_currency?: string; default_account?: string; commodities?: Record<string, any>; default_decimals?: number };
  busy: boolean;
  status?: string;
  parse?: {
    ok: boolean;
    transactions: Array<{
      date: string;
      datetime: string;
      meta?: string | null;
      amount: number;
      narration?: string | null;
      postings: Array<{ account: string; commodity: string; amount: number }>;
    }>;
    balances: Array<{ account: string; totals: Array<{ commodity: string; amount: number }> }>;
    accounts_with_opening: string[];
    account_properties: Record<string, any>;
    diagnostics: any[];
  };
  selectedAccount?: string;
  drillPath: string[];
  pipelineWarnings: string[];
  accountSetMap: Record<string, string[]>;
  accountFoldersMap: Record<string, string>;
  accountPropertiesMap: Record<string, any>;
  transactionValues?: Map<string, number | null>;
  metadataReady?: boolean;
  devices?: any[];
  syncLog?: any[];
  tradeLinks: any[];
  tradeSuggestions: any[];
  relayConfig?: any;
  accountGaps?: any[];
  sourceFolderPaths?: Record<string, string>;
  reportsBuilding?: boolean;
}

// ---------------------------------------------------------------------------
// Main startup function
// ---------------------------------------------------------------------------

export async function normalStartup(state: StartupState, deps: StartupDeps): Promise<void> {
  // ── Round 1 ── list_account_sets (everything depends on selectedAccountSet)
  try {
    const sets = await deps.invoke<string[]>("list_account_sets");
    state.accountSets = sets;
    if (sets.length > 0 && !state.selectedAccountSet) {
      const saved = deps.getItem("arimalo_selectedAccountSet");
      state.selectedAccountSet = saved && sets.includes(saved) ? saved : sets[0];
    }
  } catch { /* not available in non-Tauri contexts */ }

  const accountSet = state.selectedAccountSet ?? "";

  // ── Round 2 ── independent operations, all started concurrently
  const pipelineP = (async () => {
    state.busy = true;
    state.status = "Rebuilding...";
    deps.render();
    try {
      const response = await deps.invoke<any>("rebuild_pipeline", {
        nowYyyymm: deps.nowYYYYMM(),
        accountSet,
      });
      applyPipelineResponse(state, response, deps.getItem);
      if (response.result.output_files_written > 0) {
        state.reportsBuilding = true;
      }
      const n = response.parse.transactions.length;
      state.status = `Parsed (${n} transactions) — transformed ${response.result.csv_transformed}, cached ${response.result.csv_cached}, manual ${response.result.manual_count}`;
    } catch (err) {
      state.status = `Error: ${String(err)}`;
    } finally {
      state.busy = false;
      deps.render();
    }
  })();

  const displayConfigP = (async () => {
    try {
      state.displayConfig = await deps.invoke<any>("get_display_config", { accountSet });
    } catch { /* fallback to defaults */ }
  })();

  const metadataP = (async () => {
    try {
      await deps.invoke("init_metadata");
      state.metadataReady = true;
    } catch { /* metadata not available */ }
  })();

  const relayP = (async () => {
    try {
      const c = await deps.invoke<any>("get_relay_config");
      if (c) state.relayConfig = c;
    } catch { /* relay not available */ }
  })();

  const gapsP = (async () => {
    try {
      state.accountGaps = await deps.invoke<any[]>("get_account_gaps");
    } catch { /* gaps not available */ }
  })();

  // listen registration is fast (just sets up a callback)
  deps.listen("pipeline-rebuilt", (event: any) => {
    deps.onPipelineEvent(event.payload);
  }).catch(() => {});

  await Promise.allSettled([pipelineP, displayConfigP, metadataP, relayP, gapsP]);
  deps.render();

  // ── Round 3 ── operations that depend on round 2 results
  const baseCurrency = state.displayConfig?.base_currency;

  const warningsP = (async () => {
    try {
      const w = await deps.invoke<string[]>("get_pipeline_warnings");
      if (w.length > 0) state.pipelineWarnings = w;
    } catch { /* warnings not available */ }
  })();

  const txnValuesP = loadTransactionValues(state, deps, baseCurrency);

  const metadataSubP = (async () => {
    await metadataP; // ensure metadata is initialized first
    if (!state.metadataReady) return;
    try {
      const [devices, syncLog, links] = await Promise.all([
        deps.invoke<any[]>("list_devices").catch(() => []),
        deps.invoke<any[]>("get_sync_log").catch(() => []),
        deps.invoke<any[]>("get_trade_links").catch(() => []),
      ]);
      state.devices = devices;
      state.syncLog = syncLog;
      state.tradeLinks = links;
    } catch { /* metadata sub-calls failed */ }
  })();

  const suggestionsP = (async () => {
    await metadataP; // ensure metadata is initialized before suggesting trades
    if (!state.metadataReady) return;
    try {
      state.tradeSuggestions = await deps.invoke<any[]>("suggest_trade_links_cmd", {
        accountSet,
        baseCurrency: baseCurrency ?? null,
      });
    } catch { /* suggestions not available */ }
  })();

  const foldersP = (async () => {
    const uniqueFolders = [...new Set(Object.values(state.accountFoldersMap))];
    if (uniqueFolders.length === 0) return;
    const paths: Record<string, string> = {};
    await Promise.all(uniqueFolders.map(async (folder) => {
      try {
        paths[folder] = await deps.invoke<string>("get_source_folder_path", { folderName: folder });
      } catch { /* folder not found */ }
    }));
    state.sourceFolderPaths = paths;
  })();

  const issuesP = (async () => {
    try {
      await (await import("./issues")).refreshIssuesCache();
    } catch { /* collect_issues_cmd unavailable */ }
  })();

  await Promise.allSettled([warningsP, txnValuesP, metadataSubP, suggestionsP, foldersP, issuesP]);
  deps.render();
}

// ---------------------------------------------------------------------------
// State helpers (duplicated from main.ts to keep startup self-contained)
// ---------------------------------------------------------------------------

function applyPipelineResponse(
  state: StartupState,
  response: any,
  getItem: (k: string) => string | null,
): void {
  applyParse(state, response.parse, getItem);
  state.pipelineWarnings = response.warnings ?? [];
  if (response.result.owner_accounts) state.accountSetMap = response.result.owner_accounts;
  if (response.result.account_folders) state.accountFoldersMap = response.result.account_folders;
  if (response.result.account_properties) {
    state.accountPropertiesMap = { ...state.accountPropertiesMap, ...response.result.account_properties };
  }
}

function applyParse(
  state: StartupState,
  response: any,
  getItem: (k: string) => string | null,
): void {
  state.parse = response;
  if (response.account_properties) {
    state.accountPropertiesMap = { ...state.accountPropertiesMap, ...response.account_properties };
  }
  const nextBalances: any[] = response.balances ?? [];
  if (!state.selectedAccount) {
    const matchesPfx = (acct: string, pfx: string) => acct === pfx || acct.startsWith(pfx + ":");
    const defaultAccount = state.displayConfig?.default_account;
    state.selectedAccount =
      (defaultAccount && nextBalances.find((b: any) => matchesPfx(b.account, defaultAccount))?.account)
        ?? nextBalances[0]?.account;
  }
}

async function loadTransactionValues(
  state: StartupState,
  deps: Pick<StartupDeps, "invoke">,
  baseCurrency: string | undefined,
): Promise<void> {
  if (!baseCurrency || !state.parse || !state.selectedAccount) {
    state.transactionValues = undefined;
    return;
  }
  const matchPfx = (acct: string, pfx: string) => acct === pfx || acct.startsWith(pfx + ":");
  const account = state.selectedAccount;
  const txns = state.parse.transactions.filter(
    (t) => t.postings.some((p) => matchPfx(p.account, account)),
  );
  const samplePosting = txns[0]?.postings.find((p) => matchPfx(p.account, account));
  if (samplePosting && samplePosting.commodity === baseCurrency) {
    state.transactionValues = undefined;
    return;
  }

  const requests = txns.map((t) => {
    const posting = t.postings.find((p) => matchPfx(p.account, account));
    return {
      commodity: posting?.commodity ?? "",
      amount: Math.abs(posting?.amount ?? 0),
      datetime: t.datetime,
    };
  });

  try {
    const result = await deps.invoke<{ values: (number | null)[] }>("convert_to_base_currency", {
      baseCurrency,
      requests,
    });
    const valMap = new Map<string, number | null>();
    result.values.forEach((v, i) => {
      const txn = txns[i];
      const txnId = (txn.meta ?? "").split(",").map((p: string) => p.trim()).find((p: string) => p.startsWith("txn:")) ?? "";
      const key = `${txnId}|${txn.datetime}|${txn.amount}|${txn.narration ?? ""}`;
      const posting = txn.postings.find((p) => matchPfx(p.account, account));
      const sign = posting && posting.amount < 0 ? -1 : 1;
      const priceVal = postingPriceValue(posting as Posting | undefined);
      const price = (posting as any)?.price as { commodity: string } | undefined;
      if (priceVal !== null && price?.commodity === baseCurrency) {
        valMap.set(key, sign * priceVal);
      } else if (v !== null) {
        valMap.set(key, sign * v);
      } else if (priceVal !== null) {
        valMap.set(key, sign * priceVal);
      } else {
        valMap.set(key, null);
      }
    });
    // For trade-linked transactions, use the partner's counterparty value
    if ((state as any).tradeLinks?.length && state.parse) {
      const linkMap = new Map<string, string>();
      for (const link of (state as any).tradeLinks) {
        linkMap.set(link.txn_id_a, link.txn_id_b);
        linkMap.set(link.txn_id_b, link.txn_id_a);
      }
      const allTxns = state.parse.transactions;
      const partnerReqs: { txnIdx: number; commodity: string; amount: number; datetime: string }[] = [];
      txns.forEach((t, i) => {
        const tid = (t.meta ?? "").split(",").map((p: string) => p.trim()).find((p: string) => p.startsWith("txn:")) ?? "";
        if (!tid || !linkMap.has(tid)) return;
        const partnerId = linkMap.get(tid)!;
        const partner = allTxns.find((pt) => {
          if (pt === t) return false;
          const pid = (pt.meta ?? "").split(",").map((p: string) => p.trim()).find((p: string) => p.startsWith("txn:")) ?? "";
          const ptComm = pt.postings.find((p) => matchPfx(p.account, account))?.commodity;
          const tComm = t.postings.find((p) => matchPfx(p.account, account))?.commodity;
          return pid === partnerId && ptComm !== tComm;
        });
        if (!partner) return;
        const pp = partner.postings.find((p) => matchPfx(p.account, account));
        if (!pp || pp.commodity === t.postings.find((p) => matchPfx(p.account, account))?.commodity) return;
        partnerReqs.push({ txnIdx: i, commodity: pp.commodity, amount: Math.abs(pp.amount), datetime: partner.datetime });
      });
      if (partnerReqs.length > 0) {
        const pr = await deps.invoke<{ values: (number | null)[] }>("convert_to_base_currency", {
          baseCurrency,
          requests: partnerReqs.map((r) => ({ commodity: r.commodity, amount: r.amount, datetime: r.datetime })),
        });
        pr.values.forEach((pv, j) => {
          if (pv !== null) {
            const idx = partnerReqs[j].txnIdx;
            const t2 = txns[idx];
            const tid2 = (t2.meta ?? "").split(",").map((p: string) => p.trim()).find((p: string) => p.startsWith("txn:")) ?? "";
            const k = `${tid2}|${t2.datetime}|${t2.amount}|${t2.narration ?? ""}`;
            const p2 = t2.postings.find((p) => matchPfx(p.account, account));
            const s = p2 && p2.amount < 0 ? -1 : 1;
            valMap.set(k, s * pv);
          }
        });
      }
    }

    state.transactionValues = valMap;
  } catch {
    state.transactionValues = undefined;
  }
}
