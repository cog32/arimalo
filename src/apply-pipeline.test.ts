import { describe, it, expect, vi } from "vitest";
import { applyPipelineResponse, applyMutationResponse, type ApplyTarget } from "./apply-pipeline";
import type { PipelineResponse } from "./types";

function makeState(extra: Partial<ApplyTarget> = {}): ApplyTarget {
  return {
    pipelineWarnings: [],
    accountSetMap: {},
    accountFoldersMap: {},
    accountPropertiesMap: {},
    ...extra,
  };
}

function makeResponse(): PipelineResponse {
  return {
    result: {
      owner_accounts: {},
      account_folders: {},
      account_properties: {},
      output_files_written: 1,
      total_written: 3,
    } as any,
    parse: {
      ok: true,
      diagnostics: [],
      transactions: [],
      balances: [],
      accounts_with_opening: [],
      account_properties: {},
    },
    warnings: [],
  };
}

describe("applyPipelineResponse", () => {
  it("invokes state.refreshView so the search view reloads after a mutation", async () => {
    const refreshView = vi.fn().mockResolvedValue(undefined);
    const state = makeState({ refreshView });

    await applyPipelineResponse(state, makeResponse());

    expect(refreshView).toHaveBeenCalledTimes(1);
  });

  it("refreshes AFTER applying parse state (so the refresh can read fresh state)", async () => {
    const order: string[] = [];
    const state = makeState({
      refreshView: async () => {
        order.push(`refresh:parseOk=${state.parse?.ok}`);
      },
    });

    await applyPipelineResponse(state, makeResponse());

    expect(order).toEqual(["refresh:parseOk=true"]);
  });

  it("does not throw when refreshView is unset", async () => {
    const state = makeState();
    await expect(applyPipelineResponse(state, makeResponse())).resolves.toBeUndefined();
  });

  it("populates justAddedTxnIds with txns present after but not before the rebuild", async () => {
    const state = makeState({
      parse: {
        ok: true,
        diagnostics: [],
        transactions: [
          { meta: "txn:keep, payee:Foo", date: "", datetime: "", amount: 0, narration: null, postings: [] },
        ],
        balances: [],
        accounts_with_opening: [],
        account_properties: {},
      } as any,
    });

    const resp = makeResponse();
    resp.parse.transactions = [
      { meta: "txn:keep, payee:Foo", date: "", datetime: "", amount: 0, narration: null, postings: [] },
      { meta: "txn:new, payee:Bar", date: "", datetime: "", amount: 0, narration: null, postings: [] },
    ] as any;

    await applyPipelineResponse(state, resp);

    expect(state.justAddedTxnIds).toBeInstanceOf(Set);
    expect(state.justAddedTxnIds?.has("txn:new")).toBe(true);
    expect(state.justAddedTxnIds?.has("txn:keep")).toBe(false);
  });

  it("leaves justAddedTxnIds undefined when no new ids arrived", async () => {
    const state = makeState({
      parse: {
        ok: true,
        diagnostics: [],
        transactions: [
          { meta: "txn:a", date: "", datetime: "", amount: 0, narration: null, postings: [] },
        ],
        balances: [],
        accounts_with_opening: [],
        account_properties: {},
      } as any,
    });

    const resp = makeResponse();
    // Pipeline returned the same set of txns (e.g. a no-op rebuild)
    resp.parse.transactions = [
      { meta: "txn:a", date: "", datetime: "", amount: 0, narration: null, postings: [] },
    ] as any;

    await applyPipelineResponse(state, resp);

    expect(state.justAddedTxnIds).toBeUndefined();
  });

  it("resets paged scroll so refreshView re-fetches from offset 0", async () => {
    // Regression: after a mutation (save_label / save_rule) the search view
    // must re-fetch from the top, otherwise runSearchFilter appends a fresh
    // page to the stale earlier pages and the rows on screen keep their
    // pre-mutation values.
    let seenWindowStart: number | undefined;
    let seenFiltered: unknown;
    const state = makeState({
      txWindowStart: 500,
      searchFilteredTransactions: [{ id: "stale" } as never],
      refreshView: async () => {
        seenWindowStart = state.txWindowStart;
        seenFiltered = state.searchFilteredTransactions;
      },
    });

    await applyPipelineResponse(state, makeResponse());

    expect(seenWindowStart).toBe(0);
    expect(seenFiltered).toBeUndefined();
  });
});

describe("applyMutationResponse", () => {
  it("refreshes the account balance + window via refreshView, not just the window", async () => {
    // Perf regression guard: a slim mutation must use refreshView (which now runs
    // the cheap balances-only prefix query + the paged window), NOT the window-only
    // refreshSearch. Otherwise the account header balance goes stale after an add
    // until the next navigation. (The prefix query was made balances-only / cheap,
    // so there is no longer a reason to skip it on mutations.)
    const refreshView = vi.fn().mockResolvedValue(undefined);
    const refreshSearch = vi.fn().mockResolvedValue(undefined);
    const state = makeState({ refreshView, refreshSearch });

    await applyMutationResponse(state, { ok: true, warnings: [], output_files_written: 1 });

    expect(refreshView).toHaveBeenCalledTimes(1);
    expect(refreshSearch).not.toHaveBeenCalled();
  });

  it("resets the paged window so the refresh re-fetches from offset 0", async () => {
    let seenWindowStart: number | undefined;
    const state = makeState({
      txWindowStart: 500,
      searchFilteredTransactions: [{ id: "stale" } as never],
      refreshView: async () => {
        seenWindowStart = state.txWindowStart;
      },
    });

    await applyMutationResponse(state, { ok: true, warnings: [], output_files_written: 0 });

    expect(seenWindowStart).toBe(0);
  });

  it("does not throw when refreshView is unset", async () => {
    const state = makeState();
    await expect(
      applyMutationResponse(state, { ok: true, warnings: [], output_files_written: 0 }),
    ).resolves.toBeUndefined();
  });
});
