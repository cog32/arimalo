// Non-intrusive DOM update: applies a new HTML description to an existing
// element via morphdom, only swapping the parts that actually changed.
//
// Architectural contract: any subtree whose new HTML is byte-identical to
// the existing DOM is left untouched. Focused inputs (mid-edit) are
// explicitly preserved — morphdom will still update other elements but
// won't replace the focused input's value or selection.

import morphdom from "morphdom";

export function morphInPlace(target: Element, html: string): void {
  morphdom(target, html, {
    // Key txn rows by data-expand-key (unique per posting) rather than
    // data-txn-row-id (which collides — one on-chain txn renders as N
    // rows, one per posting/leg, and they all carry the same txnId).
    // Without unique-per-row keys, morphdom collapses the group into a
    // single node and the surviving 5/6 are matched by position,
    // shuffling content across rows and surfacing duplicates plus
    // orphaned detail rows. Detail rows (the expanded sub-row inserted
    // by toggleTxnExpand) carry data-txn-detail-for, which IS the
    // expand-key — same uniqueness, same scheme. Anything else with a
    // stable id attribute gets keyed by id.
    getNodeKey: (node) => {
      if (node.nodeType !== 1) return undefined;
      const el = node as Element;
      const expandKey = el.getAttribute("data-expand-key");
      if (expandKey) return `row:${expandKey}`;
      const detailFor = el.getAttribute("data-txn-detail-for");
      if (detailFor) return `detail:${detailFor}`;
      // Fallback for non-txn rows that still want stable identity
      // (e.g. the <tr> for a manual transaction without an expand key,
      // or other elements that just carry data-txn-row-id).
      const txnRowId = el.getAttribute("data-txn-row-id");
      if (txnRowId) return `txn:${txnRowId}`;
      return el.id || undefined;
    },
    onBeforeElUpdated: (fromEl, toEl) => {
      // Leave imperatively-managed subtrees (e.g. the ApexCharts mount) alone —
      // morphdom must not reconcile a widget's injected children against our
      // empty placeholder. The widget owns everything inside the marked node.
      if (fromEl instanceof Element && fromEl.hasAttribute("data-morph-preserve")) {
        return false;
      }
      if (fromEl.isEqualNode(toEl)) return false;
      if (
        document.activeElement === fromEl &&
        (fromEl instanceof HTMLInputElement || fromEl instanceof HTMLTextAreaElement)
      ) {
        return false;
      }
      return true;
    },
  });
}
