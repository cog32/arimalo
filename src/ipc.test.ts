// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke, isTauri } from "./ipc";

describe("ipc shim (browser / PWA mode)", () => {
  beforeEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    vi.restoreAllMocks();
  });

  it("detects no Tauri runtime in jsdom", () => {
    expect(isTauri()).toBe(false);
  });

  it("routes invoke() to POST /api/<cmd> with the args as the JSON body", async () => {
    const fetchMock = vi.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => [2024, 2025],
    }));
    vi.stubGlobal("fetch", fetchMock);

    const result = await invoke<number[]>("list_report_years_cmd", {
      accountSet: "",
      reportType: "cgt",
    });

    expect(result).toEqual([2024, 2025]);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, opts] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe("/api/list_report_years_cmd");
    expect(opts.method).toBe("POST");
    expect(JSON.parse(opts.body as string)).toEqual({
      accountSet: "",
      reportType: "cgt",
    });
  });

  it("rejects with the server error message on a non-ok response", async () => {
    const fetchMock = vi.fn(async () => ({
      ok: false,
      status: 422,
      json: async () => ({ error: "command not supported in web mode: rebuild_pipeline" }),
    }));
    vi.stubGlobal("fetch", fetchMock);

    await expect(invoke("rebuild_pipeline", {})).rejects.toThrow("not supported");
  });

  it("sends an empty object body when no args are given", async () => {
    const fetchMock = vi.fn(async () => ({ ok: true, status: 200, json: async () => true }));
    vi.stubGlobal("fetch", fetchMock);

    await invoke<boolean>("has_root_dir");

    const [, opts] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(JSON.parse(opts.body as string)).toEqual({});
  });
});
