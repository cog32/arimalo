#!/usr/bin/env python3
"""Do My Crypto Taxes — iterative classifier driver.

Loops:
  1. Find the top-value `expenses:unknown` crypto transaction via arimalo-query.
  2. Build a prompt that injects the DO_MY_TAXES process, the accounts list,
     and the txn's `--print-prompt` context.
  3. Hand the prompt to `claude -p` and let Claude follow the process
     (research, pick an account, write a rule, regenerate, verify).
  4. Repeat until no unknowns remain, the same txn re-appears (no progress),
     or max_iterations is hit.

Arimalo CLIs (arimalo-query, arimalo-classify, arimalo-regenerate) are on
PATH inside the runner; absolute paths are also passed via ctx["bin"].
"""

import json
import os
import select
import subprocess
import sys
import time
from pathlib import Path


def log(msg: str) -> None:
    print(msg, file=sys.stderr, flush=True)


def run(cmd: list[str], **kw) -> subprocess.CompletedProcess:
    log(f"$ {' '.join(cmd)}")
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def find_bin(ctx: dict, name: str) -> str:
    # ctx.bin uses underscores: arimalo_query, arimalo_classify, ...
    key = name.replace("-", "_")
    return ctx.get("bin", {}).get(key) or os.environ.get(key.upper()) or name


def top_unknown(query_bin: str, query: str) -> dict | None:
    r = run([query_bin, query, "--sort", "value", "desc", "--limit", "1", "--format", "json"])
    if r.returncode != 0:
        log(f"arimalo-query failed: {r.stderr}")
        return None
    try:
        data = json.loads(r.stdout)
    except json.JSONDecodeError:
        log(f"arimalo-query returned non-JSON: {r.stdout[:200]}")
        return None
    txns = data.get("transactions") or []
    return txns[0] if txns else None


def txn_id(txn: dict) -> str:
    # Identify the work item, not just the on-chain txn. A single signature
    # can carry multiple unknown postings with different payees (e.g. one
    # txn distributing rewards from two distinct vaults); writing a rule
    # for one of those payees still leaves the others on expenses:unknown,
    # so the no-progress guard must distinguish them. Include the payee in
    # the fingerprint to keep work-items unique per (signature, payee).
    top_meta = txn.get("meta") or ""
    sig = ""
    if "txn:" in top_meta:
        sig = top_meta.split("txn:", 1)[1].split()[0]
    if not sig:
        for p in txn.get("postings", []):
            meta = p.get("meta") or ""
            if "txn:" in meta:
                sig = meta.split("txn:", 1)[1].split()[0]
                break
    payee = (txn.get("payee") or "").strip()
    if sig:
        return f"{sig}::{payee}" if payee else sig
    return f"{txn.get('date')}::{txn.get('amount')}::{txn.get('amount_commodity')}::{payee}"


def print_prompt(classify_bin: str, account_set: str, txid: str) -> str:
    r = run([classify_bin, "--print-prompt", "--account-set", account_set, "--txid", txid])
    if r.returncode != 0:
        log(f"arimalo-classify --print-prompt failed: {r.stderr}")
        return ""
    return r.stdout


def read_or_warn(path: Path, label: str) -> str:
    if not path.exists():
        log(f"warning: {label} not found at {path}")
        return f"(missing: {path})"
    return path.read_text()


def build_prompt(
    process_md: str,
    accounts_md: str,
    txn: dict,
    classify_context: str,
    iteration: int,
    max_iterations: int,
    extra: str,
    chrome_dir: Path,
    cdp_url: str,
) -> str:
    return f"""You are running the DO_MY_TAXES classification process autonomously.
This is iteration {iteration}/{max_iterations}. The plugin driver will loop and
call you again on the next unknown transaction after this run completes.

## Process (verbatim from DO_MY_TAXES.md)

{process_md}

## Existing accounts (verbatim from TAX_ACCOUNTS.md)

{accounts_md}

## Current target transaction (top `expenses:unknown` by value)

```json
{json.dumps(txn, indent=2)}
```

## arimalo-classify --print-prompt context for this txn

{classify_context if classify_context.strip() else "(empty — txn not found by --txid; use the JSON above)"}

## Instructions

- Follow the process above end-to-end for THIS ONE transaction only.
- NEVER invent accounts. Only use accounts already in the ledger.
- DO NOT use `arimalo-classify` to write rules — only `--print-prompt` is allowed.
- Edit the appropriate `_rules.json` directly, then run `arimalo-regenerate`.
- Verify the posting moved off `expenses:unknown` before stopping.
- Do not commit unless explicitly part of the process and only the files you changed.
- When done, exit. The driver will pick the next unknown.

## dev-browser-use invocation (OVERRIDE the flags shown in the process doc)

Always call dev-browser-use with a persistent shared chrome profile AND a
fixed CDP endpoint so a single browser instance is reused across iterations
(faster, avoids re-solving anti-bot challenges, keeps logins). Pass
`--keep-alive` and `--no-isolate-cdp`, and pin both `--chrome-dir` and
`--cdp-url`:

    dev-browser-use --max-steps 5 --no-vision --keep-alive --no-isolate-cdp \\
      --chrome-dir {chrome_dir} \\
      --cdp-url {cdp_url} \\
      "<prompt>"

CRITICAL — flags you MUST NOT pass (they break reuse and force a fresh
Chrome+profile on port 9223 each run, defeating cookie/CF reuse):

  - `--no-keep-alive`   (kills Chrome at end of run)
  - `--isolate-cdp`     (spawns a fresh isolated CDP target on a new port)

Reuse the exact `--chrome-dir` and `--cdp-url` every time so every iteration
attaches to the same already-running browser. After a run, verify reuse by
checking the run's `run-metadata.json`:

    "isolate_existing_cdp": false
    "resolved_cdp_url": "{cdp_url}"

If you see `isolate_existing_cdp: true` or a different port (e.g. 9223), the
flags above were not honored — fix the invocation and retry.

{extra}
"""


# Sentinel return code for a watchdog-triggered termination.
STALL_RC = -999


def call_claude(
    claude_bin: str,
    permission_mode: str,
    prompt: str,
    log_path: Path,
    verbose: bool,
    cwd: Path,
    stall_seconds: int = 300,
) -> int:
    cmd = [claude_bin, "-p", "--dangerously-skip-permissions"]
    if verbose:
        # Stream every event (tool calls, thinking, partial messages) as JSON lines.
        cmd += ["--verbose", "--output-format", "stream-json"]
    log(f"$ (cwd={cwd}) {' '.join(cmd)} <prompt {len(prompt)} chars>  (log: {log_path}, stall={stall_seconds}s)")
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with log_path.open("w") as logf:
        logf.write(f"# prompt ({len(prompt)} chars)\n{prompt}\n\n# --- claude output ---\n")
        logf.flush()
        proc = subprocess.Popen(
            cmd,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
            cwd=str(cwd),
        )
        assert proc.stdin and proc.stdout
        proc.stdin.write(prompt)
        proc.stdin.close()
        # Stall watchdog: read claude's stdout via select() so we can wake up
        # periodically even when no output arrives. If nothing is written for
        # `stall_seconds`, terminate. The dev-browser-use background-task
        # wrapper has been observed to hang in a polling `until` loop forever
        # when the inner browser-use process crashes without writing its
        # exitcode file; this kicks us out of those holes.
        fd = proc.stdout.fileno()
        last_output = time.monotonic()
        poll_interval = min(30.0, max(5.0, stall_seconds / 10))
        while True:
            ready, _, _ = select.select([fd], [], [], poll_interval)
            if ready:
                line = proc.stdout.readline()
                if not line:  # EOF
                    break
                logf.write(line)
                logf.flush()
                sys.stderr.write(_pretty_event(line) if verbose else line)
                sys.stderr.flush()
                last_output = time.monotonic()
                continue
            if proc.poll() is not None:
                break
            idle = time.monotonic() - last_output
            if idle >= stall_seconds:
                log(f"watchdog: no output for {idle:.0f}s (>= {stall_seconds}s) — terminating claude pid={proc.pid}")
                logf.write(f"\n# --- watchdog: stalled {idle:.0f}s, terminating claude pid={proc.pid} ---\n")
                logf.flush()
                try:
                    proc.terminate()
                    try:
                        proc.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        proc.kill()
                        proc.wait(timeout=10)
                except Exception as exc:  # noqa: BLE001
                    log(f"watchdog: error terminating claude: {exc}")
                return STALL_RC
        return proc.wait()


def _pretty_event(line: str) -> str:
    """Best-effort one-line summary of a stream-json event for the live log."""
    s = line.strip()
    if not s:
        return ""
    try:
        ev = json.loads(s)
    except json.JSONDecodeError:
        return f"  {s}\n"
    t = ev.get("type", "?")
    if t == "assistant":
        msg = ev.get("message", {})
        for block in msg.get("content", []):
            bt = block.get("type")
            if bt == "text":
                txt = block.get("text", "").strip().replace("\n", " ")
                if txt:
                    return f"  [text] {txt[:240]}\n"
            elif bt == "tool_use":
                name = block.get("name", "?")
                inp = block.get("input", {})
                hint = inp.get("command") or inp.get("file_path") or inp.get("pattern") or inp.get("description") or ""
                hint = str(hint).replace("\n", " ")[:200]
                return f"  [tool] {name}  {hint}\n"
        return ""
    if t == "user":
        msg = ev.get("message", {})
        for block in msg.get("content", []):
            if block.get("type") == "tool_result":
                out = block.get("content")
                if isinstance(out, list):
                    out = " ".join(b.get("text", "") for b in out if isinstance(b, dict))
                out = str(out).replace("\n", " ")[:200]
                return f"  [result] {out}\n"
        return ""
    if t == "result":
        return f"  [done] subtype={ev.get('subtype')} cost=${ev.get('total_cost_usd', 0):.4f} turns={ev.get('num_turns')}\n"
    if t == "system":
        return f"  [system] {ev.get('subtype', '')}\n"
    return f"  [{t}]\n"


def main() -> int:
    ctx = json.load(sys.stdin)
    config = ctx.get("config", {})

    sources_dir = Path(ctx["sources_dir"])
    vault_root = sources_dir.parent

    account_set = config.get("account_set", "richard")
    max_iterations = int(config.get("max_iterations", 20))
    process_file = config.get("process_file", "DO_MY_TAXES.md")
    accounts_file = config.get("accounts_file", "TAX_ACCOUNTS.md")
    query = config.get("query", "account:expenses:unknown AND account:assets:crypto")
    claude_bin = config.get("claude_bin", "claude")
    extra = config.get("extra_instructions", "") or ""
    verbose = bool(config.get("verbose", True))
    chrome_dir_name = config.get("chrome_dir", "browser-cdp-9444")
    chrome_dir = vault_root / chrome_dir_name
    chrome_dir.mkdir(parents=True, exist_ok=True)
    cdp_port = int(config.get("cdp_port", 9444))
    cdp_url = config.get("cdp_url") or f"http://127.0.0.1:{cdp_port}"
    stall_seconds = int(config.get("stall_seconds", 300))
    log(f"shared chrome profile: {chrome_dir}  cdp: {cdp_url}  stall: {stall_seconds}s")
    data_dir = Path(ctx.get("data_dir") or (Path(ctx["plugin_dir"]) / ".data"))
    run_id = __import__("time").strftime("%Y%m%d-%H%M%S")
    run_log_dir = data_dir / "runs" / run_id
    log(f"per-iteration logs: {run_log_dir}")

    query_bin = find_bin(ctx, "arimalo-query")
    classify_bin = find_bin(ctx, "arimalo-classify")
    regen_bin = find_bin(ctx, "arimalo-regenerate")

    process_md = read_or_warn(vault_root / process_file, "process_file")
    accounts_md = read_or_warn(vault_root / accounts_file, "accounts_file")

    iterations_run = 0
    classified_ids: list[str] = []
    seen_ids: set[str] = set()
    warnings: list[str] = []

    for i in range(1, max_iterations + 1):
        log(f"\n=== iteration {i}/{max_iterations} ===")
        txn = top_unknown(query_bin, query)
        if not txn:
            log("no unknown crypto transactions remain — done.")
            break

        tid = txn_id(txn)
        if tid in seen_ids:
            msg = f"no progress: txn {tid} still top after classification — stopping to avoid loop."
            log(msg)
            warnings.append(msg)
            break
        seen_ids.add(tid)

        log(f"target txn: {tid}  ({txn.get('date')} {txn.get('amount')} {txn.get('amount_commodity')})")

        classify_context = print_prompt(classify_bin, account_set, tid[:16])
        prompt = build_prompt(process_md, accounts_md, txn, classify_context, i, max_iterations, extra, chrome_dir, cdp_url)

        log_path = run_log_dir / f"iter-{i:02d}-{tid[:12]}.log"
        rc = call_claude(claude_bin, "", prompt, log_path, verbose, vault_root, stall_seconds=stall_seconds)
        iterations_run += 1
        if rc == STALL_RC:
            warnings.append(f"claude -p stalled (>{stall_seconds}s no output) on iteration {i} (txn {tid}) — killed by watchdog")
        elif rc != 0:
            warnings.append(f"claude -p exited {rc} on iteration {i} (txn {tid})")

        # Defensive: ensure pipeline is current even if Claude forgot to regen.
        rr = run([regen_bin])
        if rr.returncode != 0:
            warnings.append(f"arimalo-regenerate failed on iteration {i}: {rr.stderr.strip()[:300]}")

        classified_ids.append(tid)
    else:
        warnings.append(f"hit max_iterations ({max_iterations}) — unknowns may remain.")

    result = {
        "files_written": [],  # Claude writes rule files; driver does not touch sources/.
        "records_fetched": iterations_run,
        "iterations": iterations_run,
        "classified_txns": classified_ids,
        "warnings": warnings,
    }
    print(json.dumps(result))
    return 0 if not warnings else 2


if __name__ == "__main__":
    sys.exit(main())
