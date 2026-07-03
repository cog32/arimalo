/** Virtual scroll — renders a rolling window of table rows. */

export const TX_VISIBLE = 200;
export const TX_BUFFER = 150;
export const TX_WINDOW = TX_VISIBLE + 2 * TX_BUFFER; // 500 total
export const ROW_HEIGHT_EST = 48;

/** Minimum shift in windowStart before re-rendering (avoids thrashing). */
const SHIFT_THRESHOLD = 50;

export type WindowRange = { start: number; end: number };

export function calculateWindow(
  scrollTop: number,
  tableOffsetTop: number,
  totalItems: number,
  containerHeight: number,
): WindowRange {
  const relativeScroll = Math.max(0, scrollTop - tableOffsetTop);
  const visibleStart = Math.floor(relativeScroll / ROW_HEIGHT_EST);
  const start = Math.max(0, visibleStart - TX_BUFFER);
  const end = Math.min(totalItems, start + TX_WINDOW);
  return { start, end };
}

/**
 * Attach a scroll listener to the content container that calls `onWindowChange`
 * when the visible window shifts enough to warrant a re-render.
 * Returns a cleanup function.
 */
export function attachVirtualScroll(
  contentEl: Element,
  totalItems: number,
  getCurrentStart: () => number,
  onWindowChange: (range: WindowRange) => void,
): () => void {
  let rafId = 0;

  function onScroll() {
    if (rafId) return;
    rafId = requestAnimationFrame(() => {
      rafId = 0;
      const table = contentEl.querySelector(".txTable");
      if (!table) return;
      const tableTop = (table as HTMLElement).offsetTop;
      const range = calculateWindow(
        contentEl.scrollTop,
        tableTop,
        totalItems,
        contentEl.clientHeight,
      );
      if (Math.abs(range.start - getCurrentStart()) >= SHIFT_THRESHOLD) {
        onWindowChange(range);
      }
    });
  }

  contentEl.addEventListener("scroll", onScroll, { passive: true });
  return () => {
    contentEl.removeEventListener("scroll", onScroll);
    if (rafId) cancelAnimationFrame(rafId);
  };
}

/** Compute which items to render and spacer sizes.
 *  When Rust returns a pre-paginated slice (rustOffset/rustTotal set),
 *  render all fetched items with spacers for the global position.
 *  Otherwise, apply local windowing from txWindowStart. */
export function computeRenderWindow(
  itemCount: number,
  txWindowStart: number,
  rustOffset: number | undefined,
  rustTotal: number | undefined,
): { sliceStart: number; sliceEnd: number; beforeCount: number; afterCount: number } {
  if (rustOffset !== undefined && rustTotal !== undefined) {
    return {
      sliceStart: 0,
      sliceEnd: itemCount,
      beforeCount: rustOffset,
      afterCount: Math.max(0, rustTotal - rustOffset - itemCount),
    };
  }
  const windowEnd = Math.min(itemCount, txWindowStart + TX_WINDOW);
  return {
    sliceStart: txWindowStart,
    sliceEnd: windowEnd,
    beforeCount: txWindowStart,
    afterCount: itemCount - windowEnd,
  };
}

/** Should virtual scroll be attached?
 *  Uses rustTotal (if available) instead of local item count,
 *  since Rust pagination already limits items to TX_WINDOW. */
export function shouldAttachVirtualScroll(
  itemCount: number,
  rustTotal: number | undefined,
): boolean {
  return (rustTotal ?? itemCount) > TX_WINDOW;
}

/** Build spacer row HTML for above/below the visible window. */
export function spacerRow(itemCount: number, colSpan: number): string {
  if (itemCount <= 0) return "";
  const height = itemCount * ROW_HEIGHT_EST;
  return `<tr class="txTable__spacer"><td colspan="${colSpan}" style="height:${height}px;padding:0;border:0"></td></tr>`;
}
