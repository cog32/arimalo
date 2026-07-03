import { describe, expect, it } from "vitest";
import { escapeText, highlightNonAscii } from "./text";

describe("escapeText", () => {
  it("escapes the standard HTML entities", () => {
    expect(escapeText(`<a href="x">B & 'C'</a>`)).toBe(
      "&lt;a href=&quot;x&quot;&gt;B &amp; &#039;C&#039;&lt;/a&gt;",
    );
  });
  it("escapes ampersands first so existing entities are double-escaped (safe by default)", () => {
    expect(escapeText("&amp;")).toBe("&amp;amp;");
  });
  it("passes plain text through unchanged", () => {
    expect(escapeText("hello world")).toBe("hello world");
  });
});

describe("highlightNonAscii", () => {
  it("wraps non-ascii runs in a span", () => {
    expect(highlightNonAscii("hello café")).toBe(
      'hello caf<span class="non-ascii">é</span>',
    );
  });
  it("groups adjacent non-ascii characters into one span", () => {
    expect(highlightNonAscii("漢字 test")).toBe(
      '<span class="non-ascii">漢字</span> test',
    );
  });
  it("returns ascii-only strings unchanged", () => {
    expect(highlightNonAscii("plain ascii")).toBe("plain ascii");
  });
});
