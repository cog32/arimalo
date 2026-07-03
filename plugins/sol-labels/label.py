#!/usr/bin/env python3
"""Solana Address Labels Plugin for Arimalo.

Scans Solana transaction CSVs, looks up unknown addresses via
GitHub token lists and Solana RPC, and adds payee and commodity
rules to _rules.json for human-readable transaction labeling.

Lookup sources (in order):
  1. GitHub token lists (solana-labs, jup-ag) — cached 7 days
  2. Well-known Solana programs (hardcoded)
  3. Solana RPC getMultipleAccounts — identifies SPL token accounts
     and resolves their mint to a token symbol via the token lists

Input (stdin JSON):
  - config.scan_path: account path or folder to scan
  - config.from_column: CSV column for sender address
  - config.to_column: CSV column for recipient address
  - config.commodity_column: CSV column for token mint addresses
  - secrets.solana_rpc_url: optional RPC endpoint override
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

# GitHub token list URLs
TOKEN_LIST_URLS = [
    ("solana-labs", "https://raw.githubusercontent.com/solana-labs/token-list/main/src/tokens/solana.tokenlist.json"),
    ("jup-ag", "https://raw.githubusercontent.com/jup-ag/token-list/main/validated-tokens.csv"),
]

SOLSCAN_LABELS_URL = "https://raw.githubusercontent.com/solscanofficial/labels/main/labels.json"

DEFAULT_RPC_URL = "https://api.mainnet-beta.solana.com"

GITHUB_CACHE_MAX_AGE = 7 * 24 * 3600  # 7 days

# Solana addresses are base58-encoded, 32-44 characters
SOL_ADDRESS_RE = re.compile(r"^[1-9A-HJ-NP-Za-km-z]{32,44}$")

# Known exchange wallet addresses
# Sources: Binance Proof-of-Reserves API, DefiLlama Adapters
KNOWN_EXCHANGES = {
    # Binance (from https://www.binance.com/bapi/apex/v1/public/apex/market/por/address)
    "28nYGHJyUVcVdxZtzKByBXEj127XnrUkrE3VaGuWj1ZU": "Binance",
    "2ojv9BAiHUrvsm9gxDe7fJSzbNZSJcxZvf8dqmWGHG8S": "Binance",
    "38xCLm9kSExfGU1GdyVuX4vop7SZns9kU2mQyTmmMdUP": "Binance",
    "3gd3dqgtJ4jWfBfLYTX67DALFetjc5iS72sCgRhCkW2u": "Binance",
    "3yFwqXBfZY4jBVUafQ1YEXw189y2dN3V5KQq9uzBDy1E": "Binance",
    "5SDrsMNTYdhmApjfqYHDvjoW92f2S42vcc7zNDVcQ9Ej": "Binance",
    "5tzFkiKscXHK5ZXCGbXZxdw7gTjjD1mBwuoFbhUvuAi9": "Binance",
    "6QJzieMYfp7yr3EdrePaQoG3Ghxs2wM98xSLRu8Xh56U": "Binance",
    "6oCa9Tz8VXVp63WiFyruE5PD6yXz3pCsv6oGzUGvg9TP": "Binance",
    "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM": "Binance",
    "AEkGD1y3LzaXxrh4xYqLkWu9MSYkY1knr2nRt1YPULsx": "Binance",
    "BZ3kabSsMzbuJUguYxtmkRtzw7ACqw1DUMH8PcbvXiUr": "Binance",
    "EPauhjQjjTBCpeBtszS3xGRASLpEJFM1cspSiFRXZa9Z": "Binance",
    "EtwjSV65xPjZxDmmoNTw78gdYxLa1ayqVo4kXGsjhMiA": "Binance",
    "ExFUyu3f5C9UW3zLZybVcXv156X2g5AxdZCZJvCh23w4": "Binance",
    "G9RCBaYb8aBRxoe8QBC2ucGrVqjuZFysRhY8d56cnNT1": "Binance",
    "GBrURzmtWujJRTA3Bkvo7ZgWuZYLMMwPCwre7BejJXnK": "Binance",
    "GK35nWN6ZHSGZrRTf8kTQd8RkFCighChPEb41XwSFVAC": "Binance",
    "H8BgJgae6qhMtf7BM2JtddywSQt11WdxHHxkGLNX5hss": "Binance",
    "HXsKP7wrBWaQ8T2Vtjry3Nj3oUgwYcqq9vrHDM12G664": "Binance",
    "c5f9zfpkKMD9N8uLqJcFeJAAz7v12vDMnup9Y6EeQkk": "Binance",
    "2nz5URgDe4yKwDiWXRunVYqLnpWLcW6M4C7sX4Qdtes8": "Binance",
    "FmywK9jTaHci6kmAyZ9pVwmhf97jWaun4j86zR6KnYTr": "Binance",
    # Bybit (from DefiLlama)
    "AC5RDfQFmDS1deWZos921JfqscXdByf8BKHs5ACWjtW2": "Bybit",
    "42brAgAVNzMBP7aaktPvAmBSPEkehnFQejiZc53EpJFd": "Bybit",
    "Hvkm4H2Ta3L3ssWbB5jeC4kpEJDuZnZqapAXp1V7UHEw": "Bybit",
    "CqQ6AX1fiFfHKKY3saGzT5pgbkLwfLVrrAKpFhUG38oe": "Bybit",
    "FeNayVKekV9FJzhD7ycTd6sbKEyzt9CRCiqxw5nr41yR": "Bybit",
    "32cT9eAwkEvAk631rUcUAbXVFPg21DaAXzGiz9AqHTVE": "Bybit",
    "9Z7S8vCj6nDbK9t4m4AU3vZpKm4UufHAwpmRYyKgZf7r": "Bybit",
    "BunaYnktTigcU1ovzVt9dG7NMv2gW5VX7MBfSS8J38s2": "Bybit",
    "AaFm2LPX8NUKXe64JaxcRNUc8QPGYCxrPG1HjHcTTGAK": "Bybit",
    "i9XvhQqBCTQapqaFKPDuCbtPYMCwELmX8VTCsDhRG7d": "Bybit",
    "7ReR6syi6gr7qUrKCL1FB9VFzGhVgHwLJ8wtfNtH9Mv4": "Bybit",
    "iGdFcQoyR2MwbXMHQskhmNsqddZ6rinsipHc4TNSdwu": "Bybit",
    "9ZifroknFoYu4r6DUk6nYoJiUQnEyyoUyeAwjXbPoL2x": "Bybit",
    "2qo8jvuc49pFmTjmUHLiARSV6ppPTaE7gw27ZJ6DnNZy": "Bybit",
    "CK8i4zFXkDE2KWfyg7g9S748r6mwxajbcKcyGhQMR3qQ": "Bybit",
    "Gem2VAypSg7Ai7vjDKPTtqFahpoQWkfgVkyzx3rPoTka": "Bybit",
    "5LZkATrLwHYCQj2YuVbjjgsDZzBk6YfL4pFQRJmtboT2": "Bybit",
    "7cAui6ADtxLnpRr2wYvwJWTkzwgmVF2LYKnjKTLx4xR8": "Bybit",
    "F6iEboP31qwgewVfonux3bdgWonJQHLgsky5uztmHDng": "Bybit",
    "4SQQkqaouajAj9HaALAE1GUYhrkis96AhGr4gfnSDiUA": "Bybit",
    "giPVBame6yenRbuYdsjXwku6TS3FaEkw7Nwy2VHiq8X": "Bybit",
    "AL63c8ZNNAHZbAub6v1EGqQLt1fwyGQbVnFudw8xTPa5": "Bybit",
    "8vHZDc2VTtynUizstoJHtXaH5VNKzynySMLZRZ8rj8Vc": "Bybit",
    "BEyAq6ZgDkBwms5tNnYEjjvxsT7RaL8HVjxZvdu7XnJJ": "Bybit",
    "FCgSWpNqaTvYydABHVCBxs7MRpgPBemhVDEYUcudj3Zv": "Bybit",
    "CSSJFgoeqidqVtHKSNP7i7s6WX8APHfH2kYGdLV195Jb": "Bybit",
    "FQEmsV5A6jZdmb3KuP3YBLmHfFNoKuoWddBGMQuW1M9f": "Bybit",
    "CMivUnnbDHxLq9ChV1bSuiQE5ycZf6JVvFFDePMHhHYK": "Bybit",
    "6fJxHzqAvnmsfb61Hkh7dNtUqLCaZdBtCvYq5X8BqHYj": "Bybit",
    "6coXmZ8FRDRcuGQnoa1wH9GTrzJ7d1cD7NhiVDdMKt7F": "Bybit",
    "98Nfcvz1yFgwimcmQNBpqob4bzSBqxCsCynigyFdDaZV": "Bybit",
    "EVTzXcENdy55DpysRQ5mptcxfzZagucJdEb8jMEKQAxz": "Bybit",
    # OKX (from DefiLlama)
    "CeGyfZdtbjzC5FeVXCBdYE1v397yxxqYgmMcUwtouJUu": "OKX",
    "6Attw9NcFspCAreufvQ3mW8aqXEk6MDceeoC8obw45cJ": "OKX",
    "CE8joA143dBjsCFTeBxBs3kNGNU87WH6Q7GMMfxPzSr2": "OKX",
    "FefAbVt2EgXMGxeJb6sB1k5pQJhiGPxw4mB1zeUohZLk": "OKX",
    "BEaiMhcc4Kao7B4hoq6r9m8neUpfimUMwhzugKxtkZw6": "OKX",
    "5VCwKtCXgCJ6kit5FybXjvriW3xELsFDhYrPSqtJNmcD": "OKX",
    "APJmXrtC9TUAg5gcjbcsVUiVzeDv85xLZRLad1GiQTNE": "OKX",
    "5kiqH41rz2wPoNH5FuDJ3x8dB3EEe3XGoSWy2485tAp9": "OKX",
    "8dUc88Nss8uhqzzFvUQhepkwZaVrfzpoCfKjCXGkdzAG": "OKX",
    "44DwSkZbY2PVo9wfFP21D7HG7xf5rw8QaF6xSLGSfm7P": "OKX",
    "945SJTwsBSqwEEgtKMnhcLDnJowP6YpJUTEykMVK5k6q": "OKX",
    "9un5wqE3q4oCjyrDkwsdD48KteCJitQX5978Vh7KKxHo": "OKX",
    "FJS2rTjAQmFFRmVqtnkceGvLJrMwUPgCUfLjciuMYkSC": "OKX",
    "7fGw3UURsxk1szQ4buxQyEkiF4P6z7vx7sN1MHEguTJg": "OKX",
    "BhJrLQEyFyrcf1746pHATzAZQpC1uk3SJf2AHHK7LW3K": "OKX",
    "5URmUviWsAtcciFXqgM7f1jmhvBULdQDT47dqDkrJUi": "OKX",
    "6NnzUC7mrM7ZgKdcMq1jPAdqh9TMgdsaVR1DNq6kce3A": "OKX",
    "7o9ukGhWvc71yYcjinKnrxMn2kFMyQD1iCG3romrzg7r": "OKX",
    "7CtzEcGeYpMfR1aEi1Gbt8GRHDPH9uDB6GNTsBABqSmo": "OKX",
    "GcFZHQVJ2icPGLrA9qPLq88b7eqBXSUcdL1utUtUfcSD": "OKX",
    "BWZqi67kXsvf5crjd19Fb3gmf3DCipGE8eWWJT2YHDXQ": "OKX",
    "CGZAHxBannZzYK3rzVdG1e3uNjMxrmaXDhLEZxQBbueq": "OKX",
    "EBVigEhxvkWUwreG98kTh5FbZEjnXTXC5ANZtLz4YpHQ": "OKX",
    "HnLezgQkNVWMp2AV6mcHM1Ljst32kfsoVnYUkvXuQpjg": "OKX",
    "is6MTRHEgyFLNTfYcuV4QBWLjrZBfmhVNYR6ccgr8KV": "OKX",
    "FmpbkPgxjoYBJ6ac6YMhZ1FycCwuG8Zs2epxgsGFYvf6": "OKX",
    "C68a6RCGLiPskbPYtAcsCjhG8tfTWYcoB4JjCrXFdqyo": "OKX",
    "AEZoku1fLfUz5JYJ3kJ5YVdf3QT1T4RwdggGbuR8Eakd": "OKX",
    "CBEADkb8TZAXHjVE3zwad4L995GZE7rJcacJ7asebkVG": "OKX",
    # KuCoin (from DefiLlama)
    "BmFdpraQhkiDQE6SnfG5omcA1VwzqfXrwtNYBwWTymy6": "KuCoin",
    "EkUy8BB574iEVAQE9dywEiMhp9f2mFBuFu6TBKAkQxFY": "KuCoin",
    "HVh6wHNBAsG3pq1Bj5oCzRjoWKVogEDHwUHkRz3ekFgt": "KuCoin",
    "3VRJ7acLUS9RYLLgRMenM6BqBVVQpY7xFXa7jKDdYw6M": "KuCoin",
    "EEnu7YoRDnZ1EHHggnnQoFk7fUjNQrXQw33vgFWwMJn": "KuCoin",
    "57vSaRTqN9iXaemgh4AoDsZ63mcaoshfMK8NP3Z5QNbs": "KuCoin",
    "EfwJn8cXCYhcGrsavxWSDbUFHPrCK9gvdCr6AVywFBPg": "KuCoin",
    "DBmae92YTQKLsNzXcPscxiwPqMcz9stQr2prB5ZCAHPd": "KuCoin",
    "FZ1t8TZtx7VSCQdBsxvFJiezj9paUBF6Ub7RKA2eTGyE": "KuCoin",
    "GL8T72PKygWaYrLKtSteN9UUvYYnuC8azNDunk4eaqqQ": "KuCoin",
    "6BhBoBB47wSGjK5uzcGWNcTf2oNRPQNuv6GVdkNyj9PB": "KuCoin",
    "7gQ1CfjdysJkSEVSDXNjJHnzrqvP2zQYQDJHJP67o1bb": "KuCoin",
    "AGVhmrhDi3RKLu9nxnRqp3CUpaG3SVeYXWkWcygHAk8N": "KuCoin",
    "CAxKWUpSbsNsWu2gEFjed64jrNxiNYfRMVEMahshHotb": "KuCoin",
    "FPRFLszBJFHCThF2yWvkGkfvtwyFPRGX9MFHnDGP6UYp": "KuCoin",
    # Gate.io (from DefiLlama)
    "u6PJ8DtQuPFnfmwHbGFULQ4u4EgjDiyYKjVEsynXq2w": "Gate.io",
    "HiRpdAZifEsZGdzQ5Xo5wcnaH3D2Jj9SoNsUzcYNK78J": "Gate.io",
    "G9XFfWz6adb9wFDKN2v7HfmJDgAc2hirrTwBmca4w26C": "Gate.io",
    "Egf5D8NKBivJavLKmCssE93J7X6fKvEPQwFTWLZUnaSN": "Gate.io",
    # HTX/Huobi (from DefiLlama)
    "88xTWZMeKfiTgbfEmPLdsUCQcZinwUfk25EBQZ21XMAZ": "HTX",
    "BY4StcU9Y2BpgH8quZzorg31EGE4L1rjomN8FNsCBEcx": "HTX",
    "8NBEbxLknGv5aRYefFrW2qFXoDZyi9fSHJNiJRvEcMBE": "HTX",
    "5bJcc9eb2XE7mqcET2xDuAdMGuXWybb4YPmAHLjKLhQG": "HTX",
    # Bitget (from DefiLlama)
    "A77HErqtfN1hLLpvZ9pCtu66FEtM8BveoaKbbMoZ4RiR": "Bitget",
    "3bLkLrRvkwHMrqyoCaDSCn6bZnpfJCVsHxcmznUwB1p5": "Bitget",
    "42zAGwv37eZFwwcExfCAV9oSw2kNQX3aBxsbM6zvQECM": "Bitget",
    "YiZeibU6zzEHyKiSTjygXUPkMktKj9a3DCAcWmZ4XjF": "Bitget",
    "AvLGED7RBzYv4AuvkgFSCFMqyB2WjUff7TVKVEv5MjMs": "Bitget",
    "48Zo7g9SReCWmNtCvr2es4H9CLCRQHrSND2Wzi61sCsQ": "Bitget",
    "57WSBnNTC2MaqpY6NWLdNjhrELced4jSGV2hLSpjzct9": "Bitget",
    "DP1FqoBnE23QNNz4LpT9FCQvETdJN4nph5c11NiinrGg": "Bitget",
    "AyhsmFptkM251V1AoH2gf8d4QUnxUkkmaDqFfFwBwGni": "Bitget",
    "4S8C1yrRZmJYPzCqzEVjZYf6qCYWFoF7hWLRzssTCotX": "Bitget",
    "7TWnq4WeYcwQWBCwKeEX2Q9xqVtthPGkB7adNvueuVuh": "Bitget",
    # Coinbase (from Solscan labels)
    "H8sMJSCQxfKiFTCfDR3DUMLPwcRbM61LGFJ8N4dK3WjS": "Coinbase",
    "GJRs4FwHtemZ5ZE9x3FNvJ8TMwitKTh21yxdRPqn7npE": "Coinbase",
    "D89hHJT5Aqyx1trP6EnGY9jJUB3whgnq3aUvvCqedvzf": "Coinbase",
    # Crypto.com (from Solscan labels)
    "AobVSwdW9BbpMdJvTqeCN4hPAmh4rHm7vwLnQ5ATSyrS": "Crypto.com",
    # Kraken (from Solscan)
    "FWznbcNXWQuHTawe9RxvQ2LdCENssh12dsznf4RiouN5": "Kraken",
    # CEX.IO (from official blog)
    "2QwUbEACJ3ppwfyH19QCSVvNrRzfuK5mNVNDsDMsZKMh": "CEX.IO",
    "DUru5ZfCdCnjPFuY7NPniV3hhZqNJLgn2sBZJGaMc2Sj": "CEX.IO",
    "CGRNicgpirZd3unSzn1Y34k7w31rQftTbaJwEuQu31XP": "CEX.IO",
    # Robinhood (from Solscan labels via browser lookup)
    "8Tp9fFkZ2KcRBLYDTUNXo98Ez6ojGb6MZEPXfGDdeBzG": "Robinhood",
    "6brjeZNfSpqjWoo16z1YbywKguAruXZhNz9bJMVZE8pD": "Robinhood",
    "9FtGm6hJULCpA8An4sFg5ysHUExDZBtMeDCxsYnTnWh5": "Robinhood",
    "AeBwztwXScyNNuQCEdhS54wttRQrw3Nj1UtqddzB4C7b": "Robinhood",
    "4xLpwxgYuPwPvtQjE94RLS4WZ4aD8NJYYKr2AJk99Qdg": "Robinhood",
    # Kraken (from Solscan labels via browser lookup)
    "CDhUgGEiUxx1aTbnoiSAKcmBhnGUFRQ6AMzuLQRD5VFZ": "Kraken",
    "EFE3j1pcSP1paUzA86zW7989ZjsFP2J7ginyUqo4ewqR": "Kraken",
    "E2RvJg2myWpKcbkhBuF81gfhJwqRYkFFASoKXisvjdio": "Kraken",
    "6LY1JzAFVZsP2a2xKrtU6znQMQ5h4i7tocWdgrkZzkzF": "Kraken",
    "krakeNd6ednDPEXxHAmoBs1qKVM8kLg79PvWF2mhXV1": "Kraken",
    "9cNE6KBg2Xmf34FPMMvzDF8yUHMrgLRzBV3vD7b1JnUS": "Kraken",
    "F7RkX6Y1qTfBqoX5oHoZEgrG1Dpy55UZ3GfWwPbM58nQ": "Kraken",
    "CAo1dCGYrB6NhHh5xb1cGjUiu86iyCfMTENxgHumSve4": "Kraken",
    "DPnnYagxg6EaDT9t7Dwm8S8fvqWqRqjJV6a25qwjyqb2": "Kraken",
    "HamtXGnEQMGJubM2mkQY7LBzPLiqeD4HrAsU753wgkW2": "Kraken",
    # FTX (from Solscan labels via browser lookup)
    "96h9hxB8ZtRYxJPbWVdqzr7zyGDMZHKnV2R2A1B9Quw5": "FTX",
}

SYSTEM_PROGRAM = "11111111111111111111111111111111"

# Well-known Solana programs and system accounts
# Source: https://github.com/solana-foundation/explorer/blob/master/app/utils/programs.ts
KNOWN_PROGRAMS = {
    # Native built-ins
    "11111111111111111111111111111111": "System Program",
    "Vote111111111111111111111111111111111111111": "Vote Program",
    "Stake11111111111111111111111111111111111111": "Stake Program",
    "ComputeBudget111111111111111111111111111111": "Compute Budget",
    "Config1111111111111111111111111111111111111": "Config Program",
    "AddressLookupTab1e1111111111111111111111111": "Address Lookup Table",
    # Native precompiles
    "KeccakSecp256k11111111111111111111111111111": "Secp256k1 SigVerify",
    "Ed25519SigVerify111111111111111111111111111": "Ed25519 SigVerify",
    # Loaders
    "BPFLoader1111111111111111111111111111111111": "BPF Loader",
    "BPFLoader2111111111111111111111111111111111": "BPF Loader 2",
    "BPFLoaderUpgradeab1e11111111111111111111111": "BPF Upgradeable Loader",
    "NativeLoader1111111111111111111111111111111": "Native Loader",
    # SPL programs
    "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA": "Token Program",
    "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb": "Token-2022",
    "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL": "Associated Token",
    "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr": "Memo Program",
    "Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo": "Memo Program v1",
    "namesLPneVptA9Z5rqUDD9tMTWEJwofgaYwp8cawRkX": "Name Service",
    "SPoo1Ku8WFXoNDMHPsrGSTSG1Y47rzgn41SLUNakuHy": "Stake Pool",
    "SwaPpA9LAaLfeLi3a68M4DjnLqgtticKg6CnyNwgAC8": "Swap Program",
    "LendZqTs7gn5CTSJU1jWKhKuVpjJGom45nnwPb2AMTi": "Lending Program",
    "cmtDvXumGCrqC1Age74AVPhSRVXJMd8PJS91L8KbNCK": "State Compression",
    "metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s": "Token Metadata",
    "vau1zxA2LbssAUEF7Gpw91zMM1LvXrvpzJtmZ58rPsn": "Token Vault",
    "ProgM6JCCvbYkfKqJYHePx4xxSUSqJp7rh8Lyv7nk7S": "Program Metadata",
    # Well-known tokens
    "So11111111111111111111111111111111111111112": "Wrapped SOL",
    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v": "USDC",
    "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB": "USDT",
    # DEX programs
    "srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX": "OpenBook DEX",
    "BJ3jrUzddfuSrZHXSCxMUUQsjKEyLmuuyZebkcaFp2fg": "Serum DEX v1",
    "EUqojwWA2rd19FZrzeBncJsm38Jm1hEhE3zsmX3bRc2o": "Serum DEX v2",
    "9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin": "Serum DEX v3",
    "22Y43yTVxuUkoRKdm9thyRhQ3SdgQS7c7kB6UNCiaczD": "Serum Swap",
    "WvmTNLpGMVbwJVYztYL4Hnsy82cJhQorxjnnXcRm3b6": "Serum Pool",
    "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4": "Jupiter v6",
    "JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB": "Jupiter v4",
    "Dooar9JkhdZ7J3LHN3A7YCuoGRUggXhQaG4kijfLGU2j": "STEPN DEX",
    # Orca
    "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc": "Orca Whirlpool",
    "9W959DqEETiGZocYWCQPaJ6sBmUzgfxXfqGeTEdp3aQP": "Orca Swap v2",
    "DjVE6JNiYqPL2QXyCUUh8rNjHrbz9hXHNYt99MQ59qw1": "Orca Swap v1",
    "82yxjeMsvaURa4MbZZ7WZZHfobirZYkH1zF8fmeGtyaQ": "Orca Aquafarm",
    # Raydium
    "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8": "Raydium AMM",
    "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK": "Raydium CLMM",
    "RVKd61ztZW9GUwhRbbLoYVRE5Xf1B2tVscKqwZqXgEr": "Raydium LP v1",
    "27haf8L6oxUeXrHrgEgsexjSY5hbVUWEmvv9Nyxg8vQv": "Raydium LP v2",
    "9HzJyW1qZsEiSfMUf6L2jo3CcTKAyBmSyKdwQeYisHrC": "Raydium IDO",
    "EhhTKczWMGQt46ynNeRX1WfeagwwJd7ufHvCDjRxjo5Q": "Raydium Staking",
    # Saber
    "Crt7UoUR6QgrFrN7j8rmSQpUTNWNSitSwWvsWGf1qZ5t": "Saber Router",
    "SSwpkEEcbUqx4vtoEByFjSkhKdCT862DNVb52nZg1UZ": "Saber Stable Swap",
    # Solend
    "So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo": "Solend",
    # Mango
    "JD3bq9hGdy38PuWQ4h2YJpELmHVGPPfFSuFkpzAd9zfu": "Mango v1",
    "5fNfvyp5czQVX77yoACa3JJVEhdRaWjPuazuWgjhTqEH": "Mango v2",
    "mv3ekLzLbnVPNxjSKvqBpU3ZeZXPQdEC3bp5MDEBG68": "Mango v3",
    "GqTPL6qRf5aUuqscLh8Rg2HTxPUXfhhAXDptTLhp1t2J": "Mango Governance",
    "7sPptkymzvayoSbLXzBsXEF8TSf3typNnAWkrKrDizNb": "Mango ICO",
    # Oracles
    "FsJ3A3u2vn5cTVofAjvy6y5kwABJAqYWpe4975bi2epH": "Pyth Oracle",
    "DtmE9D2CSB4L5D6A15mraeEjrGMm6auWVzgaD8hK2tZM": "Switchboard Oracle",
    "cjg3oHmg9uuPsP8D6g29NWvhySJkdYdAo9D25PRbKXJ": "Chainlink Oracle",
    "HEvSKofvBgfaexv23kMabbYqxasxU3mQ4ibBMEmJWHny": "Chainlink Store",
    "Gt9S41PtjR58CbG9JhJ3J6vxesqrNAswbWYbLNTMZA3c": "Chainlink Data Streams Verifier",
    # Wormhole
    "WormT3McKhFJ2RkiGpdw9GKvNCrB2aB54gb2uV9MfQC": "Wormhole",
    "worm2ZoG2kUd4vFXhvjh93UUH596ayRfgQ2MgjNMTth": "Wormhole Core Bridge",
    "wormDTUJ6AWPNvk59vGQbDvGJmqbDTdgWgAqcLBCgUb": "Wormhole Token Bridge",
    "WnFt12ZrnzZrFZkt2xsNsaNWoQribnuQ5B5FrDbwDhD": "Wormhole NFT Bridge",
    # NFT / Metaplex
    "p1exdMJcjVao65QdewkaZRUnU6VPSXhus9n2GzWfh98": "Metaplex",
    "auctxRXPeJoc4817jDhf4HbjnhEcr1cCXenosMhK5R8": "NFT Auction",
    "cndyAnrLdpjq1Ssp1z8xxDsB8dxe7u4HL5Nxi2K5WXZ": "NFT Candy Machine",
    "cndy3Z4yapfJBmL3ShUp5exZKqR3z33thTzeNMm2gRZ": "NFT Candy Machine V2",
    # Staking / Liquid staking
    "MarBmsSgKXdrN1egZf5sqe1TMai9K1rChYNDJgjq7aD": "Marinade Staking",
    "CrX7kMhLC3cSsXJdT7JDgqrRVWGnUpX3gfEfxxU2NVLi": "Lido for Solana",
    # Other DeFi
    "MERLuDFBMmsHnsBPZw2sDQZHvXFMwp8EdjudcU2HKky": "Mercurial Stable Swap",
    "Port7uDYB3wk6GJAw4KT1WpTeMtSu9bTcChBHkX2LfR": "Port Finance",
    "SSwpMgqNDsyV7mAgN9ady4bDVu5ySjmmXejXvy2vLt1": "Step Finance Swap",
    "SWiMDJYFUGj6cPrQ6QYYYWZtvXQdRChSVAygDZDsCHC": "Swim Swap",
    "C64kTdg1Hzv5KoQmZrQRcm2Qz7PkxtFBgw7EpFhvYn8W": "Acumen",
    "DF1ow4tspfHX9JwWJsAb9epbkA8hmpSEAtxXy1V27QBH": "DFlow Aggregator v4",
    "oreV2ZymfyeXgNgBdqMkumTqqAprVqgBWQfoYkrtKWQ": "ORE Program",
    # Quarry
    "QMMD16kjauP5knBwxNUJRZ1Z5o3deBuFrqVjBVmmqto": "Quarry Merge Mine",
    "QMNeHCGYnLVDn1icRAfQZpjPLBNkfGbSKRB83G5d8KB": "Quarry Mine",
    "QMWoBmAyJLAsA1Lh9ugMTw2gciTihncciphzdNzdZYV": "Quarry Mint Wrapper",
    "QRDxhMw1P2NEfiw5mYXG79bwfgHTdasY2xNP76XSea9": "Quarry Redeemer",
    "QREGBnEj9Sa5uR91AV8u3FxThgP5ZCvdZUW2bHAkfNc": "Quarry Registry",
    # Clockwork
    "3XXuUFfweXBwFgFfYaejLvZE4cGZiHgKiGfMtdxNzYmv": "Clockwork v1",
    "CLoCKyJ6DXBJqqu2VWx9RLbgnwwR6BMHHuyasVmfMzBh": "Clockwork v2",
    # NFT marketplaces
    "CJsLwbP1iu5DuUikHEJnLfANgKy6stB2uFgvBBHoyxwz": "Solanart",
    "5ZfZAwP2m93waazg8DkrrVmsupeiPEvaEHowiUP7UAbJ": "Solanart GO",
    # ZK Compression
    "SySTEM1eSU2p4BGQfQpimFEWWSC1XDFeun3Nqzz3rT7": "ZK Light System",
    "cTokenmWW8bLPjZEBAUgYy3zKxQZW6VKi7bqNFEVv3m": "ZK Compressed Token",
    "compr6CUsB5m2jS4Y3831ztGSTnDpnKJTKS95d64XVq": "ZK Account Compression",
    # Other
    "L2TExMFKdjpN9kozasaurPirfHy9P8sbXoAN1qA3S95": "Lighthouse",
    "BrEAK7zGZ6dM71zUDACDqJnekihmwF15noTddWTsknjC": "Break Solana",
    "22zoJMtdu4tQc2PzL74ZUT7FrwgB1Udec8DdW4yw4BdG": "Solana Attestation Service",
    # Specific pool/vault addresses (from Solscan labels via browser lookup)
    "DdZR6zRFiUt4S5mg7AV1uKB2z1f1WzcNYCaTEEWPAuby": "Solend Main Pool Lending Authority",
    "8UviNr47S8eL6J3WfDxMRa3hvLta1VDJwNWqsDgtN3Cv": "Solend Main Pool (SOL) Vault",
    "8SheGtsopRUDzdiD6v6BR9a6bqZ9QwywYQY99Fp5meNf": "Solend Main Pool (USDC) Vault",
    "UtRy8gcEu9fCkDuUrU8EmC7Uc6FZy5NCwttzG7i6nkw": "Solend Main Pool (cUSDC) Supply",
    "DQyrAcCrDXQ7NeoqGgDCZwBvWDcYmFCjSb9JtteuvPpz": "Raydium WSOL-USDC Pool",
    "HLmqeL62xR1QoZ1HKKbXRrdN1p3phKpxRMb2VVopvBBz": "Raydium WSOL-USDC Pool",
    "FaUYbopmMVdNRe3rLnqGPBA2KB96nLHudKaEgAUcvHXn": "Raydium SNY-USDC Pool",
    "9YiW8N9QdEsAdTQN8asjebwwEmDXAHRnb1E3nvz64vjg": "Raydium SNY-USDC Pool",
    "91fMidHL8Yr8KRcu4Zu2RPRRg1FbXxZ7DV43rAyKRLjn": "Raydium MNGO-USDC Pool",
    "93oFfbcayY2WkcR6d9AyqPcRC121dXmWarFJkwPErRRE": "Raydium MNGO-USDC Pool",
    "FdmKUE4UMiJYFK5ogCngHzShuVKrFXBamPWcewDr31th": "Raydium RAY-USDC Pool",
    "Eqrhxd7bDUCH3MepKmdVkgwazXRzY6iHhEoBpY7yAohk": "Raydium RAY-USDC Pool",
    "mrksLcZ6rMs9xkmJgw6oKiR3GECw44Gb5NeDqu64kiw": "Save Reward",
    "FC81tbGt6JWRXidaWYFXxGnTk4VgobhJHATvTRVMqgWj": "FranciumDeFi Lending",
    # Francium yield farming PDAs (from Solscan via browser lookup)
    "9Jbh6bcHxgxb7S1FfNjFTUQABmd2cwzHWY3gxG4A2CjD": "Francium Yield Farming",
    "9dhoieCDdX3qawKK283vtcLmKHnvnMm3VQTKybyBcSdd": "Francium Yield Farming",
    # Jito tip accounts (from Solscan labels via browser lookup)
    "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49": "Jitotip 4",
    # Orca pool accounts (from Solscan via browser lookup)
    "FNi3sgLGkZk5QKTQgeCs5JbW5sG9wtZSDTvCb1TCoD72": "Orca Whirlpool",
    "64KYDNevDqphJHSHb3snrBU3PBJzka7gsJwzYm85LuoB": "Orca LIQ-USDC Pool",
    # Raydium pool/staking accounts (from Solscan via browser lookup)
    "CcRZ2sBjxFtPM2GFJ4HeRu4eeBTsx9Ng5Mug6uxUjZxo": "Raydium LARIX-RAY Pool",
    "5KQFnDd33J5NaMC9hQ64P5XzaaSz8Pt7NBCkZFYn1po": "Raydium RAY-USDC Pool",
    "DgbCWnbXg43nmeiAveMCkUUPEpAr3rZo3iop3TyP6S63": "Raydium Staking",
    "DdFXxCbn5vpxPRaGmurmefCTTSUa5XZ9Kh6Noc4bvrU9": "Raydium RAY Pool",
    "9VbmvaaPeNAke2MAL3h2Fw82VubH1tBCzwBzaWybGKiG": "Raydium RAY-WSOL Pool",
    # Synthetify (from Solscan via browser lookup)
    "HNc8UfDDMkrpLcNapbp7yqKYG2UjJufTvZiYsCTvFPV5": "Synthetify",
    # Orca protocol PDA (from Solscan via browser lookup)
    "MDcWkwPqr5HrA91g4GGax7bVP1NDDetnR12nGhoAdYj": "Orca WSOL-USDC Staking",
    # Saber pool authority (from Solscan labels via browser lookup)
    "9osV5a7FXEjuMujxZJGBRXVAyQ5fJfBFNkyAf6fSz9kw": "Saber UST-USDC Pool Authority",
    # Orca Aquafarm reward vault paying LARIX incentives (verified via Solscan — calls Orca Aquafarm harvest, not Larix)
    "HF19JZBTQafLS8JaV842wMQAkTucTZrgGz1NAo3v6jcz": "Orca Aquafarm LARIX Reward Vault",
    # Raydium Stake V5 farm reward vault paying LARIX incentives (verified via Solscan — Raydium Stake V5 program, not Larix)
    "sCDx3LzV8jPFX1VuRQDAGNKVfiCvhvrv3tJijaXzhXw": "Raydium Farm LARIX Reward Vault",
    # Orca farming PDAs (from Solscan via browser lookup)
    "9czgZkSxLFtxmvWSb1PEHmUyBuNpAUxj9XAcHKikYnzt": "Orca Double-Dip Farm",
    "CtXKDXJ4wzgto48QQFANestEgtov5dJRrs9qpRw7BV1h": "Orca Double-Dip Farm",
    # Openbook pool authority (from Solscan labels via browser lookup)
    "EUMY3VKFzAeNANgPW11vUQvSUxCzQp6a1yMqZjLvMWXY": "Openbook PYTH-USDC Pool Authority",
    # Meteora DLMM pool (from Solscan via browser lookup)
    "HyhMt7jPKJ1LLXQTm5wjf5f4kWqAeTeKQZvMq8TtZnPV": "Meteora DLMM JUP-USDC Pool",
    # Mango Markets (from Solscan via browser lookup)
    "EiRhqxC8qCX5UYW6B7iBQ7QrkPcK2QxwKvyvV85YqXRD": "Mango Markets",
    # Lifinity (from Solscan labels via browser lookup)
    "AvtfUvU3byPXgp6Dpw3mgKB2BbVwQvGyry9KeMzD9BLc": "Lifinity Locker",
}

# RPC batch size (Solana getMultipleAccounts max is 100)
RPC_BATCH_SIZE = 100

# Curated address labels shipped with the plugin. Generated from on-chain
# metadata; regenerated via update_labels_from_csv.py.
_KNOWN_ADDRESSES_PATH = os.path.join(os.path.dirname(__file__), "known_addresses.json")


def _load_known_addresses():
    try:
        with open(_KNOWN_ADDRESSES_PATH) as f:
            return json.load(f)
    except (OSError, json.JSONDecodeError):
        return {}


KNOWN_ADDRESSES = _load_known_addresses()


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
      - Literal paths: "richard/crypto/wallet/solana" (used as-is if it exists)
      - Account-style paths: "assets:solana" -> searches for {owner}/solana
        or {owner}-solana under sources/
    """
    if os.path.isdir(os.path.join(sources_dir, scan_path)):
        return scan_path

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
        if os.path.isdir(os.path.join(full, suffix)):
            return os.path.join(entry, suffix)
        if "/" not in suffix and entry.endswith(f"-{suffix}"):
            return entry

    return None


def scan_csvs_for_addresses(sources_dir, scan_path, columns):
    """Walk CSVs under scan_path and extract unique Solana addresses from given columns."""
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
                            addr = row.get(col, "").strip()
                            if addr and SOL_ADDRESS_RE.match(addr):
                                addresses.add(addr)
            except (csv.Error, UnicodeDecodeError, KeyError):
                continue

    return addresses


def extract_covered_addresses(rules, prefix="auto-sol-"):
    """Extract addresses already covered by existing rules, keyed by rule type.

    Commodity rules with generic labels (e.g. 'Token_Program_account') are
    NOT counted as covered so they get re-looked-up on the next run.
    """
    payee_covered = set()
    commodity_covered = set()
    for rule in rules:
        rule_id = rule.get("id", "")
        if not rule_id.startswith(prefix) and not rule_id.startswith("auto-sol-token-"):
            continue
        pattern = rule.get("pattern", "")
        # Extract the address from *address* pattern
        addr = pattern.strip("*").strip()
        if addr and SOL_ADDRESS_RE.match(addr):
            if rule.get("match_field") == "commodity":
                commodity = rule.get("commodity", "")
                # Skip generic fallback labels so they get re-resolved
                if commodity.endswith("_account") or commodity.endswith("_mint"):
                    continue
                commodity_covered.add(addr)
            else:
                payee_covered.add(addr)
    return payee_covered, commodity_covered


def fetch_token_list_solana_labs(data_dir):
    """Fetch or load cached solana-labs token list. Returns {mint_address: symbol}."""
    cache_path = os.path.join(data_dir, "tokenlist_solana_labs.json")

    if os.path.exists(cache_path):
        age = time.time() - os.path.getmtime(cache_path)
        if age < GITHUB_CACHE_MAX_AGE:
            cached = load_json_file(cache_path)
            if cached:
                return cached

    url = TOKEN_LIST_URLS[0][1]
    try:
        req = Request(url, headers={"Accept": "application/json"})
        with urlopen(req, timeout=30) as resp:
            data = json.loads(resp.read())
    except (HTTPError, URLError, json.JSONDecodeError):
        cached = load_json_file(cache_path)
        return cached if cached else {}

    tokens = {}
    for token in data.get("tokens", []):
        addr = token.get("address", "").strip()
        symbol = token.get("symbol", "").strip()
        if addr and symbol:
            tokens[addr] = symbol

    save_json_file(cache_path, tokens)
    return tokens


def fetch_token_list_jup(data_dir):
    """Fetch or load cached Jupiter validated token list. Returns {mint_address: symbol}."""
    cache_path = os.path.join(data_dir, "tokenlist_jup.json")

    if os.path.exists(cache_path):
        age = time.time() - os.path.getmtime(cache_path)
        if age < GITHUB_CACHE_MAX_AGE:
            cached = load_json_file(cache_path)
            if cached:
                return cached

    url = TOKEN_LIST_URLS[1][1]
    try:
        req = Request(url, headers={"Accept": "text/csv"})
        with urlopen(req, timeout=30) as resp:
            text = resp.read().decode("utf-8")
    except (HTTPError, URLError):
        cached = load_json_file(cache_path)
        return cached if cached else {}

    tokens = {}
    reader = csv.DictReader(text.splitlines())
    for row in reader:
        addr = row.get("Mint", row.get("mint", "")).strip()
        symbol = row.get("Symbol", row.get("symbol", "")).strip()
        if addr and symbol:
            tokens[addr] = symbol

    save_json_file(cache_path, tokens)
    return tokens


def fetch_all_token_lists(data_dir):
    """Fetch all GitHub token lists and merge. Returns {mint_address: symbol}."""
    tokens = {}
    # solana-labs first, then jup overrides (more actively maintained)
    tokens.update(fetch_token_list_solana_labs(data_dir))
    tokens.update(fetch_token_list_jup(data_dir))
    return tokens


def fetch_solscan_labels(data_dir):
    """Fetch or load cached Solscan program/address labels. Returns {address: name}."""
    cache_path = os.path.join(data_dir, "solscan_labels.json")

    if os.path.exists(cache_path):
        age = time.time() - os.path.getmtime(cache_path)
        if age < GITHUB_CACHE_MAX_AGE:
            cached = load_json_file(cache_path)
            if cached:
                return cached

    try:
        req = Request(SOLSCAN_LABELS_URL, headers={"Accept": "application/json"})
        with urlopen(req, timeout=30) as resp:
            data = json.loads(resp.read())
    except (HTTPError, URLError, json.JSONDecodeError):
        cached = load_json_file(cache_path)
        return cached if cached else {}

    # Merge "program" and "address" sections into a flat {address: name} dict
    labels = {}
    for section in ("program", "address"):
        section_data = data.get(section, {})
        if isinstance(section_data, dict):
            for addr, name in section_data.items():
                if addr and name:
                    labels[addr] = name

    save_json_file(cache_path, labels)
    return labels


def rpc_get_multiple_accounts(addresses, rpc_url):
    """Call Solana getMultipleAccounts RPC. Returns list of account info dicts (or None)."""
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getMultipleAccounts",
        "params": [
            list(addresses),
            {"encoding": "jsonParsed"},
        ],
    }
    body = json.dumps(payload).encode()
    req = Request(rpc_url, data=body, headers={
        "Content-Type": "application/json",
        "Accept": "application/json",
    })
    try:
        with urlopen(req, timeout=30) as resp:
            data = json.loads(resp.read())
    except (HTTPError, URLError, json.JSONDecodeError):
        return [None] * len(addresses)

    result = data.get("result", {})
    return result.get("value", [None] * len(addresses))


def lookup_addresses(addresses, data_dir, rpc_url, token_lists, solscan_labels=None):
    """Look up Solana addresses using token lists, Solscan labels, known programs, and RPC.

    Returns {addr: name} and list of warnings.
    """
    if solscan_labels is None:
        solscan_labels = {}

    labels = {}
    warnings = []

    cache_path = os.path.join(data_dir, "label_cache.json")
    cache = load_json_file(cache_path) or {}

    # Re-check previously empty cache entries against curated known addresses
    # and Solscan labels (new sources may resolve addresses unknown before)
    for addr in list(cache):
        if cache[addr] == "":
            if addr in KNOWN_ADDRESSES:
                cache[addr] = KNOWN_ADDRESSES[addr]
            elif addr in solscan_labels:
                cache[addr] = solscan_labels[addr]

    uncached = set()
    for addr in addresses:
        if addr in cache:
            name = cache[addr]
            if name:
                labels[addr] = name
        else:
            uncached.add(addr)

    if not uncached:
        save_json_file(cache_path, cache)
        return labels, warnings

    # 1. Known programs + exchanges (hardcoded), curated known addresses,
    #    token lists, Solscan labels
    still_unknown = set()
    for addr in uncached:
        name = (KNOWN_EXCHANGES.get(addr)
                or KNOWN_PROGRAMS.get(addr)
                or KNOWN_ADDRESSES.get(addr)
                or token_lists.get(addr)
                or solscan_labels.get(addr))
        if name:
            labels[addr] = name
            cache[addr] = name
        else:
            still_unknown.add(addr)

    # 2. Solana RPC — identify SPL token accounts and resolve mints
    if still_unknown:
        addr_list = sorted(still_unknown)
        rpc_count = 0
        for i in range(0, len(addr_list), RPC_BATCH_SIZE):
            batch = addr_list[i : i + RPC_BATCH_SIZE]
            accounts = rpc_get_multiple_accounts(batch, rpc_url)

            for addr, acct in zip(batch, accounts):
                if acct is None:
                    cache[addr] = ""
                    continue

                acct_data = acct.get("data", {})

                if isinstance(acct_data, dict):
                    parsed = acct_data.get("parsed", {})
                    program = acct_data.get("program", "")

                    # SPL token mint — resolve to token symbol.
                    # SPL token accounts (ATAs) and program-owned accounts are
                    # intentionally skipped: "USDC token account" / "Token Program
                    # account" labels are not informative enough to be useful.
                    if program == "spl-token" and isinstance(parsed, dict):
                        ptype = parsed.get("type", "")
                        if ptype == "mint":
                            sym = token_lists.get(addr)
                            name = f"{sym} token mint" if sym else addr
                            labels[addr] = name
                            cache[addr] = name
                            continue

                cache[addr] = ""

            rpc_count += len(batch)
            # Rate limit: ~2 req/sec for public RPC
            time.sleep(0.5)

            if rpc_count >= 1000:
                remaining = len(addr_list) - rpc_count
                if remaining > 0:
                    warnings.append(f"RPC lookup stopped after {rpc_count} addresses, {remaining} remaining")
                break

    save_json_file(cache_path, cache)
    return labels, warnings


def make_rule(address, name):
    """Create a payee rule for a labeled address."""
    short = address[:10]
    return {
        "id": f"auto-sol-{short}",
        "pattern": f"*{address}*",
        "payee": name,
        "comment": "auto:sol-labels",
    }


def sanitize_commodity(symbol):
    """Sanitize a token symbol into a valid ledger commodity.

    Trading pairs like 'PORT/USDC' or 'RAY-SOL' become 'PORT_USDC_LP'.
    Multi-word names like 'Token Program account' become 'Token_Program_account'.
    The commodity regex allows: ^[A-Za-z0-9_][A-Za-z0-9_.\\-]*$
    """
    # Detect LP/pair tokens: split on / or -
    if "/" in symbol or "-" in symbol:
        parts = re.split(r"[/\-]", symbol)
        if len(parts) == 2 and all(p.strip() for p in parts):
            return f"{parts[0]}_{parts[1]}_LP"
    # Replace spaces with underscores
    sanitized = symbol.replace(" ", "_")
    # Strip any remaining invalid characters (keep A-Za-z0-9_.- )
    sanitized = re.sub(r"[^A-Za-z0-9_.\-]", "", sanitized)
    # Must start with [A-Za-z0-9_]
    sanitized = re.sub(r"^[^A-Za-z0-9_]+", "", sanitized)
    return sanitized if sanitized else "UNKNOWN"


def make_commodity_rule(address, token_symbol):
    """Create a commodity-rename rule for a token mint address."""
    short = address[:10]
    return {
        "id": f"auto-sol-token-{short}",
        "pattern": f"*{address}*",
        "match_field": "commodity",
        "commodity": sanitize_commodity(token_symbol),
        "comment": "auto:sol-labels",
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

    # Build index of existing auto-generated rules by id
    existing_idx = {}
    for i, r in enumerate(existing):
        rid = r.get("id", "")
        if rid:
            existing_idx[rid] = i

    # Separate into updates (existing auto rules) vs truly new
    truly_new = []
    for rule in new_rules:
        rid = rule.get("id")
        if rid in existing_idx:
            # Update auto-generated rules in place
            idx = existing_idx[rid]
            if existing[idx].get("comment") == "auto:sol-labels":
                existing[idx] = rule
        else:
            truly_new.append(rule)

    catchall_idx = len(existing)
    for i, rule in enumerate(existing):
        if rule.get("pattern") == "*":
            catchall_idx = i
            break

    for rule in truly_new:
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

    raw_scan_path = config.get("scan_path", "assets:crypto:wallet:solana")
    from_col = config.get("from_column", "from_address")
    to_col = config.get("to_column", "to_address")
    commodity_col = config.get("commodity_column", "")
    rpc_url = secrets.get("solana_rpc_url") or DEFAULT_RPC_URL

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

    # 1b. Scan CSVs for token mint addresses (commodity column)
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

    # 3. Fetch token lists, then look up all new addresses
    token_lists = fetch_all_token_lists(data_dir)
    solscan_labels = fetch_solscan_labels(data_dir)
    labels, lookup_warnings = lookup_addresses(new_for_lookup, data_dir, rpc_url, token_lists, solscan_labels)
    warnings.extend(lookup_warnings)

    # 4. Generate rules
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
