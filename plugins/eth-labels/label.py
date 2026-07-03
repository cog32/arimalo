#!/usr/bin/env python3
"""Ethereum Address Labels Plugin for Arimalo.

Scans ethereum transaction CSVs, looks up unknown addresses via
GitHub label databases and Etherscan, and adds payee and commodity
rules to _rules.json for human-readable transaction labeling.

Input (stdin JSON):
  - config.scan_path: source folder to scan (relative to sources/)
  - config.from_column: CSV column for sender address
  - config.to_column: CSV column for recipient address
  - config.commodity_column: CSV column for token contract addresses
  - secrets.etherscan_api_key: optional API key for higher rate limits
  - sources_dir: path to sources/ directory
  - data_dir: path to plugin's .data/ directory for caching

Output:
  - Merges new rules into sources/{scan_path}/_rules.json
  - Prints JSON status to stdout
"""

import csv
import json
import os
import re
import sys
import time
from urllib.request import Request, urlopen
from urllib.error import HTTPError, URLError

# GitHub raw URLs for bulk label databases
ETH_LABELS_URLS = [
    ("exchange", "https://raw.githubusercontent.com/dawsbot/eth-labels/master/src/mainnet/exchange/all.json"),
    ("token-contract", "https://raw.githubusercontent.com/dawsbot/eth-labels/master/src/mainnet/token-contract/all.json"),
]

# How old the cached GitHub data can be before we re-fetch (seconds)
GITHUB_CACHE_MAX_AGE = 7 * 24 * 3600  # 7 days

ETH_ADDRESS_RE = re.compile(r"^0x[0-9a-f]{40}$")

# Well-known DeFi protocol contracts (multi-chain — same address across EVM chains)
# Sources: Uniswap docs, Aave docs, protocol documentation
KNOWN_CONTRACTS = {
    # Uniswap V4
    "0x9a13f98cb987694c9f086b1f5eb990eea8264ec3": "Uniswap V4 Pool Manager",
    # Uniswap V3
    "0x1f98431c8ad98523631ae4a59f267346ea31f984": "Uniswap V3 Factory",
    "0xe592427a0aece92de3edee1f18e0157c05861564": "Uniswap V3 Router",
    "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45": "Uniswap V3 Router 2",
    "0xc36442b4a4522e871399cd717abdd847ab11fe88": "Uniswap V3 Position Manager",
    "0xb27308f9f90d607463bb33ea1bebb41c27ce5ab6": "Uniswap V3 Quoter",
    "0x61ffe014ba17989e743c5f6cb21bf9697530b21e": "Uniswap V3 Quoter V2",
    "0xe34139463ba50bd61336e0c446bd8c0867c6fe65": "Uniswap V3 Staker",
    # Uniswap Universal Router (chain-specific)
    "0x66a9893cc07d91d95644aedd05d03f95e1dba8af": "Uniswap Universal Router",
    "0x851116d9223fabed8e56c0e6b8ad0c31d98b3507": "Uniswap Universal Router",
    "0xa51afafe0263b40edaef0df8781ea9aa03e381a3": "Uniswap Universal Router",
    "0x6ff5693b99212da76ad316178a184ab56d299b43": "Uniswap Universal Router",
    "0x1095692a6237d83c6a72f3f5efedb9a670c49223": "Uniswap Universal Router",
    "0x94b75331ae8d42c1b61065089b7d48fe14aa73b7": "Uniswap Universal Router",
    "0x1906c1d672b88cd1b9ac7593301ca990f94eae07": "Uniswap Universal Router",
    # Uniswap V2
    "0x7a250d5630b4cf539739df2c5dacb4c659f2488d": "Uniswap V2 Router",
    "0x5c69bee701ef814a2b6a3edd4b1652cb9cc5aa6f": "Uniswap V2 Factory",
    # Permit2 (universal across chains)
    "0x000000000022d473030f116ddee9f6b43ac78ba3": "Permit2",
    # Aave V3 (same address across Optimism, Arbitrum, Polygon, Avalanche, etc.)
    "0x794a61358d6845594f94dc1db02a252b5b4814ad": "Aave V3 Pool",
    "0x87870bca3f3fd6335c3f4ce8392d69350b4fa4e2": "Aave V3 Pool",
    "0xa72636cbcaa8f5ff95b2cc47f3cdee83f3294a0b": "Aave V3 ACL Manager",
    "0x8145edddf43f50276641b55bd3ad95944510021e": "Aave V3 Pool Configurator",
    # Aave V2
    "0x7d2768de32b0b80b7a3454c06bdac94a69ddc7a9": "Aave V2 Lending Pool",
    # 1inch
    "0x1111111254eeb25477b68fb85ed929f73a960582": "1inch Router V5",
    "0x111111125421ca6dc452d289314280a0f8842a65": "1inch Router V6",
    # SushiSwap
    "0xd9e1ce17f2641f24ae83637ab66a2cca9c378b9f": "SushiSwap Router",
    # Curve
    "0xbebe89714da92ee2b29ea19b71de84cb10a4e3ae": "Curve Router",
    # Lido
    "0xae7ab96520de3a18e5e111b5eaab095312d7fe84": "Lido stETH",
    "0x7f39c581f595b53c5cb19bd0b3f8da6c935e2ca0": "Lido wstETH",
    # Optimism bridge
    "0x99c9fc46f92e8a1c0dec1b1747d010903e884be1": "Optimism Gateway",
    "0x4200000000000000000000000000000000000006": "WETH (L2)",
    "0x4200000000000000000000000000000000000042": "OP Token",
    "0x4200000000000000000000000000000000000010": "L2 Standard Bridge",
    # Arbitrum
    "0x912ce59144191c1204e64559fe8253a0e49e6548": "ARB Token",
    "0xaf88d065e77c8cc2239327c5edb3a432268e5831": "Circle USDC",
    "0x7ecfbaa8742fdf5756dac92fbc8b90a19b8815bf": "Arbitrum L2 Multicall",
    # WETH
    "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2": "WETH",
    "0x7ceb23fd6bc0add59e62ac25578270cff1b9f619": "WETH",
    "0x6a023ccd1ff6f2045c3309768ead9e68f978f6e1": "WETH",
    # Stablecoins (L2)
    "0x94b008aa00579c1307b0ef2c499ad98a8ce58e58": "Bridged USDT",
    "0xddafbb505ad214d7b80b1f830fccc89b60fb7a83": "USDC",
    "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913": "Circle USDC",
    "0xe9e7cea3dedca5984780bafc599bd69add087d56": "Binance BUSD",
    # Polygon
    "0x0000000000000000000000000000000000001010": "Polygon POL Token",
    "0xa0c68c638235ee32657e8f720a23cec1bfc6c9a8": "Polygon Bridge",
    # Gnosis/xDai
    "0x88ad09518695c6c3712ac10a214be5109a655671": "xDai Bridge",
    "0x6c76971f98945ae98dd7d4dfca8711ebea946ea6": "Lido wstETH",
    # LayerZero / Stargate
    "0x902f09715b6303d4173037652fa7377e5b98089e": "LayerZero Relayer V2",
    "0x4d73adb72bc3dd368966edd0f0b2148401a178e2": "LayerZero UltraLightNode V2",
    "0xdf0770df86a8034b3efef0a1bb3c889b8332ff56": "Stargate S*USDC",
    "0x2eb9ea9df49bebb97e7750f231a32129a89b82ee": "Stargate WidgetSwap",
    # Squid (cross-chain)
    "0xce16f69375520ab01377ce7b88f5ba8c48f8d666": "Squid Router",
    "0xea749fd6ba492dbc14c24fe8a3d08769229b896c": "Squid Multicall",
    # Hyperliquid
    "0x2df1c51e09aecf9cacb7bc98cb1742757f163df7": "Hyperliquid Deposit Bridge",
    # PancakeSwap (BSC)
    "0x10ed43c718714eb63d5aa57b78b54704e256024e": "PancakeSwap Router V2",
    # Lido
    "0xc3c7d422809852031b44ab29eec9f1eff2a58756": "Lido LDO Token",
    "0x4b3edb22952fb4a70140e39fb1add05a6b49622b": "Lido Early Stakers Airdrop",
    # KlimaDAO
    "0x4e78011ce80ee02d2c3e649fb657e45898257815": "KlimaDAO KLIMA Token",
    "0x4d70a031fc76da6a9bc0c922101a05fa95c3a227": "KlimaDAO Staking",
    # Wormhole bridges (multi-chain)
    "0x0b2402144bb366a632d14b83f244d2e0e21bd39c": "Wormhole Token Bridge",
    "0x3ee18b2214aff97000d974cf647e7c347e8fa585": "Wormhole Portal Token Bridge",
    "0xcafd2f0a35a4459fa40c0517e17e6fa2939441ca": "Wormhole TokenBridgeRelayer",
    "0xae8dc4a7438801ec4edc0b035eccccf3807f4cc1": "Wormhole TokenBridgeRelayer",
    # Celer
    "0x841ce48f9446c8e281d3f1444cb859b4a6d0738c": "Celer cBridge",
    # Uniswap (additional deployments from web search)
    "0x3fc91a3afd70395cd496c647d5a6cc9d4b2b7fad": "Uniswap Universal Router",
    "0x5e325eda8064b456f4781070c0738d849c824258": "Uniswap Universal Router",
    "0xf1f199342687a7d78bcc16fce79fa2665ef870e1": "Uniswap V3 USDC/USDT Pool",
    "0xc6962004f452be9203591991d15f6b388e09e8d0": "Uniswap V3 USDC/WETH Pool",
    # SushiSwap (multi-chain)
    "0x1b02da8cb0d097eb8d57a175b88c7d8b47997506": "SushiSwap Router",
    "0x1e67124681b402064cd0abe8ed1b5c79d2e02f64": "SushiSwap LP",
    "0x9803c7ae526049210a1725f7487af26fe2c24614": "SushiSwap LP",
    # Balancer
    "0xba12222222228d8ba445958a75a0704d566bf2c8": "Balancer Vault",
    # CoW Protocol
    "0x9008d19f58aabd9ed0d60971565aa8510560ab41": "CoW Protocol GPv2Settlement",
    # Morpho
    "0xbbbbbbbbbb9cc5e90e3b3af64bdaf62c37eeffcb": "Morpho Blue",
    # Aave (additional)
    "0x724dc807b04555b71ed48a6896b6f41593b8c637": "Aave aDPI Token V3",
    # PancakeSwap (additional)
    "0x0e09fabb73bd3ade0a17ecc321fd13a19e81ce82": "PancakeSwap CAKE Token",
    "0x0ed7e52944161450477ee417de9cd3a859b14fd0": "PancakeSwap CAKE/WBNB Pool",
    "0x58f876857a02d6762e0101bb5c46a8c1ed44dc16": "PancakeSwap WBNB/BUSD Pool",
    # Stablecoins (L2 bridged)
    "0x2791bca1f2de4661ed88a30c99a7a9449aa84174": "USDC.e",
    "0xff970a61a04b1ca14834a43f5de4533ebddb5cc8": "USDC (Bridged)",
    "0x7f5c764cbc14f9669b88837ca1490cca17c31607": "USDC.e",
    "0x0b2c639c533813f4aa9d7837caf62653d097ff85": "Circle USDC",
    "0xc2132d05d31c914a87c6611c10748aeb04b58e8f": "USDT",
    "0x8f3cf7ad23cd3cadbd9735aff958023239c6a063": "DAI",
    # WETH (additional)
    "0x82af49447d8a07e3bd95bd0d56f35241523fbab1": "WETH",
    # Other tokens
    "0x6985884c4392d348587b19cb9eaaf157f13271cd": "LayerZero ZRO Token",
    "0xb0897686c545045afc77cf20ec7a532e3120e0f1": "Chainlink LINK Token",
    "0x532f27101965dd16442e59d40670faf5ebb142e4": "BRETT Token",
    # Toucan / KlimaDAO (additional)
    "0x2f800db0fdb5223b3c3f354886d907a671414a7f": "Toucan BCT Token",
    "0x25d28a24ceb6f81015bb0b2007d795acac411b4d": "KlimaDAO Staking",
    "0xb0c22d8d350c67420f06f48936654f567c73e8c8": "KlimaDAO sKLIMA",
    # Gains Network
    "0x6b8d3c08072a020ac065c467ce922e3a36d3f9d6": "Gains Network GNS Staking",
    # Sommelier
    "0xc47bb288178ea40bf520a91826a3dee9e0dbfa4c": "Sommelier Cellar",
    # Arbitrum infrastructure
    "0x6c411ad3e74de3e7bd422b94a27770f5b86c623b": "Arbitrum L2 WETH Gateway",
    "0xf3fc178157fb3c87548baa86f9d24ba38e649b58": "Arbitrum DAO Treasury",
    "0x67a24ce4321ab3af51c2d0a4801c3e111d88c9d9": "Arbitrum Token Distributor",
    # BSC infrastructure
    "0x0000000000000000000000000000000000001004": "BSC Token Hub",
    # Farcaster
    "0x00000000fcce7f938e7ae6d3c335bd6a1a7c593d": "Farcaster Storage Registry",
    "0x00000000fc25870c6ed6b6c7e41fb078b7656f69": "Farcaster Id Gateway",
    # Lido (additional)
    "0xdc24316b9ae028f1497c275eb9192a3ea0f67022": "Lido Curve stETH/ETH Pool",
    "0x48f300bd3c52c7da6aabde4b683deb27d38b9abb": "Lido Finance Multisig",
    # Venice
    "0xacfe6019ed1a7dc6f7b508c02d1b04ec88cc21bf": "Venice VVV Token",
    "0x321b7ff75154472b18edb199033ff4d116f340ff": "Venice VVV Treasury",
    # Utilities
    "0xd152f549545093347a162dce210e7293f1452150": "Disperse.app",
    "0x09350f89e2d7b6e96ba730783c2d76137b045fef": "Gaslite Drop",
    "0x032b17633c956c10845643f0bf9ea7c16a3cfb62": "USDC Exchange Proxy",
}


def load_json_file(path):
    """Load a JSON file, returning None if it doesn't exist."""
    if not os.path.exists(path):
        return None
    with open(path) as f:
        return json.load(f)


def save_json_file(path, data):
    """Save data as JSON with indentation."""
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        json.dump(data, f, indent=2)
        f.write("\n")


def resolve_scan_path(sources_dir, scan_path):
    """Resolve scan_path to an actual folder under sources/.

    Supports:
      - Literal paths: "richard/crypto/wallet/ethereum" (used as-is if it exists)
      - Account-style paths: "assets:ethereum" → searches for {owner}/ethereum
        or {owner}-ethereum under sources/
    """
    # Literal path — use directly if it exists
    if os.path.isdir(os.path.join(sources_dir, scan_path)):
        return scan_path

    # Account-style: strip type prefix, convert : to /
    path = scan_path
    for prefix in ("assets:", "liabilities:", "income:", "expenses:", "equity:"):
        if path.startswith(prefix):
            path = path[len(prefix):]
            break
    suffix = path.replace(":", "/")

    try:
        entries = sorted(os.listdir(sources_dir))
    except OSError:
        return None

    for entry in entries:
        full = os.path.join(sources_dir, entry)
        if not os.path.isdir(full):
            continue
        # Nested: {owner}/{suffix}
        if os.path.isdir(os.path.join(full, suffix)):
            return os.path.join(entry, suffix)
        # Flat: {owner}-{suffix} (single-segment only)
        if "/" not in suffix and entry.endswith(f"-{suffix}"):
            return entry

    return None


def scan_csvs_for_addresses(sources_dir, scan_path, columns):
    """Walk CSVs under scan_path and extract unique addresses from given columns."""
    scan_dir = os.path.join(sources_dir, scan_path)
    addresses = set()

    if not os.path.isdir(scan_dir):
        return addresses

    for root, _dirs, files in os.walk(scan_dir):
        for fname in files:
            if not fname.lower().endswith(".csv"):
                continue
            fpath = os.path.join(root, fname)
            try:
                with open(fpath, newline="", encoding="utf-8-sig") as f:
                    reader = csv.DictReader(f)
                    for row in reader:
                        for col in columns:
                            addr = row.get(col, "").strip().lower()
                            if addr and ETH_ADDRESS_RE.match(addr):
                                addresses.add(addr)
            except (csv.Error, UnicodeDecodeError, KeyError):
                continue

    return addresses


def extract_covered_addresses(rules):
    """Extract addresses already covered by existing rules, keyed by rule type.

    Returns (payee_covered, commodity_covered) as two sets of addresses.
    """
    payee_covered = set()
    commodity_covered = set()
    for rule in rules:
        pattern = rule.get("pattern", "").lower()
        match = re.search(r"(0x[0-9a-f]{40})", pattern)
        if not match:
            continue
        addr = match.group(1)
        if rule.get("match_field") == "commodity":
            commodity_covered.add(addr)
        else:
            payee_covered.add(addr)
    return payee_covered, commodity_covered


def fetch_github_labels(data_dir):
    """Fetch or load cached GitHub address labels. Returns {address: name}."""
    cache_path = os.path.join(data_dir, "eth_labels_github.json")
    labels = {}

    # Check cache freshness
    if os.path.exists(cache_path):
        age = time.time() - os.path.getmtime(cache_path)
        if age < GITHUB_CACHE_MAX_AGE:
            cached = load_json_file(cache_path)
            if cached:
                return cached

    # Fetch from all GitHub label URLs
    for _name, url in ETH_LABELS_URLS:
        try:
            req = Request(url, headers={"Accept": "application/json"})
            with urlopen(req, timeout=30) as resp:
                data = json.loads(resp.read())
        except (HTTPError, URLError, json.JSONDecodeError):
            continue

        for entry in data:
            addr = entry.get("address", "").strip().lower()
            name = entry.get("nameTag", "").strip()
            if addr and name:
                labels[addr] = name

    if not labels:
        # Fall back to cache if all fetches fail
        cached = load_json_file(cache_path)
        return cached if cached else {}

    save_json_file(cache_path, labels)
    return labels


def fetch_etherscan_label(address, api_key=None):
    """Look up a single address on Etherscan. Returns contract name or None."""
    params = f"module=contract&action=getsourcecode&address={address}"
    if api_key:
        params += f"&apikey={api_key}"

    url = f"https://api.etherscan.io/api?{params}"
    try:
        req = Request(url, headers={"Accept": "application/json"})
        with urlopen(req, timeout=15) as resp:
            data = json.loads(resp.read())
    except (HTTPError, URLError, json.JSONDecodeError):
        return None

    if data.get("status") != "1":
        return None

    results = data.get("result", [])
    if not results or not isinstance(results, list):
        return None

    contract_name = results[0].get("ContractName", "").strip()
    if contract_name:
        return contract_name

    return None


def lookup_addresses(addresses, data_dir, api_key=None):
    """Look up addresses via GitHub labels then Etherscan. Returns {addr: name}."""
    labels = {}
    warnings = []

    # Load persistent label cache
    cache_path = os.path.join(data_dir, "label_cache.json")
    cache = load_json_file(cache_path) or {}

    # Check cache first
    uncached = set()
    for addr in addresses:
        if addr in cache:
            name = cache[addr]
            if name:  # Skip addresses we looked up but found nothing
                labels[addr] = name
        else:
            uncached.add(addr)

    if not uncached:
        return labels, warnings

    # 1. Known contracts (hardcoded, multi-chain DeFi protocols)
    # 2. GitHub bulk labels (exchange + token-contract lists)
    github_labels = fetch_github_labels(data_dir)
    still_unknown = set()
    for addr in uncached:
        name = KNOWN_CONTRACTS.get(addr) or github_labels.get(addr)
        if name:
            labels[addr] = name
            cache[addr] = name
        else:
            still_unknown.add(addr)

    # Etherscan API for remaining (rate limited)
    etherscan_count = 0
    for addr in still_unknown:
        name = fetch_etherscan_label(addr, api_key)
        if name:
            labels[addr] = name
            cache[addr] = name
        else:
            cache[addr] = ""  # Mark as looked up but not found

        etherscan_count += 1

        # Rate limit: 5 req/sec free tier
        if not api_key:
            time.sleep(0.25)
        else:
            time.sleep(0.1)

        # Safety limit to avoid burning through API quota
        if etherscan_count >= 200:
            remaining = len(still_unknown) - etherscan_count
            if remaining > 0:
                warnings.append(f"Etherscan lookup stopped after 200 requests, {remaining} addresses remaining")
            break

    # Persist cache
    save_json_file(cache_path, cache)
    return labels, warnings


def make_rule(address, name):
    """Create a payee rule dict for a labeled address."""
    short = address[:10]
    return {
        "id": f"auto-eth-{short}",
        "pattern": f"*{address}*",
        "payee": name,
        "comment": "auto:eth-labels",
    }


def make_commodity_rule(address, token_symbol):
    """Create a commodity-rename rule for a token contract address."""
    short = address[:10]
    return {
        "id": f"auto-eth-token-{short}",
        "pattern": f"*{address}*",
        "match_field": "commodity",
        "commodity": token_symbol,
        "comment": "auto:eth-labels",
    }


def backup_rules(rules_path):
    """Create a timestamped backup of _rules.json before modifying it."""
    if not os.path.exists(rules_path):
        return
    from datetime import datetime
    ts = datetime.now().strftime("%Y%m%d_%H%M%S")
    backup_path = rules_path + f".backup_{ts}"
    import shutil
    shutil.copy2(rules_path, backup_path)
    # Keep only the 5 most recent backups
    import glob
    backups = sorted(glob.glob(rules_path + ".backup_*"))
    for old in backups[:-5]:
        os.remove(old)


def merge_rules(rules_path, new_rules):
    """Merge new rules into _rules.json, inserting before the catch-all."""
    backup_rules(rules_path)

    data = load_json_file(rules_path)
    if data is None:
        data = {"rules": []}

    existing = data.get("rules", [])

    # Deduplicate: skip new rules whose id already exists
    existing_ids = {r.get("id") for r in existing}
    new_rules = [r for r in new_rules if r.get("id") not in existing_ids]

    # Find catch-all rule index (pattern == "*" with no address)
    catchall_idx = len(existing)
    for i, rule in enumerate(existing):
        if rule.get("pattern") == "*" and not re.search(r"0x[0-9a-f]", rule.get("pattern", "")):
            catchall_idx = i
            break

    # Insert new rules before catch-all
    for rule in new_rules:
        existing.insert(catchall_idx, rule)
        catchall_idx += 1

    data["rules"] = existing
    save_json_file(rules_path, data)


def main():
    ctx = json.load(sys.stdin)
    config = ctx.get("config", {})
    secrets = ctx.get("secrets", {})
    sources_dir = ctx["sources_dir"]
    data_dir = ctx["data_dir"]

    raw_scan_path = config.get("scan_path", "assets:crypto:wallet:ethereum")
    from_col = config.get("from_column", "from_address")
    to_col = config.get("to_column", "to_address")
    commodity_col = config.get("commodity_column", "")
    api_key = secrets.get("etherscan_api_key") or None

    files_written = []
    warnings = []

    # Resolve account-style path to actual folder
    scan_path = resolve_scan_path(sources_dir, raw_scan_path)
    if scan_path is None:
        suffix = raw_scan_path
        for prefix in ("assets:", "liabilities:", "income:", "expenses:", "equity:"):
            if suffix.startswith(prefix):
                suffix = suffix[len(prefix):]
                break
        suffix = suffix.replace(":", "/")
        print(json.dumps({
            "files_written": [],
            "records_fetched": 0,
            "warnings": [
                f"No source folder found matching '{raw_scan_path}'. "
                f"Expected a folder like sources/{{owner}}/{suffix} "
                f"(e.g., sources/richard/{suffix}/)."
            ],
        }))
        sys.exit(1)

    # 1. Scan CSVs for counterparty addresses
    payee_addresses = scan_csvs_for_addresses(sources_dir, scan_path, [from_col, to_col])

    # 1b. Scan CSVs for token contract addresses (commodity column)
    commodity_addresses = set()
    if commodity_col:
        commodity_addresses = scan_csvs_for_addresses(sources_dir, scan_path, [commodity_col])

    all_addresses = payee_addresses | commodity_addresses
    if not all_addresses:
        print(json.dumps({"files_written": [], "records_fetched": 0, "warnings": ["No CSV files found or no addresses extracted"]}))
        return

    # 2. Load existing rules and find uncovered addresses
    rules_path = os.path.join(sources_dir, scan_path, "_rules.json")
    rules_data = load_json_file(rules_path)
    existing_rules = rules_data.get("rules", []) if rules_data else []
    payee_covered, commodity_covered = extract_covered_addresses(existing_rules)

    new_payee = payee_addresses - payee_covered
    new_commodity = commodity_addresses - commodity_covered
    new_for_lookup = new_payee | new_commodity

    if not new_for_lookup:
        print(json.dumps({"files_written": [], "records_fetched": 0, "warnings": ["All addresses already have rules"]}))
        return

    # 3. Look up labels for all new addresses
    labels, lookup_warnings = lookup_addresses(new_for_lookup, data_dir, api_key)
    warnings.extend(lookup_warnings)

    # 4. Generate rules — payee rules for counterparty addresses, commodity rules for token addresses
    new_rules = []
    for addr in sorted(labels.keys()):
        name = labels[addr]
        if addr in new_payee:
            new_rules.append(make_rule(addr, name))
        if addr in new_commodity:
            new_rules.append(make_commodity_rule(addr, name))

    if new_rules:
        merge_rules(rules_path, new_rules)
        rel_path = os.path.join(scan_path, "_rules.json")
        files_written.append(rel_path)

    unlabeled = len(new_for_lookup) - len(labels)
    if unlabeled > 0:
        warnings.append(f"{unlabeled} addresses could not be labeled")

    result = {
        "files_written": files_written,
        "records_fetched": len(labels),
        "warnings": warnings,
    }
    print(json.dumps(result))


if __name__ == "__main__":
    main()
