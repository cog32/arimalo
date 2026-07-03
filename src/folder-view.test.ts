import { describe, expect, it } from "vitest";
import {
  folderDisplayName,
  folderHeaderTotal,
  folderTotalAud,
  formatFolderAmount,
  isFolderSelection,
  ROOT_FOLDER_PATH,
  syntheticFolderBalance,
} from "./folder-view";
import type { AccountBalance } from "./types";

const tree = [
  {
    name: "assets",
    fullPath: "assets",
    totals: new Map(),
    isLeaf: false,
    children: [
      {
        name: "cash",
        fullPath: "assets:cash",
        totals: new Map(),
        isLeaf: false,
        children: [
          { name: "cba", fullPath: "assets:cash:cba", totals: new Map([["AUD", -337646.16]]), isLeaf: true, children: [] },
        ],
      },
      {
        name: "crypto",
        fullPath: "assets:crypto",
        totals: new Map(),
        isLeaf: false,
        children: [
          { name: "btc", fullPath: "assets:crypto:btc", totals: new Map([["BTC", 1.5]]), isLeaf: true, children: [] },
        ],
      },
    ],
  },
];

describe("folder-view", () => {
  describe("ROOT_FOLDER_PATH", () => {
    it("is the synthetic assets root path", () => {
      expect(ROOT_FOLDER_PATH).toBe("assets");
    });
  });

  describe("isFolderSelection", () => {
    it("returns true for the synthetic root", () => {
      expect(isFolderSelection("assets", tree)).toBe(true);
    });
    it("returns true for a non-leaf node", () => {
      expect(isFolderSelection("assets:cash", tree)).toBe(true);
    });
    it("returns false for a leaf node", () => {
      expect(isFolderSelection("assets:cash:cba", tree)).toBe(false);
    });
    it("returns false for undefined / unknown", () => {
      expect(isFolderSelection(undefined, tree)).toBe(false);
      expect(isFolderSelection("assets:bogus", tree)).toBe(false);
    });
  });

  describe("folderDisplayName", () => {
    it("returns the account-set label for the root path", () => {
      expect(folderDisplayName("assets", "Richard")).toBe("Richard");
    });
    it("returns the leaf folder segment for a sub-folder", () => {
      expect(folderDisplayName("assets:cash", "Richard")).toBe("cash");
      expect(folderDisplayName("assets:crypto:btc", "Richard")).toBe("btc");
    });
  });

  describe("folderTotalAud", () => {
    it("returns the cached base-currency total for a folder", () => {
      const totals = new Map([["assets", -50000], ["assets:cash", -337646.16], ["assets:crypto", 923801.9]]);
      expect(folderTotalAud("assets", totals)).toBe(-50000);
      expect(folderTotalAud("assets:cash", totals)).toBe(-337646.16);
    });
    it("returns undefined when no cached total exists", () => {
      expect(folderTotalAud("assets:gone", new Map())).toBeUndefined();
      expect(folderTotalAud(undefined, new Map())).toBeUndefined();
    });
  });

  describe("folderHeaderTotal", () => {
    it("equals the sum of visible immediate child treeBaseTotals (ignoring hidden leaves)", () => {
      // treeBaseTotals knows about a hidden bonds leaf, but the sidebar tree
      // only has cash + crypto. Header must show only what the user sees.
      const totals = new Map([
        ["assets", -337646.16 + 923801.90 + 999999],
        ["assets:cash", -337646.16],
        ["assets:crypto", 923801.90],
        ["assets:bonds", 999999],
      ]);
      const total = folderHeaderTotal("assets", tree, totals);
      expect(total).toBeCloseTo(-337646.16 + 923801.90, 2);
    });

    it("sums children for a sub-folder selection", () => {
      const totals = new Map([
        ["assets:cash:cba", -337646.16],
      ]);
      expect(folderHeaderTotal("assets:cash", tree, totals)).toBe(-337646.16);
    });

    it("returns undefined when the folder isn't in the tree", () => {
      expect(folderHeaderTotal("assets:gone", tree, new Map())).toBeUndefined();
    });

    it("returns undefined for a leaf node (no children to sum)", () => {
      expect(folderHeaderTotal("assets:cash:cba", tree, new Map([["assets:cash:cba", 100]]))).toBeUndefined();
    });
  });

  describe("formatFolderAmount", () => {
    it("formats AUD with $, commas, and no decimals", () => {
      expect(formatFolderAmount(4064836.36, "AUD")).toBe("$4,064,836");
    });
    it("formats USD with $", () => {
      expect(formatFolderAmount(1234.5, "USD")).toBe("$1,235");
    });
    it("formats EUR with €", () => {
      expect(formatFolderAmount(1000, "EUR")).toBe("€1,000");
    });
    it("handles negatives with sign before the symbol", () => {
      expect(formatFolderAmount(-337646.16, "AUD")).toBe("-$337,646");
    });
    it("omits symbol for unknown commodities", () => {
      expect(formatFolderAmount(1.6, "BTC")).toBe("2");
    });
  });

  describe("syntheticFolderBalance", () => {
    it("aggregates leaf-account commodities under a folder prefix", () => {
      const balances: AccountBalance[] = [
        { account: "assets:cash:cba", totals: [{ commodity: "AUD", amount: -337646.16 }] },
        { account: "assets:cash:ubank", totals: [{ commodity: "AUD", amount: 500 }] },
        { account: "assets:crypto:btc", totals: [{ commodity: "BTC", amount: 1.5 }] },
      ];
      const b = syntheticFolderBalance("assets:cash", balances);
      expect(b).toBeDefined();
      expect(b!.account).toBe("assets:cash");
      const aud = b!.totals.find((t) => t.commodity === "AUD");
      expect(aud?.amount).toBeCloseTo(-337146.16, 2);
      expect(b!.totals.find((t) => t.commodity === "BTC")).toBeUndefined();
    });

    it("for the root path aggregates every asset leaf", () => {
      const balances: AccountBalance[] = [
        { account: "assets:cash:cba", totals: [{ commodity: "AUD", amount: 100 }] },
        { account: "assets:crypto:btc", totals: [{ commodity: "BTC", amount: 1 }, { commodity: "AUD", amount: 50000 }] },
      ];
      const b = syntheticFolderBalance("assets", balances);
      expect(b!.totals.find((t) => t.commodity === "AUD")?.amount).toBe(50100);
      expect(b!.totals.find((t) => t.commodity === "BTC")?.amount).toBe(1);
    });

    it("returns undefined for a leaf path (caller uses real balance)", () => {
      const balances: AccountBalance[] = [
        { account: "assets:cash:cba", totals: [{ commodity: "AUD", amount: 100 }] },
      ];
      expect(syntheticFolderBalance("assets:cash:cba", balances)).toBeUndefined();
    });
  });
});
