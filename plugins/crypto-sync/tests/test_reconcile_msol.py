"""Tests for scripts/reconcile_solana_msol.py core logic.

Reconciles a wallet's mSOL against chain by token-account ADDRESS (2021-era RPC
omits `owner`), so the signed base-unit delta and Port-program detection must be
correct. The corrector (chain - ledger_leg) then fixes missing/sign-flipped/
phantom legs uniformly.
"""
import importlib.util
from pathlib import Path

SCRIPT = Path(__file__).parent.parent / "scripts" / "reconcile_solana_msol.py"
MSOL = "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So"
PORT = "Port7uDYB3wk6GJAw4KT1WpTeMtSu9bTcChBHkX2LfR"
ATA = "C65Xof4phHffbqrde5LMK128PZK71UU6SGLsX9ujG89C"
WALLET = "3FXQVRb1kEXeP7pmsrdUnfEXVdszj9dVZyfY3shx4gnt"
POOL = "So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo"


def _load():
    spec = importlib.util.spec_from_file_location("reconcile_msol", SCRIPT)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _tx(keys, pre, post):
    def bal(entries):
        return [
            {"accountIndex": i, "mint": m, "owner": o, "uiTokenAmount": {"amount": str(a)}}
            for (i, m, o, a) in entries
        ]
    return {"transaction": {"message": {"accountKeys": keys}},
            "meta": {"preTokenBalances": bal(pre), "postTokenBalances": bal(post)}}


reconcile = _load()


def test_sign_flip_delta_by_ata_address():
    # enrichment booked +149.31 receive, but on-chain the ATA went 149.31 -> 0 (a send)
    keys = [WALLET, ATA, POOL]
    res = _tx(keys, pre=[(1, MSOL, None, 149312807711)], post=[(1, MSOL, None, 0)])
    assert reconcile.base_unit_delta(res, {ATA}, WALLET) == -149312807711


def test_inflow_delta_by_owner():
    other = "SomeOtherAcct1111111111111111111111111111111"
    keys = [WALLET, other, POOL]
    res = _tx(keys, pre=[(1, MSOL, WALLET, 0)], post=[(1, MSOL, WALLET, 1000000000)])
    assert reconcile.base_unit_delta(res, {ATA}, WALLET) == 1000000000


def test_ignores_other_mints_and_third_parties():
    usdc = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
    keys = [WALLET, ATA, POOL]
    res = _tx(keys,
              pre=[(1, MSOL, None, 50), (2, usdc, WALLET, 999), (3, MSOL, POOL, 7)],
              post=[(1, MSOL, None, 50), (2, usdc, WALLET, 0), (3, MSOL, POOL, 700)])
    assert reconcile.base_unit_delta(res, {ATA}, WALLET) == 0


def test_is_port_detects_program():
    assert reconcile.is_port(_tx([WALLET, ATA, PORT], [], [])) is True
    assert reconcile.is_port(_tx([WALLET, ATA, POOL], [], [])) is False


def test_csv_columns_cover_source_schema():
    needed = {"record_id", "tx_hash", "timestamp", "from_address", "to_address",
              "value", "tx_type", "token_symbol", "token_contract", "token_decimals"}
    assert needed.issubset(set(reconcile.CSV_COLUMNS))
