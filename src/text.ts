/**
 * Pure text helpers extracted from src/main.ts so they can be unit-tested.
 */
import { shortAddress } from "./account-utils";

export function escapeText(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

/** Wrap runs of non-ASCII characters in a highlight span (call AFTER escapeText). */
export function highlightNonAscii(escaped: string): string {
  return escaped.replace(/([^\x00-\x7F]+)/g, '<span class="non-ascii">$1</span>');
}

export function displayName(name: string): string {
  return shortAddress(name);
}
