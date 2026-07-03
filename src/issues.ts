/**
 * Data-quality issue collection — UI-side thin client.
 *
 * Collection logic lives in Rust (`src-tauri/src/issues.rs`) and is exposed via
 * the `collect_issues_cmd` Tauri command and the `arimalo-issues` CLI. This
 * module caches the most recent backend result so the synchronous render path
 * in main.ts can read issues without awaiting.
 *
 * Call `refreshIssuesCache()` after startup and whenever the pipeline rebuilds;
 * the render loop reads the cache via `collectIssues` / `buildAccountIssueCounts`.
 */

import { invoke } from "./ipc";

export type IssueSeverity = "error" | "warning" | "info";

export type Issue = {
  severity: IssueSeverity;
  group: string;
  message: string;
  filterKind?: string;
  revealPath?: string;
  accounts?: string[];
  tradeSuggestionIdx?: number;
  scrollToTxnId?: string;
};

export type IssueGroup = {
  label: string;
  severity: IssueSeverity;
  issues: Issue[];
  filterKind?: string;
  revealPath?: string;
  account?: string;
};

export type AccountGap = {
  account: string;
  first_month: string;
  last_month: string;
  missing_months: string[];
};

/** Backend payload from `collect_issues_cmd`. */
export type CollectedIssues = {
  groups: IssueGroup[];
  accountCounts: Record<string, number>;
};

/** Kept for call-site compatibility; all fields now optional and unused. */
export interface IssueState {
  parse?: unknown;
  pipelineWarnings?: string[];
  accountFoldersMap?: Record<string, string>;
  sourceFolderPaths?: Record<string, string>;
  accountGaps?: AccountGap[];
  tradeSuggestions?: { txn_id_a: string; txn_id_b: string; summary: string }[];
}

// ── Module-level cache ──

let cache: CollectedIssues = { groups: [], accountCounts: {} };

/** Fetch a fresh snapshot from the backend and update the cache. */
export async function refreshIssuesCache(): Promise<void> {
  cache = await invoke<CollectedIssues>("collect_issues_cmd", {});
}

/** Test seam: replace the cache directly (used by Vitest). */
export function __setIssuesCacheForTesting(next: CollectedIssues): void {
  cache = next;
}

/**
 * Return the cached issue groups, optionally narrowed to a single account.
 * The backend returns groups across all accounts; we filter client-side using
 * the `account` field set on per-account groups.
 */
export function collectIssues(_state: IssueState, forAccount?: string): IssueGroup[] {
  if (!forAccount) return cache.groups;
  return cache.groups.filter((g) => {
    // Groups with no `account` tag are global (parse errors, uncategorised,
    // unverified, trade suggestions). Keep them only when the global pane
    // is shown. When drilling into one account, hide global groups so the
    // panel shows just the selected account's issues.
    return g.account === forAccount;
  });
}

/** Per-account issue counts for sidebar badges. */
export function buildAccountIssueCounts(_state: IssueState): Map<string, number> {
  return new Map(Object.entries(cache.accountCounts));
}
