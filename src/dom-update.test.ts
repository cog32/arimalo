// @vitest-environment jsdom
// Architectural fixture tests for the morphdom-based DOM update path.
//
// These prove the non-intrusive update contract: re-applying the same
// HTML produces zero structural changes; the existing DOM nodes keep
// their identity (and any DOM properties / attached state) across
// re-renders. This is the test of record for the "UI works on the
// local DOM" requirement — if a future refactor goes back to wholesale
// innerHTML replacement these will fail loudly.

import { describe, it, expect, beforeEach } from "vitest";
import { morphInPlace } from "./dom-update";

function setup(html: string): HTMLElement {
  document.body.innerHTML = `<div id="root">${html}</div>`;
  return document.getElementById("root")!;
}

describe("morphInPlace — non-intrusive DOM updates", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("preserves node identity when the new HTML is identical", () => {
    const root = setup(`<table><tbody><tr id="r1"><td>A</td></tr></tbody></table>`);
    const beforeRow = root.querySelector("#r1");

    morphInPlace(
      root,
      `<div id="root"><table><tbody><tr id="r1"><td>A</td></tr></tbody></table></div>`,
    );

    const afterRow = root.querySelector("#r1");
    expect(afterRow).toBe(beforeRow);
  });

  it("preserves a JS property on a row across an identical render", () => {
    // Identity is the architectural guarantee — runtime DOM properties
    // (e.g. event listeners attached via addEventListener, custom JS
    // expandos) survive a re-render because morphdom keeps the
    // existing element rather than replacing it. Runtime *attributes*
    // not in the new HTML are still synced by morphdom — that's
    // expected; if we want them preserved we'd put them in the HTML.
    const root = setup(`<table><tbody><tr id="r1"><td>A</td></tr></tbody></table>`);
    const row = root.querySelector("#r1") as HTMLElement & { __testMarker?: string };
    row.__testMarker = "preserved";

    morphInPlace(
      root,
      `<div id="root"><table><tbody><tr id="r1"><td>A</td></tr></tbody></table></div>`,
    );

    expect(row.__testMarker).toBe("preserved");
    expect(document.contains(row)).toBe(true);
  });

  it("preserves identity for unchanged rows when only one row's text changes", () => {
    const root = setup(`
      <table><tbody>
        <tr id="r1"><td>A</td></tr>
        <tr id="r2"><td>B</td></tr>
        <tr id="r3"><td>C</td></tr>
      </tbody></table>
    `);
    const r1Before = root.querySelector("#r1");
    const r2Before = root.querySelector("#r2");
    const r3Before = root.querySelector("#r3");

    morphInPlace(
      root,
      `<div id="root"><table><tbody>
        <tr id="r1"><td>A</td></tr>
        <tr id="r2"><td>B-changed</td></tr>
        <tr id="r3"><td>C</td></tr>
      </tbody></table></div>`,
    );

    expect(root.querySelector("#r1")).toBe(r1Before);
    expect(root.querySelector("#r3")).toBe(r3Before);
    // r2's <tr> can survive too — morphdom updates the inner text on the
    // existing element rather than replacing it.
    expect(root.querySelector("#r2")).toBe(r2Before);
    expect(root.querySelector("#r2")?.textContent).toBe("B-changed");
  });

  it("removes a row that's gone from the new HTML", () => {
    const root = setup(`
      <ul>
        <li id="a">A</li>
        <li id="b">B</li>
        <li id="c">C</li>
      </ul>
    `);
    const aBefore = root.querySelector("#a");
    const cBefore = root.querySelector("#c");

    morphInPlace(
      root,
      `<div id="root"><ul>
        <li id="a">A</li>
        <li id="c">C</li>
      </ul></div>`,
    );

    expect(root.querySelector("#a")).toBe(aBefore);
    expect(root.querySelector("#b")).toBeNull();
    expect(root.querySelector("#c")).toBe(cBefore);
  });

  it("keyed rows: removing one txn row removes THAT row, not the last position", () => {
    // Without getNodeKey, morphdom matches children by tag+position. If
    // the new HTML has 4 rows and the old DOM has 5, morphdom would
    // position-update rows 0..3 (their content shifts up to match the
    // new sequence) and remove row 4. The CLICKED row's <tr> survives
    // with the next row's content — visually "the row is still there"
    // and the bottom row vanishes. getNodeKey on data-txn-row-id makes
    // morphdom remove the actual deleted row.
    const root = setup(`
      <table><tbody>
        <tr data-testid="txn-row" data-txn-row-id="txn:a"><td>Alice</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:b"><td>Bob</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:c"><td>Carol</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:d"><td>Dave</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:e"><td>Eve</td></tr>
      </tbody></table>
    `);
    const aliceBefore = root.querySelector('[data-txn-row-id="txn:a"]');
    const carolBefore = root.querySelector('[data-txn-row-id="txn:c"]');
    const eveBefore = root.querySelector('[data-txn-row-id="txn:e"]');

    // New HTML omits "txn:c" — Carol was deleted.
    morphInPlace(
      root,
      `<div id="root"><table><tbody>
        <tr data-testid="txn-row" data-txn-row-id="txn:a"><td>Alice</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:b"><td>Bob</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:d"><td>Dave</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:e"><td>Eve</td></tr>
      </tbody></table></div>`,
    );

    // Alice and Eve must survive with their original DOM identity.
    // Carol must be gone. The clicked row IS the one removed.
    expect(root.querySelector('[data-txn-row-id="txn:a"]')).toBe(aliceBefore);
    expect(root.querySelector('[data-txn-row-id="txn:c"]')).toBeNull();
    expect(root.querySelector('[data-txn-row-id="txn:e"]')).toBe(eveBefore);
    // The "carolBefore" reference shouldn't be in the DOM anywhere.
    expect(document.contains(carolBefore!)).toBe(false);
    // Total row count is now 4.
    expect(root.querySelectorAll('[data-testid="txn-row"]').length).toBe(4);
  });

  it("multi-leg txn: each leg is keyed independently by data-expand-key", () => {
    // Live-app reproduction: a single on-chain Solana txn renders as N
    // separate <tr>s (one per posting/leg). They all share the same
    // data-txn-row-id (the meta's txn id), so keying by it collapses
    // the whole group to one node in morphdom's map. Only data-expand-key
    // is unique per leg (txnId|datetime|amount|narration). With it the
    // identity of every leg survives across renders and an "expand
    // leg-2 only" render keeps the right detail row attached.
    const root = setup(`
      <table><tbody>
        <tr data-testid="txn-row" data-txn-row-id="txn:multi" data-expand-key="txn:multi|t|10|leg-out"><td>Out 10</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:multi" data-expand-key="txn:multi|t|-10|leg-in"><td>In -10</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:multi" data-expand-key="txn:multi|t|0.1|leg-fee"><td>Fee 0.1</td></tr>
      </tbody></table>
    `);
    const outBefore = root.querySelector('[data-expand-key="txn:multi|t|10|leg-out"]');
    const inBefore = root.querySelector('[data-expand-key="txn:multi|t|-10|leg-in"]');
    const feeBefore = root.querySelector('[data-expand-key="txn:multi|t|0.1|leg-fee"]');

    // User clicks the "in" leg → only THAT leg expands. New HTML
    // inserts the detail row right after the "in" leg.
    morphInPlace(
      root,
      `<div id="root"><table><tbody>
        <tr data-testid="txn-row" data-txn-row-id="txn:multi" data-expand-key="txn:multi|t|10|leg-out"><td>Out 10</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:multi" data-expand-key="txn:multi|t|-10|leg-in"><td>In -10</td></tr>
        <tr class="txRow__detail" data-txn-detail-for="txn:multi|t|-10|leg-in"><td>Detail for in</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:multi" data-expand-key="txn:multi|t|0.1|leg-fee"><td>Fee 0.1</td></tr>
      </tbody></table></div>`,
    );

    // Every leg's identity survives — none of them was reused as a
    // peer or merged.
    expect(root.querySelector('[data-expand-key="txn:multi|t|10|leg-out"]')).toBe(outBefore);
    expect(root.querySelector('[data-expand-key="txn:multi|t|-10|leg-in"]')).toBe(inBefore);
    expect(root.querySelector('[data-expand-key="txn:multi|t|0.1|leg-fee"]')).toBe(feeBefore);

    // The detail row is positioned right after the "in" leg, not
    // floating after the "fee" leg or somewhere else.
    expect(inBefore?.nextElementSibling?.getAttribute("data-txn-detail-for"))
      .toBe("txn:multi|t|-10|leg-in");

    // No leg was duplicated.
    const allLegs = root.querySelectorAll<HTMLTableRowElement>('[data-testid="txn-row"]');
    expect(allLegs.length).toBe(3);
    const expandKeys = Array.from(allLegs).map((r) => r.getAttribute("data-expand-key"));
    expect(new Set(expandKeys).size).toBe(3);
  });

  it("multi-leg swap rows: removing one leg leaves the other intact", () => {
    // Regression: a single on-chain transaction with multiple postings
    // renders as multiple <tr>s, all carrying the SAME data-txn-row-id
    // (the meta's txn id, not the posting). getNodeKey returns
    // `txn:${txnRowId}` for both, which collides in morphdom's keyed
    // map. After the user deletes one of the legs (the click handler
    // does tr.remove() on the clicked leg only, then pendingDeletes
    // adds the txn id, then the next render filters BOTH legs out for
    // a moment), morphdom's diff has to handle the surviving sibling
    // gracefully when subsequent renders bring legs back.
    //
    // Symptom: clicking another row shows a "hidden" row appearing —
    // the surviving sibling that morphdom didn't track right.
    const root = setup(`
      <table><tbody>
        <tr data-testid="txn-row" data-txn-row-id="txn:swap" data-expand-key="txn:swap|leg1"><td>Leg 1</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:swap" data-expand-key="txn:swap|leg2"><td>Leg 2</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:other" data-expand-key="txn:other|x"><td>Other</td></tr>
      </tbody></table>
    `);
    const leg2Before = root.querySelector('[data-expand-key="txn:swap|leg2"]');
    const otherBefore = root.querySelector('[data-expand-key="txn:other|x"]');

    // The click handler removes leg1 directly via tr.remove().
    leg2Before!.previousElementSibling!.remove();

    // The next render's HTML still contains the surviving leg2 plus
    // the unrelated other row. morphdom should keep leg2 in place.
    morphInPlace(
      root,
      `<div id="root"><table><tbody>
        <tr data-testid="txn-row" data-txn-row-id="txn:swap" data-expand-key="txn:swap|leg2"><td>Leg 2</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:other" data-expand-key="txn:other|x"><td>Other</td></tr>
      </tbody></table></div>`,
    );

    expect(root.querySelector('[data-expand-key="txn:swap|leg1"]')).toBeNull();
    expect(root.querySelector('[data-expand-key="txn:swap|leg2"]')).toBe(leg2Before);
    expect(root.querySelector('[data-expand-key="txn:other|x"]')).toBe(otherBefore);
    expect(root.querySelectorAll("[data-txn-row-id]").length).toBe(2);
  });

  it("multi-leg swap rows: a render that re-adds a deleted leg restores the right row", () => {
    // After the optimistic delete, .finally clears pendingDeletes and
    // a subsequent render brings the unwanted-deleted siblings back.
    // morphdom must put the new row in the right position next to
    // the surviving sibling and not orphan or duplicate it.
    const root = setup(`
      <table><tbody>
        <tr data-testid="txn-row" data-txn-row-id="txn:swap" data-expand-key="txn:swap|leg2"><td>Leg 2</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:other" data-expand-key="txn:other|x"><td>Other</td></tr>
      </tbody></table>
    `);

    // Render restores leg1 (e.g. user undid, or pipeline rebuilt).
    morphInPlace(
      root,
      `<div id="root"><table><tbody>
        <tr data-testid="txn-row" data-txn-row-id="txn:swap" data-expand-key="txn:swap|leg1"><td>Leg 1</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:swap" data-expand-key="txn:swap|leg2"><td>Leg 2</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:other" data-expand-key="txn:other|x"><td>Other</td></tr>
      </tbody></table></div>`,
    );

    const trs = root.querySelectorAll<HTMLTableRowElement>("[data-txn-row-id]");
    expect(trs.length).toBe(3);
    expect(trs[0].getAttribute("data-expand-key")).toBe("txn:swap|leg1");
    expect(trs[1].getAttribute("data-expand-key")).toBe("txn:swap|leg2");
    expect(trs[2].getAttribute("data-expand-key")).toBe("txn:other|x");
  });

  it("detail row sticks with its owning row when earlier rows disappear", () => {
    // Live-app reproduction: rowA is expanded (so DOM has [rowA,
    // detail-A, rowB, detail-B]). The user deletes rowA via the X
    // button: tr.remove() and detail.remove() drop both, then
    // pendingDeletes makes the next render exclude rowA. New HTML is
    // [rowB, detail-B]. morphdom must NOT leave detail-A's slot
    // "stuck" between rows so detail-B drifts to the wrong row.
    //
    // Without keying detail rows by data-txn-detail-for, the unkeyed
    // detail row from a previous render would morph in place against
    // whatever <tr> happened to be in that slot now — surfacing as a
    // detail row sitting under an unrelated row.
    const root = setup(`
      <table><tbody>
        <tr data-testid="txn-row" data-txn-row-id="txn:a"><td>Alice</td></tr>
        <tr class="txRow__detail" data-txn-detail-for="txn:a"><td>Detail A</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:b"><td>Bob</td></tr>
        <tr class="txRow__detail" data-txn-detail-for="txn:b"><td>Detail B</td></tr>
      </tbody></table>
    `);
    const detailBBefore = root.querySelector('[data-txn-detail-for="txn:b"]');

    morphInPlace(
      root,
      `<div id="root"><table><tbody>
        <tr data-testid="txn-row" data-txn-row-id="txn:b"><td>Bob</td></tr>
        <tr class="txRow__detail" data-txn-detail-for="txn:b"><td>Detail B</td></tr>
      </tbody></table></div>`,
    );

    expect(root.querySelector('[data-txn-row-id="txn:a"]')).toBeNull();
    expect(root.querySelector('[data-txn-detail-for="txn:a"]')).toBeNull();
    expect(root.querySelector('[data-txn-detail-for="txn:b"]')).toBe(detailBBefore);
    // Critical: detail-B must follow row-B, not be a stranded sibling.
    const rowB = root.querySelector('[data-txn-row-id="txn:b"]');
    expect(rowB?.nextElementSibling).toBe(detailBBefore);
    expect(root.querySelectorAll("tr").length).toBe(2);
  });

  it("re-ordered rows: detail row moves with its owning row", () => {
    // After a sort change the row order flips, but detail rows must
    // stay attached to their txn row, not to whatever sat at their
    // previous index.
    const root = setup(`
      <table><tbody>
        <tr data-testid="txn-row" data-txn-row-id="txn:a"><td>Alice</td></tr>
        <tr class="txRow__detail" data-txn-detail-for="txn:a"><td>Detail A</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:b"><td>Bob</td></tr>
      </tbody></table>
    `);

    morphInPlace(
      root,
      `<div id="root"><table><tbody>
        <tr data-testid="txn-row" data-txn-row-id="txn:b"><td>Bob</td></tr>
        <tr data-testid="txn-row" data-txn-row-id="txn:a"><td>Alice</td></tr>
        <tr class="txRow__detail" data-txn-detail-for="txn:a"><td>Detail A</td></tr>
      </tbody></table></div>`,
    );

    const trs = root.querySelectorAll<HTMLTableRowElement>("tr");
    expect(trs.length).toBe(3);
    expect(trs[0].getAttribute("data-txn-row-id")).toBe("txn:b");
    expect(trs[1].getAttribute("data-txn-row-id")).toBe("txn:a");
    expect(trs[2].getAttribute("data-txn-detail-for")).toBe("txn:a");
  });

  it("does NOT clobber the value of a focused input mid-edit", () => {
    const root = setup(`<form><input id="search" value="" /></form>`);
    const input = root.querySelector<HTMLInputElement>("#search")!;
    input.focus();
    input.value = "user typed this";
    expect(document.activeElement).toBe(input);

    // A render fires while the user is typing — the new HTML wants to
    // restore value="" because the renderer doesn't know the user is
    // editing. morphdom's onBeforeElUpdated should skip the focused
    // input.
    morphInPlace(
      root,
      `<div id="root"><form><input id="search" value="" /></form></div>`,
    );

    expect(input.value).toBe("user typed this");
    expect(document.activeElement).toBe(input);
  });
});
