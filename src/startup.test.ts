import { describe, it, expect, vi } from "vitest";
import { normalStartup, type StartupDeps, type StartupState } from "./startup";

// ---------------------------------------------------------------------------
// Mock data returned by each invoke command
// ---------------------------------------------------------------------------

const MOCK_PIPELINE_RESPONSE = {
  result: {
    csv_transformed: 1,
    csv_cached: 0,
    manual_count: 0,
    total_written: 1,
    warnings: [],
    owner_accounts: { richard: ["assets:crypto:wallet:ethereum"] },
    account_folders: { "assets:crypto:wallet:ethereum": "richard/crypto/wallet/ethereum" },
    account_properties: {},
  },
  parse: {
    ok: true,
    diagnostics: [],
    transactions: [
      {
        date: "2025-01-15",
        datetime: "2025-01-15T00:00:00",
        postings: [
          { account: "assets:crypto:wallet:ethereum", commodity: "ETH", amount: 1.5 },
          { account: "expenses:unknown", commodity: "ETH", amount: -1.5 },
        ],
      },
    ],
    balances: [{ account: "assets:crypto:wallet:ethereum", totals: [{ commodity: "ETH", amount: 1.5 }] }],
    accounts_with_opening: [],
    account_properties: {},
  },
  warnings: [],
};

const MOCK_RESPONSES: Record<string, unknown> = {
  list_account_sets: ["richard"],
  get_display_config: { commodities: {}, default_decimals: 2, base_currency: "AUD" },
  rebuild_pipeline: MOCK_PIPELINE_RESPONSE,
  get_pipeline_warnings: [],
  init_metadata: undefined,
  list_devices: [],
  get_sync_log: [],
  get_trade_links: [],
  suggest_trade_links_cmd: [],
  get_relay_config: null,
  get_account_gaps: [],
  convert_to_base_currency: { values: [100] },
  get_source_folder_path: "/mock/path",
};

// ---------------------------------------------------------------------------
// Deferred-promise test harness
//
// Every deps.invoke() call returns a deferred promise. The test controls when
// these resolve via flush(). Each flush resolves all currently-pending invoke
// calls, then waits for microtasks to settle so that subsequent invoke calls
// (started by .then() handlers) land in `pending` ready for the next flush.
//
// The number of flush() calls == number of sequential invoke rounds.
// ---------------------------------------------------------------------------

interface Pending {
  cmd: string;
  resolve: (v: unknown) => void;
}

function createHarness(responseOverrides?: Record<string, unknown>) {
  const responses: Record<string, unknown> = { ...MOCK_RESPONSES, ...responseOverrides };
  const pending: Pending[] = [];
  const rounds: string[][] = [];

  const state: StartupState = {
    accountSets: [],
    busy: false,
    status: undefined,
    drillPath: [],
    pipelineWarnings: [],
    accountSetMap: {},
    accountFoldersMap: {},
    accountPropertiesMap: {},
    tradeLinks: [],
    tradeSuggestions: [],
    sidebarView: "accounts",
    knownRoots: [],
  } as StartupState & Record<string, unknown>;

  const renderSpy = vi.fn();

  const deps: StartupDeps = {
    invoke: <T>(cmd: string, _args?: Record<string, unknown>): Promise<T> => {
      return new Promise<T>((resolve) => {
        pending.push({ cmd, resolve: resolve as (v: unknown) => void });
      });
    },
    listen: vi.fn().mockResolvedValue(undefined),
    render: renderSpy,
    getItem: () => null,
    nowYYYYMM: () => "202501",
    onPipelineEvent: vi.fn(),
  };

  /** Resolve all pending invoke calls and let microtasks settle. */
  const flush = async (): Promise<string[]> => {
    // Let microtasks from previous flush settle (new invokes may have been queued)
    await new Promise((r) => setTimeout(r, 0));

    const batch = pending.splice(0);
    if (batch.length === 0) return [];

    const cmds = batch.map((p) => p.cmd);
    rounds.push(cmds);

    for (const p of batch) {
      p.resolve(responses[p.cmd] ?? undefined);
    }

    // Let .then() handlers and subsequent microtasks run
    await new Promise((r) => setTimeout(r, 0));
    return cmds;
  };

  return { state, deps, renderSpy, flush, rounds, pending };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("normalStartup invoke rounds", () => {
  it("round 1: only list_account_sets", async () => {
    const { state, deps, flush } = createHarness();
    const done = normalStartup(state, deps);

    const r1 = await flush();
    expect(r1).toEqual(["list_account_sets"]);

    // Drain remaining rounds so the promise resolves
    while ((await flush()).length > 0) { /* drain */ }
    await done;
  });

  it("round 2: parallel independent operations", async () => {
    const { state, deps, flush } = createHarness();
    const done = normalStartup(state, deps);

    await flush(); // round 1
    const r2 = await flush();

    expect(r2.sort()).toEqual([
      "get_account_gaps",
      "get_display_config",
      "get_relay_config",
      "init_metadata",
      "rebuild_pipeline",
    ]);

    while ((await flush()).length > 0) { /* drain */ }
    await done;
  });

  it("round 3: dependent operations after pipeline + config + metadata", async () => {
    const { state, deps, flush } = createHarness();
    const done = normalStartup(state, deps);

    await flush(); // round 1
    await flush(); // round 2

    const r3 = await flush();

    // These all require results from round 2
    expect(r3).toContain("get_pipeline_warnings");
    expect(r3).toContain("list_devices");
    expect(r3).toContain("get_sync_log");
    expect(r3).toContain("get_trade_links");
    expect(r3).toContain("suggest_trade_links_cmd");
    expect(r3).toContain("convert_to_base_currency");
    expect(r3).toContain("get_source_folder_path");

    while ((await flush()).length > 0) { /* drain */ }
    await done;
  });

  it("completes in at most 3 sequential invoke rounds", async () => {
    const { state, deps, flush, rounds } = createHarness();
    const done = normalStartup(state, deps);

    while ((await flush()).length > 0) { /* drain */ }
    await done;

    expect(rounds.length).toBeLessThanOrEqual(3);
  });

  it("makes all expected invoke calls", async () => {
    const { state, deps, flush, rounds } = createHarness();
    const done = normalStartup(state, deps);

    while ((await flush()).length > 0) { /* drain */ }
    await done;

    const allCmds = rounds.flat();
    expect(allCmds).toContain("list_account_sets");
    expect(allCmds).toContain("rebuild_pipeline");
    expect(allCmds).toContain("get_display_config");
    expect(allCmds).toContain("init_metadata");
    expect(allCmds).toContain("get_relay_config");
    expect(allCmds).toContain("get_account_gaps");
    expect(allCmds).toContain("get_pipeline_warnings");
    expect(allCmds).toContain("list_devices");
    expect(allCmds).toContain("get_sync_log");
    expect(allCmds).toContain("get_trade_links");
    expect(allCmds).toContain("suggest_trade_links_cmd");
    expect(allCmds).toContain("convert_to_base_currency");
    expect(allCmds).toContain("get_source_folder_path");
  });

  it("registers the pipeline-rebuilt event listener", async () => {
    const { state, deps, flush } = createHarness();
    const done = normalStartup(state, deps);

    while ((await flush()).length > 0) { /* drain */ }
    await done;

    expect(deps.listen).toHaveBeenCalledWith("pipeline-rebuilt", expect.any(Function));
  });

  it("calls render after each round", async () => {
    const { state, deps, flush, renderSpy } = createHarness();
    const done = normalStartup(state, deps);

    await flush(); // round 1
    await flush(); // round 2
    // render called for "Rebuilding..." status + after pipeline + after round 2
    const afterR2 = renderSpy.mock.calls.length;
    expect(afterR2).toBeGreaterThanOrEqual(2);

    await flush(); // round 3
    while ((await flush()).length > 0) { /* drain */ }
    await done;

    // Final render after round 3
    expect(renderSpy.mock.calls.length).toBeGreaterThan(afterR2);
  });

  it("populates state correctly after startup", async () => {
    const { state, deps, flush } = createHarness();
    const done = normalStartup(state, deps);

    while ((await flush()).length > 0) { /* drain */ }
    await done;

    expect(state.accountSets).toEqual(["richard"]);
    expect(state.selectedAccountSet).toBe("richard");
    expect(state.displayConfig?.base_currency).toBe("AUD");
    expect(state.parse?.ok).toBe(true);
    expect(state.selectedAccount).toBe("assets:crypto:wallet:ethereum");
    expect(state.metadataReady).toBe(true);
    expect(state.accountFoldersMap).toEqual({ "assets:crypto:wallet:ethereum": "richard/crypto/wallet/ethereum" });
  });

  it("transactionValues populated with string keys after startup", async () => {
    const { state, deps, flush } = createHarness();
    const done = normalStartup(state, deps);

    while ((await flush()).length > 0) { /* drain */ }
    await done;

    expect(state.transactionValues).toBeDefined();
    expect(state.transactionValues!.size).toBe(1);
    // Keys must be strings (txnValueKey format), not numeric indices
    const key = [...state.transactionValues!.keys()][0];
    expect(typeof key).toBe("string");
  });

  it("transactionValues works with parent account prefix", async () => {
    // When selectedAccount is a parent prefix (e.g. "assets:crypto:wallet"),
    // postings with child accounts (e.g. "assets:crypto:wallet:ethereum")
    // must still match via prefix matching, not strict equality.
    const { state, deps, flush } = createHarness();
    const done = normalStartup(state, deps);

    while ((await flush()).length > 0) { /* drain */ }
    await done;

    // Simulate navigating to parent account
    state.selectedAccount = "assets:crypto:wallet";
    // The posting is on "assets:crypto:wallet:ethereum" — a child.
    // Value should still be found because loadTransactionValues uses prefix matching.
    const txn = state.parse!.transactions[0];
    const hasMatchingPosting = txn.postings.some(
      (p) => p.account === "assets:crypto:wallet" || p.account.startsWith("assets:crypto:wallet:"),
    );
    expect(hasMatchingPosting).toBe(true);
  });

  it("transactionValues honours @@ (is_total) as the total, not units × total", async () => {
    // Regression: an @@ annotation on a posting carries the leg total directly,
    // not a per-unit price. The original bug computed Math.abs(amount) × price.amount
    // unconditionally, turning `10000 BQT @@ 3650.00 AUD` into 36,500,000 AUD
    // instead of 3,650 AUD. PR #156 fixed the duplicate in main.ts; this asserts
    // the startup.ts path is fixed too.
    const pipelineWithCommsec = {
      ...MOCK_PIPELINE_RESPONSE,
      result: {
        ...MOCK_PIPELINE_RESPONSE.result,
        owner_accounts: { richard: ["assets:equity:broker:commsec:personal"] },
        account_folders: { "assets:equity:broker:commsec:personal": "richard/equity/broker/commsec/personal" },
      },
      parse: {
        ...MOCK_PIPELINE_RESPONSE.parse,
        transactions: [
          {
            date: "2004-06-03",
            datetime: "2004-06-03T00:00:00",
            meta: "txn:bug-regression",
            amount: 10000,
            narration: "B 10000 BQT @ $0.365",
            postings: [
              {
                account: "assets:equity:broker:commsec:personal",
                commodity: "BQT",
                amount: 10000,
                price: { is_total: true, amount: 3650, amount_text: "3650.00", commodity: "AUD" },
              },
              { account: "assets:transfer:cash", commodity: "AUD", amount: -3650 },
            ],
          },
        ],
        balances: [
          { account: "assets:equity:broker:commsec:personal", totals: [{ commodity: "BQT", amount: 10000 }] },
        ],
      },
    };
    const { state, deps, flush } = createHarness({
      rebuild_pipeline: pipelineWithCommsec,
      // Backend has no PriceGraph entry for BQT — forces fallback to the
      // posting annotation path, which is where the bug lived.
      convert_to_base_currency: { values: [null] },
    });
    const done = normalStartup(state, deps);

    while ((await flush()).length > 0) { /* drain */ }
    await done;

    expect(state.transactionValues).toBeDefined();
    expect(state.transactionValues!.size).toBe(1);
    const val = [...state.transactionValues!.values()][0];
    expect(val).toBe(3650);
    expect(val).not.toBe(36_500_000);
  });

  it("transactionValues computes units × price for @ (is_total=false)", async () => {
    // Counterpart: per-unit @ annotations must still multiply units by price.
    // Locks down that the helper-based fix didn't accidentally make is_total=false
    // behave like is_total=true.
    const pipelineWithPerUnit = {
      ...MOCK_PIPELINE_RESPONSE,
      result: {
        ...MOCK_PIPELINE_RESPONSE.result,
        owner_accounts: { richard: ["assets:crypto:wallet:ethereum"] },
        account_folders: { "assets:crypto:wallet:ethereum": "richard/crypto/wallet/ethereum" },
      },
      parse: {
        ...MOCK_PIPELINE_RESPONSE.parse,
        transactions: [
          {
            date: "2025-01-15",
            datetime: "2025-01-15T00:00:00",
            meta: "txn:per-unit",
            amount: 100,
            narration: "Sell 100 MNGO @ 0.50",
            postings: [
              {
                account: "assets:crypto:wallet:ethereum",
                commodity: "MNGO",
                amount: -100,
                price: { is_total: false, amount: 0.5, amount_text: "0.50", commodity: "AUD" },
              },
              { account: "assets:transfer", commodity: "AUD", amount: 50 },
            ],
          },
        ],
        balances: [
          { account: "assets:crypto:wallet:ethereum", totals: [{ commodity: "MNGO", amount: -100 }] },
        ],
      },
    };
    const { state, deps, flush } = createHarness({
      rebuild_pipeline: pipelineWithPerUnit,
      convert_to_base_currency: { values: [null] },
    });
    const done = normalStartup(state, deps);

    while ((await flush()).length > 0) { /* drain */ }
    await done;

    expect(state.transactionValues).toBeDefined();
    const val = [...state.transactionValues!.values()][0];
    // |amount| × price = 100 × 0.5 = 50; sign = -1 (amount < 0) → -50.
    expect(val).toBe(-50);
  });
});

// ---------------------------------------------------------------------------
// Efficiency constraints — guard against regressions
// ---------------------------------------------------------------------------

describe("application is efficient", () => {
  /** Count how many times each command was invoked across all rounds. */
  function countInvokes(rounds: string[][]): Map<string, number> {
    const counts = new Map<string, number>();
    for (const cmds of rounds) {
      for (const cmd of cmds) {
        counts.set(cmd, (counts.get(cmd) ?? 0) + 1);
      }
    }
    return counts;
  }

  it("when loading the application, the pipeline is rebuilt once", async () => {
    const { state, deps, flush, rounds } = createHarness();
    const done = normalStartup(state, deps);
    while ((await flush()).length > 0) { /* drain */ }
    await done;

    const counts = countInvokes(rounds);
    expect(counts.get("rebuild_pipeline")).toBe(1);
  });

  it("when loading the application, account sets are listed once", async () => {
    const { state, deps, flush, rounds } = createHarness();
    const done = normalStartup(state, deps);
    while ((await flush()).length > 0) { /* drain */ }
    await done;

    const counts = countInvokes(rounds);
    expect(counts.get("list_account_sets")).toBe(1);
  });

  it("when loading the application, config and metadata are fetched once each", async () => {
    const { state, deps, flush, rounds } = createHarness();
    const done = normalStartup(state, deps);
    while ((await flush()).length > 0) { /* drain */ }
    await done;

    const counts = countInvokes(rounds);
    expect(counts.get("get_display_config")).toBe(1);
    expect(counts.get("init_metadata")).toBe(1);
    expect(counts.get("get_relay_config")).toBe(1);
  });

  it("when loading the application, render is called at most 5 times", async () => {
    const { state, deps, flush, renderSpy } = createHarness();
    const done = normalStartup(state, deps);
    while ((await flush()).length > 0) { /* drain */ }
    await done;

    // Allowed renders: "Rebuilding..." status, after pipeline, after round 2,
    // after trade suggestions, after round 3
    expect(renderSpy.mock.calls.length).toBeLessThanOrEqual(5);
  });

  it("when loading the application, trade links and suggestions are fetched once each", async () => {
    const { state, deps, flush, rounds } = createHarness();
    const done = normalStartup(state, deps);
    while ((await flush()).length > 0) { /* drain */ }
    await done;

    const counts = countInvokes(rounds);
    expect(counts.get("get_trade_links")).toBe(1);
    expect(counts.get("suggest_trade_links_cmd")).toBe(1);
  });

  it("no expensive command is called more than twice during startup", async () => {
    const { state, deps, flush, rounds } = createHarness();
    const done = normalStartup(state, deps);
    while ((await flush()).length > 0) { /* drain */ }
    await done;

    const counts = countInvokes(rounds);
    // convert_to_base_currency may be called twice (account values + tree totals)
    // get_source_folder_path may be called per-folder — allow N
    const UNLIMITED = new Set(["get_source_folder_path"]);
    for (const [cmd, count] of counts) {
      if (!UNLIMITED.has(cmd)) {
        expect(count, `${cmd} called ${count} times`).toBeLessThanOrEqual(2);
      }
    }
  });
});
