#!/usr/bin/env python3
"""Reconcile a Solana wallet's mSOL ledger legs against on-chain truth.

Old crypto-sync pulls (pre the 2026-06-23 zerion.py enrichment fix) carry
mSOL artifacts: dropped legs (Zerion returned a content-less `execute` row),
sign-flipped legs (enrichment booked a deposit as a phantom "receive from the
wallet's own token account"), and phantom legs (a move the ATA never saw).

This computes, per transaction, the wallet's TRUE on-chain mSOL delta and the
current wallet-asset mSOL leg in the GENERATED ledger (i.e. post-transform AND
post-rules — so legs already nooped/handled by existing rules are respected),
then appends a single corrector row (`corrector = chain - ledger_leg`) wherever
they differ — uniformly fixing missing / sign-flipped / phantom cases so every
txn's wallet leg matches chain and the wallet nets to the on-chain balance.

Contra routing per corrector: Port Finance txns -> assets:crypto:lending
(deposits/withdrawals stay "owned, in protocol"); everything else by sign ->
equity:trading:sell (disposals) / equity:trading:buy (acquisitions). Routed via
per-txn `_rules.json` rules (commodity_condition mSOL = exact match, so pmSOL /
cmSOL / *_LP legs are untouched).

Self-verifying + idempotent: re-running finds corrector == 0 everywhere and
no-ops. Backs up the CSV and rules first.

Usage:  python3 reconcile_solana_msol.py <wallet_address>
"""
import csv
import json
import re
import shutil
import sys
import time
import urllib.request
from decimal import Decimal
from pathlib import Path

MSOL = "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So"
PORT_PROGRAM = "Port7uDYB3wk6GJAw4KT1WpTeMtSu9bTcChBHkX2LfR"
VAULT = Path.home() / "workspace/accountsv2"
SECRETS = VAULT / "plugins/crypto-sync/.data/secrets.json"
RID_PREFIX = "rpc-msol-reconcile"

CSV_COLUMNS = ["record_id", "provider", "network", "tx_hash", "blockchain", "timestamp",
    "from_address", "to_address", "value", "fee", "status", "tx_type", "token_name",
    "token_symbol", "token_contract", "token_decimals", "block_number", "gas_used",
    "gas_price", "method", "currency", "method_id", "function_name", "input_data",
    "tx_receipt_status", "transaction_index", "cumulative_gas_used", "confirmations"]


def rpc_factory():
    key = json.loads(SECRETS.read_text())["helius_api_key"].strip()
    url = f"https://mainnet.helius-rpc.com/?api-key={key}"

    def rpc(method, params):
        body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
        req = urllib.request.Request(url, body, {"Content-Type": "application/json"})
        for a in range(5):
            try:
                return json.loads(urllib.request.urlopen(req, timeout=30).read())
            except Exception:
                if a == 4:
                    raise
                time.sleep(1.5 * (a + 1))
    return rpc


def account_keys(res):
    return [k["pubkey"] if isinstance(k, dict) else k
            for k in res["transaction"]["message"]["accountKeys"]]


def base_unit_delta(res, atas, wallet):
    """Exact integer base-unit mSOL delta for the wallet's ATA(s) in this tx."""
    meta = res.get("meta") or {}
    keys = account_keys(res)

    def side(name):
        tot = 0
        for tb in (meta.get(name) or []):
            if tb.get("mint") != MSOL:
                continue
            idx = tb["accountIndex"]
            addr = keys[idx] if idx < len(keys) else None
            if addr in atas or tb.get("owner") == wallet:
                tot += int(tb["uiTokenAmount"]["amount"])
        return tot
    return side("postTokenBalances") - side("preTokenBalances")


def is_port(res):
    return PORT_PROGRAM in account_keys(res)


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: reconcile_solana_msol.py <wallet_address>")
    wallet = sys.argv[1]
    wdir = VAULT / "sources/richard/crypto/wallet/solana" / wallet
    csv_path = wdir / f"solana_{wallet}_transactions.csv"
    rules_path = wdir / "_rules.json"
    if not csv_path.exists():
        sys.exit(f"no source csv: {csv_path}")

    rpc = rpc_factory()

    existing_rids = {r["record_id"] for r in csv.DictReader(csv_path.open())}

    # Current wallet-asset mSOL leg per txn from the GENERATED ledger (base units).
    # This is post-transform AND post-rules, so legs already handled by existing
    # rules (e.g. a duplicate nooped to ignore) are respected — the corrector is
    # measured against what the ledger actually shows, not raw source rows.
    ledger_path = (VAULT / "generated/richard/crypto/wallet/solana" / wallet / "ledger.transactions")
    ledger_leg = {}
    if ledger_path.exists():
        leg_re = re.compile(r"assets:crypto:wallet:solana:" + re.escape(wallet) + r"\s+(-?[0-9.]+)\s+mSOL\b")
        hash_re = re.compile(r"txn:([1-9A-HJ-NP-Za-km-z]{60,90})")
        for block in re.split(r"\n\n+", ledger_path.read_text()):
            if "mSOL" not in block:
                continue
            m = hash_re.search(block)
            if not m:
                continue
            h = m.group(1)
            for amt in leg_re.findall(block):
                ledger_leg[h] = ledger_leg.get(h, 0) + int(Decimal(amt) * 10**9)

    # discover the wallet's mSOL ATA(s): live first, else from the ledger's mSOL txns
    atas = {a["pubkey"] for a in
            rpc("getTokenAccountsByOwner", [wallet, {"mint": MSOL}, {"encoding": "jsonParsed"}])
            .get("result", {}).get("value", [])}
    res_cache = {}
    for h in list(ledger_leg):
        res = rpc("getTransaction", [h, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}]).get("result")
        res_cache[h] = res
        if not res:
            continue
        meta = res.get("meta") or {}
        keys = account_keys(res)
        for tb in (meta.get("preTokenBalances") or []) + (meta.get("postTokenBalances") or []):
            if tb.get("mint") == MSOL and tb.get("owner") == wallet:
                idx = tb["accountIndex"]
                if idx < len(keys):
                    atas.add(keys[idx])
    if not atas:
        sys.exit("could not determine the wallet's mSOL token account(s)")
    print(f"wallet {wallet[:8]}…  mSOL ATA(s): {', '.join(a[:8] + '…' for a in atas)}")

    # full on-chain signature set across the ATA(s) -> catches dropped txns too
    sigs = list(ledger_leg)
    for ata in atas:
        before = None
        while True:
            p = {"limit": 1000}
            if before:
                p["before"] = before
            page = rpc("getSignaturesForAddress", [ata, p]).get("result") or []
            if not page:
                break
            sigs += [s["signature"] for s in page if not s.get("err")]
            before = page[-1]["signature"]
            if len(page) < 1000:
                break
    sigs = list(dict.fromkeys(sigs))

    corrections = []   # (sig, corrector_base, blockTime, contra)
    for sig in sigs:
        if f"{RID_PREFIX}:{sig}:msol" in existing_rids:
            continue  # already reconciled (idempotent even before a rebuild)
        res = res_cache.get(sig)
        if res is None:
            res = rpc("getTransaction", [sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}]).get("result")
            res_cache[sig] = res
        if not res:
            continue
        chain = base_unit_delta(res, atas, wallet)
        corrector = chain - ledger_leg.get(sig, 0)
        if corrector == 0:
            continue
        if is_port(res):
            contra = "assets:crypto:lending"
        else:
            contra = "equity:trading:sell" if corrector < 0 else "equity:trading:buy"
        corrections.append((sig, corrector, res.get("blockTime"), contra))

    if not corrections:
        print("nothing to reconcile — source already matches chain.")
        return

    print(f"\ncorrections ({len(corrections)}):")
    for sig, c, bt, contra in sorted(corrections, key=lambda x: x[2] or 0):
        print(f"  {bt}  {c/1e9:+15.9f} mSOL  -> {contra:24}  {sig[:14]}…")

    # ---- append corrector rows ----
    shutil.copy2(csv_path, str(csv_path) + ".msol-reconcile.bak")
    with csv_path.open("a", newline="") as f:
        w = csv.DictWriter(f, fieldnames=CSV_COLUMNS)
        for sig, c, bt, contra in corrections:
            rid = f"{RID_PREFIX}:{sig}:msol"
            if rid in existing_rids:
                continue
            outgoing = c < 0
            row = {col: "" for col in CSV_COLUMNS}
            row.update({
                "record_id": rid, "provider": RID_PREFIX, "network": "solana",
                "tx_hash": sig, "blockchain": "solana", "timestamp": str(bt or ""),
                "from_address": wallet if outgoing else "onchain-reconcile",
                "to_address": "onchain-reconcile" if outgoing else wallet,
                "value": str(abs(c)), "tx_type": "token_transfer",
                "token_name": "Marinade staked SOL", "token_symbol": "mSOL",
                "token_contract": MSOL, "token_decimals": "9", "method": "reconcile",
            })
            w.writerow(row)
    print(f"\nappended corrector rows to {csv_path.name} (backup .msol-reconcile.bak)")

    # ---- add per-txn contra rules (commodity_condition mSOL = exact) ----
    rules_doc = json.loads(rules_path.read_text()) if rules_path.exists() else {"rules": []}
    rules = rules_doc.setdefault("rules", [])
    have = {r.get("id") for r in rules}
    added = 0
    for sig, c, bt, contra in corrections:
        rid = f"rule-msol-reconcile-{sig[:10]}"
        if rid in have:
            continue
        rules.append({
            "id": rid, "match_field": "meta", "pattern": f"txn:{sig}",
            "commodity_condition": "mSOL", "amount_account": contra,
            "comment": "mSOL chain-reconcile corrector contra (Zerion enrichment artifact)",
        })
        added += 1
    if added:
        shutil.copy2(rules_path, str(rules_path) + ".msol-reconcile.bak")
        rules_path.write_text(json.dumps(rules_doc, indent=2) + "\n")
    print(f"added {added} contra rules to {rules_path.name}")


if __name__ == "__main__":
    main()
