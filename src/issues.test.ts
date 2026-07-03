import { describe, it, expect, beforeEach, vi } from "vitest";

// The IPC shim `invoke` is mocked so `refreshIssuesCache` can be exercised in Node.
vi.mock("./ipc", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "./ipc";
import {
  collectIssues,
  buildAccountIssueCounts,
  refreshIssuesCache,
  __setIssuesCacheForTesting,
  type IssueState,
  type CollectedIssues,
} from "./issues";

const EMPTY_STATE: IssueState = {};

function sampleCollected(): CollectedIssues {
  return {
    groups: [
      {
        label: "Uncategorised",
        severity: "warning",
        issues: [{ severity: "warning", group: "Uncategorised", message: "3 uncategorised transactions" }],
        filterKind: "uncategorised",
      },
      {
        label: "assets:bank",
        severity: "warning",
        issues: [{ severity: "warning", group: "assets:bank", message: "oops" }],
        account: "assets:bank",
      },
      {
        label: "assets:eth",
        severity: "info",
        issues: [{ severity: "info", group: "assets:eth", message: "Missing data for: 2024-02" }],
        account: "assets:eth",
      },
    ],
    accountCounts: {
      "assets:bank": 3,
      "assets:eth": 1,
    },
  };
}

beforeEach(() => {
  __setIssuesCacheForTesting({ groups: [], accountCounts: {} });
  vi.mocked(invoke).mockReset();
});

describe("refreshIssuesCache", () => {
  it("calls collect_issues_cmd with no args and populates the cache", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(sampleCollected());
    await refreshIssuesCache();
    expect(invoke).toHaveBeenCalledWith("collect_issues_cmd", {});
    expect(collectIssues(EMPTY_STATE)).toHaveLength(3);
  });
});

describe("collectIssues", () => {
  beforeEach(() => __setIssuesCacheForTesting(sampleCollected()));

  it("returns all cached groups when no account filter is provided", () => {
    expect(collectIssues(EMPTY_STATE)).toHaveLength(3);
  });

  it("narrows to groups tagged with the requested account", () => {
    const filtered = collectIssues(EMPTY_STATE, "assets:bank");
    expect(filtered.map((g) => g.label)).toEqual(["assets:bank"]);
  });

  it("returns an empty list for an account with no issues", () => {
    expect(collectIssues(EMPTY_STATE, "assets:unknown")).toEqual([]);
  });
});

describe("buildAccountIssueCounts", () => {
  it("converts the cached accountCounts record into a Map", () => {
    __setIssuesCacheForTesting(sampleCollected());
    const counts = buildAccountIssueCounts(EMPTY_STATE);
    expect(counts.get("assets:bank")).toBe(3);
    expect(counts.get("assets:eth")).toBe(1);
    expect(counts.size).toBe(2);
  });

  it("returns an empty Map when the cache is empty", () => {
    expect(buildAccountIssueCounts(EMPTY_STATE).size).toBe(0);
  });
});
