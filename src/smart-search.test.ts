import { describe, it, expect } from "vitest";
import { isPillValueComplete, parseSmartInput } from "./smart-search";

describe("isPillValueComplete", () => {
  it("rejects bare operators", () => {
    expect(isPillValueComplete(">")).toBe(false);
    expect(isPillValueComplete(">=")).toBe(false);
    expect(isPillValueComplete("<")).toBe(false);
    expect(isPillValueComplete("<=")).toBe(false);
    expect(isPillValueComplete("=")).toBe(false);
    expect(isPillValueComplete("..")).toBe(false);
  });

  it("rejects empty string", () => {
    expect(isPillValueComplete("")).toBe(false);
  });

  it("accepts operators followed by numbers", () => {
    expect(isPillValueComplete(">0")).toBe(true);
    expect(isPillValueComplete(">=100")).toBe(true);
    expect(isPillValueComplete("<50")).toBe(true);
    expect(isPillValueComplete("<=999")).toBe(true);
    expect(isPillValueComplete("=42")).toBe(true);
  });

  it("accepts range values", () => {
    expect(isPillValueComplete("10..500")).toBe(true);
  });

  it("accepts text values", () => {
    expect(isPillValueComplete("narration")).toBe(true);
    expect(isPillValueComplete("payee")).toBe(true);
    expect(isPillValueComplete("eth")).toBe(true);
  });

  it("accepts plain numbers", () => {
    expect(isPillValueComplete("100")).toBe(true);
    expect(isPillValueComplete("0")).toBe(true);
  });

  it("rejects unclosed quoted value", () => {
    expect(isPillValueComplete('"Solend Main')).toBe(false);
    expect(isPillValueComplete('"hello')).toBe(false);
  });

  it("accepts closed quoted value", () => {
    expect(isPillValueComplete('"Solend Main Lending"')).toBe(true);
    expect(isPillValueComplete('"hello"')).toBe(true);
  });
});

describe("parseSmartInput", () => {
  const keywords = ["field", "amount", "fee"];

  it("extracts complete keyword:value as pills", () => {
    const result = parseSmartInput("fee:>0 *trade*", keywords);
    expect(result.pills).toEqual([{ key: "fee", value: ">0" }]);
    expect(result.text).toBe("*trade*");
  });

  it("does NOT extract incomplete values like fee:>", () => {
    const result = parseSmartInput("fee:> some text", keywords);
    expect(result.pills).toEqual([]);
    expect(result.text).toBe("fee:> some text");
  });

  it("does NOT extract fee:>=  (bare operator, no number)", () => {
    const result = parseSmartInput("fee:>= some text", keywords);
    expect(result.pills).toEqual([]);
    expect(result.text).toBe("fee:>= some text");
  });

  it("extracts fee:>=100 as complete", () => {
    const result = parseSmartInput("fee:>=100", keywords);
    expect(result.pills).toEqual([{ key: "fee", value: ">=100" }]);
    expect(result.text).toBe("");
  });

  it("handles keyword: value (space after colon) with complete value", () => {
    const result = parseSmartInput("fee: >0 *trade*", keywords);
    expect(result.pills).toEqual([{ key: "fee", value: ">0" }]);
    expect(result.text).toBe("*trade*");
  });

  it("does NOT combine keyword: with incomplete value", () => {
    const result = parseSmartInput("fee: > some text", keywords);
    // "fee:" has empty value, next token ">" is incomplete → both stay as text
    expect(result.pills).toEqual([]);
    expect(result.text).toBe("fee: > some text");
  });

  it("extracts multiple pills", () => {
    const result = parseSmartInput("field:narration amount:>100 fee:>0 *pattern*", keywords);
    expect(result.pills).toEqual([
      { key: "field", value: "narration" },
      { key: "amount", value: ">100" },
      { key: "fee", value: ">0" },
    ]);
    expect(result.text).toBe("*pattern*");
  });

  it("ignores unknown keywords", () => {
    const result = parseSmartInput("unknown:value fee:>0", keywords);
    expect(result.pills).toEqual([{ key: "fee", value: ">0" }]);
    expect(result.text).toBe("unknown:value");
  });

  it("handles empty input", () => {
    const result = parseSmartInput("", keywords);
    expect(result.pills).toEqual([]);
    expect(result.text).toBe("");
  });

  it("handles text-only input (no keywords)", () => {
    const result = parseSmartInput("*trade tradespot*", keywords);
    expect(result.pills).toEqual([]);
    expect(result.text).toBe("*trade tradespot*");
  });

  it("extracts range values", () => {
    const result = parseSmartInput("amount:10..500", keywords);
    expect(result.pills).toEqual([{ key: "amount", value: "10..500" }]);
    expect(result.text).toBe("");
  });

  it("extracts negated pill with - prefix", () => {
    const result = parseSmartInput("-fee:>0 *trade*", keywords);
    expect(result.pills).toEqual([{ key: "fee", value: ">0", negated: true }]);
    expect(result.text).toBe("*trade*");
  });

  it("extracts negated pill for text values", () => {
    const allKw = ["field", "amount", "fee", "commodity"];
    const result = parseSmartInput("-commodity:SPAM", allKw);
    expect(result.pills).toEqual([{ key: "commodity", value: "SPAM", negated: true }]);
    expect(result.text).toBe("");
  });

  it("treats bare - as text, not a pill", () => {
    const result = parseSmartInput("- some text", keywords);
    expect(result.pills).toEqual([]);
    expect(result.text).toBe("- some text");
  });

  it("mixes negated and non-negated pills", () => {
    const result = parseSmartInput("fee:>0 -amount:>1000 *pattern*", keywords);
    expect(result.pills).toEqual([
      { key: "fee", value: ">0" },
      { key: "amount", value: ">1000", negated: true },
    ]);
    expect(result.text).toBe("*pattern*");
  });

  it("extracts negated pill with space after colon (-keyword: value)", () => {
    const result = parseSmartInput("-fee: >0 *trade*", keywords);
    expect(result.pills).toEqual([{ key: "fee", value: ">0", negated: true }]);
    expect(result.text).toBe("*trade*");
  });

  it("extracts quoted multi-word value as a single pill", () => {
    const allKw = ["field", "amount", "fee", "payee"];
    const result = parseSmartInput('payee:"Solend Main Lending" *trade*', allKw);
    expect(result.pills).toEqual([{ key: "payee", value: "Solend Main Lending" }]);
    expect(result.text).toBe("*trade*");
  });

  it("extracts negated quoted multi-word value", () => {
    const allKw = ["field", "amount", "fee", "payee"];
    const result = parseSmartInput('-payee:"Solend Main Lending"', allKw);
    expect(result.pills).toEqual([{ key: "payee", value: "Solend Main Lending", negated: true }]);
    expect(result.text).toBe("");
  });

  it("does not create pill for unclosed quote", () => {
    const allKw = ["field", "amount", "fee", "payee"];
    const result = parseSmartInput('payee:"Solend Main', allKw);
    expect(result.pills).toEqual([]);
    expect(result.text).toBe('payee:"Solend Main');
  });

  it("extracts quoted value with space after colon", () => {
    const allKw = ["field", "amount", "fee", "payee"];
    const result = parseSmartInput('payee: "Solend Main Lending" *trade*', allKw);
    expect(result.pills).toEqual([{ key: "payee", value: "Solend Main Lending" }]);
    expect(result.text).toBe("*trade*");
  });

  it("mixes quoted and unquoted pills", () => {
    const allKw = ["field", "amount", "fee", "payee"];
    const result = parseSmartInput('payee:"Solend Main Lending" fee:>0 *pattern*', allKw);
    expect(result.pills).toEqual([
      { key: "payee", value: "Solend Main Lending" },
      { key: "fee", value: ">0" },
    ]);
    expect(result.text).toBe("*pattern*");
  });

  it("unquoted multi-word value consumes until next keyword or end", () => {
    const allKw = ["field", "amount", "fee", "payee"];
    const result = parseSmartInput("payee:Swell Network", allKw);
    expect(result.pills).toEqual([{ key: "payee", value: "Swell Network" }]);
    expect(result.text).toBe("");
  });

  it("unquoted multi-word value stops at next keyword token", () => {
    const allKw = ["field", "amount", "fee", "payee"];
    const result = parseSmartInput("payee:Swell Network fee:>0 *pattern*", allKw);
    expect(result.pills).toEqual([
      { key: "payee", value: "Swell Network" },
      { key: "fee", value: ">0" },
    ]);
    expect(result.text).toBe("*pattern*");
  });

  it("unquoted multi-word value does not consume wildcard patterns", () => {
    const allKw = ["field", "amount", "fee", "payee"];
    const result = parseSmartInput("payee:Swell Network *token*", allKw);
    expect(result.pills).toEqual([{ key: "payee", value: "Swell Network" }]);
    expect(result.text).toBe("*token*");
  });
});
