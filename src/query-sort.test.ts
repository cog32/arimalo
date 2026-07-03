import { describe, expect, it } from "vitest";
import {
  backendSortForTransactionWindow,
  shouldClientSortLoadedTransactions,
} from "./query-sort";

describe("backendSortForTransactionWindow", () => {
  it("maps supported sorts directly to backend fields", () => {
    expect(backendSortForTransactionWindow({ column: "date", direction: "asc" })).toEqual({
      field: "date",
      order: "asc",
    });
    expect(backendSortForTransactionWindow({ column: "amount", direction: "desc" })).toEqual({
      field: "amount",
      order: "desc",
    });
    expect(backendSortForTransactionWindow({ column: "party", direction: "asc" })).toEqual({
      field: "payee",
      order: "asc",
    });
  });

  it("falls back to date desc for unsupported sorts", () => {
    expect(backendSortForTransactionWindow({ column: "notes", direction: "asc" })).toEqual({
      field: "date",
      order: "desc",
    });
    expect(backendSortForTransactionWindow({ column: "category", direction: "asc" })).toEqual({
      field: "date",
      order: "desc",
    });
  });

  it("defaults to date desc when no sort is active", () => {
    expect(backendSortForTransactionWindow()).toEqual({
      field: "date",
      order: "desc",
    });
  });
});

describe("shouldClientSortLoadedTransactions", () => {
  it("only uses client-side sorting for unsupported loaded-window columns", () => {
    expect(shouldClientSortLoadedTransactions({ column: "notes", direction: "asc" })).toBe(true);
    expect(shouldClientSortLoadedTransactions({ column: "category", direction: "desc" })).toBe(true);
    expect(shouldClientSortLoadedTransactions({ column: "date", direction: "asc" })).toBe(false);
    expect(shouldClientSortLoadedTransactions({ column: "party", direction: "asc" })).toBe(false);
    expect(shouldClientSortLoadedTransactions({ column: "amount", direction: "desc" })).toBe(false);
  });
});
