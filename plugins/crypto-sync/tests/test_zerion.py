"""Tests for sync_crypto.zerion."""
import json
from pathlib import Path
import pytest
import requests
from unittest.mock import MagicMock, patch

from sync_crypto.models import TransactionRecord, WalletConfig

FIXTURES = Path(__file__).parent / "fixtures"


def load_fixture(name: str) -> dict:
    return json.loads((FIXTURES / name).read_text())


def _tx_payload(
    *,
    tx_id: str,
    tx_hash: str,
    mined_at: str,
    chain_id: str = "ethereum",
    next_url: str | None = None,
    token_symbol: str = "ETH",
    token_name: str = "Ether",
    token_contract: str | None = None,
    decimals: int = 18,
    numeric_value: str = "1.5",
) -> dict:
    implementations = []
    if token_contract:
        implementations.append(
            {
                "address": token_contract,
                "chain_id": chain_id,
                "decimals": decimals,
            }
        )
    return {
        "data": [
            {
                "id": tx_id,
                "type": "transactions",
                "attributes": {
                    "hash": tx_hash,
                    "status": "confirmed",
                    "mined_at": mined_at,
                    "mined_at_block": 18500000,
                    "operation_type": "transfer",
                    "fee": {
                        "value": "21000000000000",
                    },
                    "transfers": [
                        {
                            "from": "0xfrom",
                            "to": "0xto",
                            "quantity": {
                                "numeric": numeric_value,
                                "decimals": decimals,
                            },
                            "fungible_info": {
                                "symbol": token_symbol,
                                "name": token_name,
                                "implementations": implementations,
                            },
                        }
                    ],
                },
                "relationships": {
                    "chain": {"data": {"id": chain_id}},
                },
            }
        ],
        "links": {
            "next": next_url,
        },
    }


def _sparse_solana_trade_payload(wallet_address: str) -> dict:
    return {
        "data": [
            {
                "id": "serum-trade",
                "type": "transactions",
                "attributes": {
                    "hash": "2wPLRKmUoy3PjX61HzZGkMNokoWooWcnjxEw5dq632QKHe7EnYTFmgo2RzgWDPyhWFcEjBKczh7MPiS5zQ5bUrzY",
                    "status": "confirmed",
                    "mined_at": "2021-05-09T06:13:56Z",
                    "mined_at_block": 77625837,
                    "operation_type": "send",
                    "fee": {"value": "10000"},
                    "transfers": [
                        {
                            "from": wallet_address,
                            "to": "8Emz4L4mjEoKPFbg2VN3xYD6Bhh8CmUYHA4L39pbfeiX",
                            "quantity": {
                                "value": "23357760",
                                "decimals": 9,
                            },
                            "fungible_info": {
                                "symbol": "SOL",
                                "name": "Solana",
                                "implementations": [
                                    {"chain_id": "solana", "decimals": 9},
                                ],
                            },
                        }
                    ],
                },
                "relationships": {
                    "chain": {"data": {"id": "solana"}},
                },
            }
        ],
        "links": {},
    }


def _parsed_solana_trade_tx(wallet_address: str) -> dict:
    return {
        "slot": 77625837,
        "blockTime": 1620596836,
        "meta": {
            "err": None,
            "fee": 10000,
            "preTokenBalances": [
                {
                    "accountIndex": 2,
                    "mint": "RAYMINT",
                    "uiTokenAmount": {"amount": "59566004", "decimals": 6},
                },
                {
                    "accountIndex": 3,
                    "mint": "RAYMINT",
                    "uiTokenAmount": {"amount": "125969800000", "decimals": 6},
                },
                {
                    "accountIndex": 4,
                    "mint": "USDCMINT",
                    "uiTokenAmount": {"amount": "2360360588168", "decimals": 6},
                },
                {
                    "accountIndex": 5,
                    "mint": "USDCMINT",
                    "owner": wallet_address,
                    "uiTokenAmount": {"amount": "99111870", "decimals": 6},
                },
            ],
            "postTokenBalances": [
                {
                    "accountIndex": 2,
                    "mint": "RAYMINT",
                    "uiTokenAmount": {"amount": "32566004", "decimals": 6},
                },
                {
                    "accountIndex": 3,
                    "mint": "RAYMINT",
                    "uiTokenAmount": {"amount": "125996800000", "decimals": 6},
                },
                {
                    "accountIndex": 4,
                    "mint": "USDCMINT",
                    "uiTokenAmount": {"amount": "2359980267718", "decimals": 6},
                },
                {
                    "accountIndex": 5,
                    "mint": "USDCMINT",
                    "owner": wallet_address,
                    "uiTokenAmount": {"amount": "479432320", "decimals": 6},
                },
            ],
            "innerInstructions": [
                {
                    "index": 1,
                    "instructions": [
                        {
                            "program": "spl-token",
                            "parsed": {
                                "type": "transfer",
                                "info": {
                                    "amount": "27000000",
                                    "authority": wallet_address,
                                    "destination": "MarketRAY",
                                    "source": "WalletRAY",
                                },
                            },
                        }
                    ],
                },
                {
                    "index": 2,
                    "instructions": [
                        {
                            "program": "spl-token",
                            "parsed": {
                                "type": "transfer",
                                "info": {
                                    "amount": "380320450",
                                    "authority": "MarketAuth",
                                    "destination": "WalletUSDC",
                                    "source": "MarketUSDC",
                                },
                            },
                        }
                    ],
                },
            ],
        },
        "transaction": {
            "message": {
                "accountKeys": [
                    {"pubkey": wallet_address, "signer": True},
                    {"pubkey": "OpenOrders", "signer": False},
                    {"pubkey": "WalletRAY", "signer": False},
                    {"pubkey": "MarketRAY", "signer": False},
                    {"pubkey": "MarketUSDC", "signer": False},
                    {"pubkey": "WalletUSDC", "signer": False},
                ],
                "instructions": [
                    {
                        "program": "system",
                        "programId": "11111111111111111111111111111111",
                        "accounts": [wallet_address, "8Emz4L4mjEoKPFbg2VN3xYD6Bhh8CmUYHA4L39pbfeiX"],
                    },
                    {
                        "programId": "9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin",
                        "accounts": ["OpenOrders", "WalletRAY", "MarketRAY", wallet_address],
                    },
                    {
                        "programId": "9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin",
                        "accounts": ["OpenOrders", wallet_address, "MarketRAY", "MarketUSDC", "WalletRAY", "WalletUSDC"],
                    },
                ],
            },
        },
    }


def _bare_execute_solana_payload(wallet_address: str, tx_hash: str) -> dict:
    """Zerion API placeholder for a transaction whose `transfers` array is
    empty — the CSV row ends up as `tx_type=execute, value=0` with no
    token info. Real example: 2021-08-09 ATA-to-ATA USDC transfers signed
    by EEbn9C → 2baaTDz which Zerion classified as bare `execute` ops.
    """
    return {
        "data": [
            {
                "id": "bare-execute",
                "type": "transactions",
                "attributes": {
                    "hash": tx_hash,
                    "status": "confirmed",
                    "mined_at": "2021-08-09T21:14:19Z",
                    "mined_at_block": 90955098,
                    "operation_type": "execute",
                    "fee": {"value": "5000"},
                    "transfers": [],
                },
                "relationships": {
                    "chain": {"data": {"id": "solana"}},
                },
            }
        ],
        "links": {},
    }


def _parsed_solana_direct_transfer_tx(wallet_address: str, dest_ata: str, amount: str = "20265700000") -> dict:
    """Solana RPC `getTransaction` result for a bare ATA-to-ATA transfer:
    one OUTER `spl-token transferChecked` instruction, no inner
    instructions, signed by `wallet_address`. Regression for the bug
    where the enrichment loop only walked `meta.innerInstructions` and
    missed direct outer-level spl-token operations.
    """
    source_ata = "Gr7w5JXqEpgcjpWwqRNntcLFrcmHGD5ssoWbCBtfbfz9"
    return {
        "slot": 90955098,
        "blockTime": 1628553259,
        "meta": {
            "err": None,
            "fee": 5000,
            "preTokenBalances": [
                {
                    "accountIndex": 1,
                    "mint": "USDCMINT",
                    "owner": wallet_address,
                    "uiTokenAmount": {"amount": "30000000000", "decimals": 6},
                },
                {
                    "accountIndex": 2,
                    "mint": "USDCMINT",
                    "owner": "DESTOWNER",
                    "uiTokenAmount": {"amount": "0", "decimals": 6},
                },
            ],
            "postTokenBalances": [
                {
                    "accountIndex": 1,
                    "mint": "USDCMINT",
                    "owner": wallet_address,
                    "uiTokenAmount": {"amount": "9734300000", "decimals": 6},
                },
                {
                    "accountIndex": 2,
                    "mint": "USDCMINT",
                    "owner": "DESTOWNER",
                    "uiTokenAmount": {"amount": amount, "decimals": 6},
                },
            ],
            "innerInstructions": [],
        },
        "transaction": {
            "message": {
                "accountKeys": [
                    {"pubkey": wallet_address, "signer": True, "writable": True},
                    {"pubkey": source_ata, "signer": False, "writable": True},
                    {"pubkey": dest_ata, "signer": False, "writable": True},
                    {"pubkey": "USDCMINT", "signer": False, "writable": False},
                    {"pubkey": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", "signer": False, "writable": False},
                ],
                "instructions": [
                    {
                        "program": "spl-token",
                        "programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
                        "accounts": [source_ata, "USDCMINT", dest_ata, wallet_address],
                        "parsed": {
                            "type": "transferChecked",
                            "info": {
                                "authority": wallet_address,
                                "source": source_ata,
                                "destination": dest_ata,
                                "mint": "USDCMINT",
                                "tokenAmount": {
                                    "amount": amount,
                                    "decimals": 6,
                                    "uiAmount": int(amount) / 1_000_000,
                                    "uiAmountString": str(int(amount) / 1_000_000),
                                },
                            },
                        },
                    },
                ],
            },
        },
    }


def _partial_solana_liquidity_payload(wallet_address: str) -> dict:
    return {
        "data": [
            {
                "id": "raydium-deposit",
                "type": "transactions",
                "attributes": {
                    "hash": "44TEvsJehq8XCjovD6BnQySfqYLZq81eUmyW4RDbr3TCNerjK6iBEBK2EVqmfQUNTTkxfhjSQUK5YXyZxxmQEyXY",
                    "status": "confirmed",
                    "mined_at": "2021-04-26T08:29:52Z",
                    "mined_at_block": 75454061,
                    "operation_type": "deposit",
                    "sent_from": wallet_address,
                    "fee": {"value": "15000"},
                    "transfers": [
                        {
                            "sender": "",
                            "recipient": wallet_address,
                            "quantity": {
                                "int": "13995716",
                                "decimals": 6,
                                "numeric": "13.995716",
                            },
                            "fungible_info": {
                                "symbol": "RAY-SOL",
                                "name": "Raydium Legacy LP Token V3 (RAY-SOL)",
                                "implementations": [
                                    {
                                        "chain_id": "solana",
                                        "address": "LPMINT",
                                        "decimals": 6,
                                    }
                                ],
                            },
                        },
                        {
                            "sender": wallet_address,
                            "recipient": "PoolWSOL",
                            "quantity": {
                                "int": "39690822531",
                                "decimals": 9,
                                "numeric": "39.690822531",
                            },
                            "fungible_info": {
                                "symbol": "SOL",
                                "name": "Solana",
                                "implementations": [
                                    {
                                        "chain_id": "solana",
                                        "address": "",
                                        "decimals": 9,
                                    }
                                ],
                            },
                        },
                    ],
                },
                "relationships": {
                    "chain": {"data": {"id": "solana"}},
                },
            }
        ],
        "links": {},
    }


def _parsed_solana_liquidity_tx(wallet_address: str) -> dict:
    return {
        "slot": 75454061,
        "blockTime": 1619425792,
        "transaction": {
            "message": {
                "accountKeys": [
                    {"pubkey": wallet_address},
                    {"pubkey": "WalletWSOL"},
                    {"pubkey": "WalletLP"},
                    {"pubkey": "WalletRAY"},
                    {"pubkey": "PoolRAY"},
                    {"pubkey": "PoolWSOL"},
                    {"pubkey": "LPMINT"},
                ],
                "instructions": [
                    {},
                    {
                        "programId": "RaydiumPoolProgram111111111111111111111111111",
                        "accounts": [wallet_address, "PoolRAY", "PoolWSOL", "WalletLP"],
                    },
                ],
            },
        },
        "meta": {
            "err": None,
            "fee": 15000,
            "preTokenBalances": [
                {
                    "accountIndex": 1,
                    "mint": "So11111111111111111111111111111111111111112",
                    "owner": wallet_address,
                    "uiTokenAmount": {"amount": "39690822531", "decimals": 9},
                },
                {
                    "accountIndex": 2,
                    "mint": "LPMINT",
                    "owner": wallet_address,
                    "uiTokenAmount": {"amount": "0", "decimals": 6},
                },
                {
                    "accountIndex": 3,
                    "mint": "RAYMINT",
                    "owner": wallet_address,
                    "uiTokenAmount": {"amount": "165600000", "decimals": 6},
                },
                {
                    "accountIndex": 4,
                    "mint": "RAYMINT",
                    "owner": "PoolAuthority",
                    "uiTokenAmount": {"amount": "1000", "decimals": 6},
                },
                {
                    "accountIndex": 5,
                    "mint": "So11111111111111111111111111111111111111112",
                    "owner": "PoolAuthority",
                    "uiTokenAmount": {"amount": "1000", "decimals": 9},
                },
            ],
            "postTokenBalances": [
                {
                    "accountIndex": 1,
                    "mint": "So11111111111111111111111111111111111111112",
                    "owner": wallet_address,
                    "uiTokenAmount": {"amount": "0", "decimals": 9},
                },
                {
                    "accountIndex": 2,
                    "mint": "LPMINT",
                    "owner": wallet_address,
                    "uiTokenAmount": {"amount": "13995716", "decimals": 6},
                },
                {
                    "accountIndex": 3,
                    "mint": "RAYMINT",
                    "owner": wallet_address,
                    "uiTokenAmount": {"amount": "0", "decimals": 6},
                },
                {
                    "accountIndex": 4,
                    "mint": "RAYMINT",
                    "owner": "PoolAuthority",
                    "uiTokenAmount": {"amount": "166601000", "decimals": 6},
                },
                {
                    "accountIndex": 5,
                    "mint": "So11111111111111111111111111111111111111112",
                    "owner": "PoolAuthority",
                    "uiTokenAmount": {"amount": "39690823531", "decimals": 9},
                },
            ],
            "innerInstructions": [
                {
                    "index": 1,
                    "instructions": [
                        {
                            "program": "spl-token",
                            "parsed": {
                                "type": "transfer",
                                "info": {
                                    "amount": "165600000",
                                    "authority": wallet_address,
                                    "destination": "PoolRAY",
                                    "source": "WalletRAY",
                                },
                            },
                        },
                        {
                            "program": "spl-token",
                            "parsed": {
                                "type": "transfer",
                                "info": {
                                    "amount": "39690822531",
                                    "authority": wallet_address,
                                    "destination": "PoolWSOL",
                                    "source": "WalletWSOL",
                                },
                            },
                        },
                        {
                            "program": "spl-token",
                            "parsed": {
                                "type": "mintTo",
                                "info": {
                                    "account": "WalletLP",
                                    "amount": "13995716",
                                    "mint": "LPMINT",
                                    "mintAuthority": "PoolAuthority",
                                },
                            },
                        },
                    ],
                }
            ],
        },
    }


def _solend_redeem_payload() -> dict:
    return {
        "data": [
            {
                "id": "solend-redeem-tx",
                "type": "transactions",
                "attributes": {
                    "hash": "4RSXp2zpuF5r97hG2rhEKn2UWv3pyAa5Kb39TYUancBqHStJtaaAHJ32tEnjMTPLsBVFJk7muzDRAagrbacXmwaM",
                    "status": "confirmed",
                    "mined_at": "2022-06-01T00:00:00Z",
                    "mined_at_block": 134900000,
                    "operation_type": "execute",
                    "fee": {"value": "5000"},
                    "transfers": [],
                },
                "relationships": {
                    "chain": {"data": {"id": "solana"}},
                },
            }
        ],
        "links": {},
    }


def _parsed_solend_redeem_tx(wallet_address: str) -> dict:
    return {
        "slot": 134900000,
        "blockTime": 1654041600,
        "transaction": {
            "message": {
                "accountKeys": [
                    {"pubkey": wallet_address},
                    {"pubkey": "ReserveCollateralSupplyATA"},
                    {"pubkey": "BurnedCollateralATA"},
                    {"pubkey": "ReserveState"},
                    {"pubkey": "ReserveCollateralMint"},
                    {"pubkey": "ReserveLiquiditySupplyATA"},
                    {"pubkey": "WalletUsdcATA"},
                ],
                "instructions": [
                    {},
                    {
                        "programId": "So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo",
                        "accounts": [
                            "ReserveCollateralSupplyATA",
                            "BurnedCollateralATA",
                            "ReserveState",
                            "ReserveState",
                            "ReserveCollateralMint",
                            "ReserveState",
                            "WalletUsdcATA",
                            "ReserveCollateralMint",
                            "ReserveLiquiditySupplyATA",
                            wallet_address,
                            wallet_address,
                            "SysvarC1ock11111111111111111111111111111111",
                            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
                        ],
                    },
                ],
            },
        },
        "meta": {
            "err": None,
            "fee": 5000,
            "preTokenBalances": [
                {
                    "accountIndex": 1,
                    "mint": "993dVFL2uXWYeoXuEBFXR4BijeXdTv4s6BzsCjJZuwqk",
                    "owner": "DdZR6zRFiUt4S5mg7AV1uKB2z1f1WzcNYCaTEEWPAuby",
                    "uiTokenAmount": {"amount": "195050477975245", "decimals": 9},
                },
                {
                    "accountIndex": 5,
                    "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                    "owner": "DdZR6zRFiUt4S5mg7AV1uKB2z1f1WzcNYCaTEEWPAuby",
                    "uiTokenAmount": {"amount": "38921644626770", "decimals": 6},
                },
                {
                    "accountIndex": 6,
                    "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                    "uiTokenAmount": {"amount": "492967995", "decimals": 6},
                },
            ],
            "postTokenBalances": [
                {
                    "accountIndex": 1,
                    "mint": "993dVFL2uXWYeoXuEBFXR4BijeXdTv4s6BzsCjJZuwqk",
                    "owner": "DdZR6zRFiUt4S5mg7AV1uKB2z1f1WzcNYCaTEEWPAuby",
                    "uiTokenAmount": {"amount": "195030896790340", "decimals": 9},
                },
                {
                    "accountIndex": 5,
                    "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                    "owner": "DdZR6zRFiUt4S5mg7AV1uKB2z1f1WzcNYCaTEEWPAuby",
                    "uiTokenAmount": {"amount": "38901644624115", "decimals": 6},
                },
                {
                    "accountIndex": 6,
                    "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                    "uiTokenAmount": {"amount": "20492970650", "decimals": 6},
                },
            ],
            "innerInstructions": [
                {
                    "index": 1,
                    "instructions": [
                        {
                            "program": "spl-token",
                            "parsed": {
                                "type": "transfer",
                                "info": {
                                    "amount": "19581184905",
                                    "authority": "DdZR6zRFiUt4S5mg7AV1uKB2z1f1WzcNYCaTEEWPAuby",
                                    "destination": "BurnedCollateralATA",
                                    "source": "ReserveCollateralSupplyATA",
                                },
                            },
                        },
                        {
                            "program": "spl-token",
                            "parsed": {
                                "type": "burn",
                                "info": {
                                    "account": "BurnedCollateralATA",
                                    "amount": "19581184905",
                                    "authority": wallet_address,
                                    "mint": "993dVFL2uXWYeoXuEBFXR4BijeXdTv4s6BzsCjJZuwqk",
                                },
                            },
                        },
                        {
                            "program": "spl-token",
                            "parsed": {
                                "type": "transfer",
                                "info": {
                                    "amount": "20000002655",
                                    "authority": "DdZR6zRFiUt4S5mg7AV1uKB2z1f1WzcNYCaTEEWPAuby",
                                    "destination": "WalletUsdcATA",
                                    "source": "ReserveLiquiditySupplyATA",
                                },
                            },
                        },
                    ],
                }
            ],
        },
    }


def _solend_deposit_payload(wallet_address: str) -> dict:
    return {
        "data": [
            {
                "id": "solend-deposit-tx",
                "type": "transactions",
                "attributes": {
                    "hash": "osTUgy1BhnJH3da39vCJt1b9YnuPovkoZMhFB7bKQtNGjcuoVeS7LrS5jC7mncbeQfLiG1EwMkDDzXpg8GAtnVp",
                    "status": "confirmed",
                    "mined_at": "2022-10-14T04:53:49Z",
                    "mined_at_block": 155264292,
                    "operation_type": "send",
                    "fee": {"value": "5000"},
                    "transfers": [
                        {
                            "from": wallet_address,
                            "to": "So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo",
                            "quantity": {
                                "value": "79614266527",
                                "decimals": 6,
                            },
                            "fungible_info": {
                                "symbol": "USDC",
                                "name": "USD Coin",
                                "implementations": [
                                    {
                                        "chain_id": "solana",
                                        "address": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                                        "decimals": 6,
                                    }
                                ],
                            },
                        }
                    ],
                },
                "relationships": {
                    "chain": {"data": {"id": "solana"}},
                },
            }
        ],
        "links": {},
    }


def _parsed_solend_deposit_tx(wallet_address: str) -> dict:
    return {
        "slot": 155264292,
        "blockTime": 1665739229,
        "transaction": {
            "message": {
                "accountKeys": [
                    {"pubkey": wallet_address},
                    {"pubkey": "BhPLJeyTJAgdWm5iAWJzZS36E3Zt4qqCssdsxZh2vJ4U"},
                    {"pubkey": "8SheGtsopRUDzdiD6v6BR9a6bqZ9QwywYQY99Fp5meNf"},
                    {"pubkey": "D7XQyLSAhCBSvAm2k4FdN22Q6YFTUB3WfTw1yctuUPer"},
                    {"pubkey": "UtRy8gcEu9fCkDuUrU8EmC7Uc6FZy5NCwttzG7i6nkw"},
                    {"pubkey": "ReserveState"},
                    {"pubkey": "ReserveLiquiditySupplyATA"},
                ],
                "instructions": [
                    {
                        "programId": "So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo",
                        "accounts": [
                            wallet_address,
                            "BhPLJeyTJAgdWm5iAWJzZS36E3Zt4qqCssdsxZh2vJ4U",
                            "8SheGtsopRUDzdiD6v6BR9a6bqZ9QwywYQY99Fp5meNf",
                            "D7XQyLSAhCBSvAm2k4FdN22Q6YFTUB3WfTw1yctuUPer",
                            "UtRy8gcEu9fCkDuUrU8EmC7Uc6FZy5NCwttzG7i6nkw",
                            "ReserveState",
                            "ReserveLiquiditySupplyATA",
                        ],
                    }
                ],
            },
        },
        "meta": {
            "err": None,
            "fee": 5000,
            "preTokenBalances": [
                {
                    "accountIndex": 1,
                    "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                    "owner": wallet_address,
                    "uiTokenAmount": {"amount": "79614266527", "decimals": 6},
                },
                {
                    "accountIndex": 2,
                    "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                    "owner": "DdZR6zRFiUt4S5mg7AV1uKB2z1f1WzcNYCaTEEWPAuby",
                    "uiTokenAmount": {"amount": "29855421151822", "decimals": 6},
                },
                {
                    "accountIndex": 3,
                    "mint": "993dVFL2uXWYeoXuEBFXR4BijeXdTv4s6BzsCjJZuwqk",
                    "owner": wallet_address,
                    "uiTokenAmount": {"amount": "0", "decimals": 6},
                },
                {
                    "accountIndex": 4,
                    "mint": "993dVFL2uXWYeoXuEBFXR4BijeXdTv4s6BzsCjJZuwqk",
                    "owner": "DdZR6zRFiUt4S5mg7AV1uKB2z1f1WzcNYCaTEEWPAuby",
                    "uiTokenAmount": {"amount": "78510969397154", "decimals": 6},
                },
            ],
            "postTokenBalances": [
                {
                    "accountIndex": 1,
                    "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                    "owner": wallet_address,
                    "uiTokenAmount": {"amount": "0", "decimals": 6},
                },
                {
                    "accountIndex": 2,
                    "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                    "owner": "DdZR6zRFiUt4S5mg7AV1uKB2z1f1WzcNYCaTEEWPAuby",
                    "uiTokenAmount": {"amount": "29935035418349", "decimals": 6},
                },
                {
                    "accountIndex": 3,
                    "mint": "993dVFL2uXWYeoXuEBFXR4BijeXdTv4s6BzsCjJZuwqk",
                    "owner": wallet_address,
                    "uiTokenAmount": {"amount": "0", "decimals": 6},
                },
                {
                    "accountIndex": 4,
                    "mint": "993dVFL2uXWYeoXuEBFXR4BijeXdTv4s6BzsCjJZuwqk",
                    "owner": "DdZR6zRFiUt4S5mg7AV1uKB2z1f1WzcNYCaTEEWPAuby",
                    "uiTokenAmount": {"amount": "78586117392181", "decimals": 6},
                },
            ],
            "innerInstructions": [
                {
                    "index": 0,
                    "instructions": [
                        {
                            "program": "spl-token",
                            "parsed": {
                                "type": "transfer",
                                "info": {
                                    "amount": "79614266527",
                                    "authority": wallet_address,
                                    "destination": "8SheGtsopRUDzdiD6v6BR9a6bqZ9QwywYQY99Fp5meNf",
                                    "source": "BhPLJeyTJAgdWm5iAWJzZS36E3Zt4qqCssdsxZh2vJ4U",
                                },
                            },
                        },
                        {
                            "program": "spl-token",
                            "parsed": {
                                "type": "mintTo",
                                "info": {
                                    "account": "D7XQyLSAhCBSvAm2k4FdN22Q6YFTUB3WfTw1yctuUPer",
                                    "amount": "75147995027",
                                    "mint": "993dVFL2uXWYeoXuEBFXR4BijeXdTv4s6BzsCjJZuwqk",
                                    "mintAuthority": "DdZR6zRFiUt4S5mg7AV1uKB2z1f1WzcNYCaTEEWPAuby",
                                },
                            },
                        },
                        {
                            "program": "spl-token",
                            "parsed": {
                                "type": "transfer",
                                "info": {
                                    "amount": "75147995027",
                                    "authority": wallet_address,
                                    "destination": "UtRy8gcEu9fCkDuUrU8EmC7Uc6FZy5NCwttzG7i6nkw",
                                    "source": "D7XQyLSAhCBSvAm2k4FdN22Q6YFTUB3WfTw1yctuUPer",
                                },
                            },
                        },
                    ],
                }
            ],
        },
    }


class TestZerionClient:
    @patch("sync_crypto.zerion.requests.get")
    def test_fetch_new_transactions_uses_basic_auth_and_normalizes_records(self, mock_get):
        from sync_crypto.zerion import ZerionClient

        response = MagicMock()
        response.status_code = 200
        response.raise_for_status = MagicMock()
        response.json.return_value = _tx_payload(
            tx_id="tx-1",
            tx_hash="0xabc",
            mined_at="2025-01-01T00:00:00Z",
            chain_id="ethereum",
            token_symbol="USDC",
            token_name="USD Coin",
            token_contract="0xa0b86991",
            decimals=6,
            numeric_value="12.345678",
        )
        mock_get.return_value = response

        client = ZerionClient(api_key="zk_dev_test")
        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="main",
            address="0xwallet",
            provider="zerion",
            network="ethereum",
        )

        records, state = client.fetch_new_transactions(wallet)

        assert len(records) == 1
        record = records[0]
        assert record.record_id == "zerion:tx-1:0"
        assert record.provider == "zerion"
        assert record.network == "ethereum"
        assert record.tx_hash == "0xabc"
        assert record.blockchain == "ethereum"
        assert record.token_symbol == "USDC"
        assert record.token_name == "USD Coin"
        assert record.token_contract == "0xa0b86991"
        assert record.token_decimals == 6
        assert record.value == "12345678"
        assert record.fee == "21000000000000"
        assert record.status == "success"
        assert state["latest_record_ids"] == ["zerion:tx-1:0"]

        auth = mock_get.call_args.kwargs["auth"]
        assert auth.username == "zk_dev_test"
        assert auth.password == ""
        params = mock_get.call_args.kwargs["params"]
        assert params["filter[chain_ids]"] == "ethereum"
        assert params["page[size]"] == 100

    @patch("sync_crypto.zerion.requests.get")
    def test_fetch_new_transactions_follows_next_link_and_stops_at_state_boundary(self, mock_get):
        from sync_crypto.zerion import ZerionClient

        page1 = _tx_payload(
            tx_id="tx-new",
            tx_hash="0xnew",
            mined_at="2025-01-03T00:00:00Z",
            next_url="https://api.zerion.io/v1/wallets/0xwallet/transactions/?page%5Bafter%5D=cursor-2",
        )
        page2 = _tx_payload(
            tx_id="tx-old",
            tx_hash="0xold",
            mined_at="2025-01-01T00:00:00Z",
        )

        response1 = MagicMock()
        response1.status_code = 200
        response1.raise_for_status = MagicMock()
        response1.json.return_value = page1

        response2 = MagicMock()
        response2.status_code = 200
        response2.raise_for_status = MagicMock()
        response2.json.return_value = page2

        mock_get.side_effect = [response1, response2]

        client = ZerionClient(api_key="zk_dev_test")
        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="main",
            address="0xwallet",
            provider="zerion",
        )

        records, state = client.fetch_new_transactions(
            wallet,
            state={
                "latest_timestamp": 1735689600,
                "latest_record_ids": ["zerion:tx-old:0"],
            },
        )

        assert len(records) == 1
        assert records[0].record_id == "zerion:tx-new:0"
        assert state["latest_record_ids"] == ["zerion:tx-new:0"]
        assert mock_get.call_count == 2

        second_call = mock_get.call_args_list[1]
        assert second_call.kwargs["params"]["page[after]"] == "cursor-2"

    @patch("sync_crypto.zerion.requests.get")
    def test_fetch_new_transaction_batches_yield_page_records_before_boundary(self, mock_get):
        from sync_crypto.zerion import ZerionClient

        page1 = _tx_payload(
            tx_id="tx-new",
            tx_hash="0xnew",
            mined_at="2025-01-03T00:00:00Z",
            next_url="https://api.zerion.io/v1/wallets/0xwallet/transactions/?page%5Bafter%5D=cursor-2",
        )
        page2 = _tx_payload(
            tx_id="tx-old",
            tx_hash="0xold",
            mined_at="2025-01-01T00:00:00Z",
        )

        response1 = MagicMock()
        response1.status_code = 200
        response1.raise_for_status = MagicMock()
        response1.json.return_value = page1

        response2 = MagicMock()
        response2.status_code = 200
        response2.raise_for_status = MagicMock()
        response2.json.return_value = page2

        mock_get.side_effect = [response1, response2]

        client = ZerionClient(api_key="zk_dev_test")
        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="main",
            address="0xwallet",
            provider="zerion",
        )

        batches = list(client.fetch_new_transaction_batches(
            wallet,
            state={
                "latest_timestamp": 1735689600,
                "latest_record_ids": ["zerion:tx-old:0"],
            },
        ))

        assert len(batches) == 1
        assert len(batches[0]) == 1
        assert batches[0][0].record_id == "zerion:tx-new:0"

    def test_advance_state_merges_record_ids_for_matching_latest_timestamp(self):
        from sync_crypto.zerion import ZerionClient

        first = TransactionRecord(
            record_id="zerion:tx-1:0",
            provider="zerion",
            network="ethereum",
            tx_hash="0xshared",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xfrom",
            to_address="0xto",
            value="1",
            fee="1",
            status="success",
        )
        second = TransactionRecord(
            record_id="zerion:tx-1:1",
            provider="zerion",
            network="ethereum",
            tx_hash="0xshared",
            blockchain="ethereum",
            timestamp=1700000000,
            from_address="0xfrom",
            to_address="0xto2",
            value="2",
            fee="1",
            status="success",
        )

        state = ZerionClient.advance_state({}, [first])
        state = ZerionClient.advance_state(state, [second])

        assert state["provider"] == "zerion"
        assert state["latest_timestamp"] == 1700000000
        assert state["latest_record_ids"] == ["zerion:tx-1:0", "zerion:tx-1:1"]

    @patch("sync_crypto.zerion.requests.get")
    def test_fetch_new_transactions_enriches_ethereum_rows_with_shared_metadata(self, mock_get):
        from sync_crypto.zerion import ZerionClient

        payload = _tx_payload(
            tx_id="tx-1",
            tx_hash="0xabc",
            mined_at="2025-01-01T00:00:00Z",
            chain_id="ethereum",
            token_symbol="USDC",
            token_name="USD Coin",
            token_contract="0xa0b86991",
            decimals=6,
            numeric_value="12.345678",
        )
        payload["data"][0]["attributes"]["transfers"].append(
            {
                "from": "0xpool",
                "to": "0xwallet",
                "quantity": {
                    "numeric": "0.42",
                    "decimals": 18,
                },
                "fungible_info": {
                    "symbol": "WETH",
                    "name": "Wrapped Ether",
                    "implementations": [
                        {
                            "address": "0xC02aaA39b223FE8D0A0E5C4F27eAD9083C756Cc2",
                            "chain_id": "ethereum",
                            "decimals": 18,
                        }
                    ],
                },
            }
        )

        response = MagicMock()
        response.status_code = 200
        response.raise_for_status = MagicMock()
        response.json.return_value = payload
        mock_get.return_value = response

        client = ZerionClient(api_key="zk_dev_test", etherscan_api_key="eth_key")
        mock_eth_client = MagicMock()
        mock_eth_client.fetch_transaction_metadata.return_value = {
            "block_number": 18500000,
            "gas_used": "21000",
            "gas_price": "1000000000",
            "input_data": "0xa9059cbb0000000000000000",
            "tx_receipt_status": "1",
            "transaction_index": 7,
            "cumulative_gas_used": "600000",
            "confirmations": 12345,
            "method_id": "0xa9059cbb",
        }
        client._etherscan_client_for_network = MagicMock(return_value=mock_eth_client)

        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="main",
            address="0xwallet",
            provider="zerion",
            network="ethereum",
        )

        records, _ = client.fetch_new_transactions(wallet)

        assert len(records) == 2
        assert [record.record_id for record in records] == ["zerion:tx-1:0", "zerion:tx-1:1"]
        for record in records:
            assert record.tx_hash == "0xabc"
            assert record.gas_used == "21000"
            assert record.gas_price == "1000000000"
            assert record.input_data == "0xa9059cbb0000000000000000"
            assert record.tx_receipt_status == "1"
            assert record.transaction_index == 7
            assert record.cumulative_gas_used == "600000"
            assert record.confirmations == 12345
            assert record.method_id == "0xa9059cbb"

        client._etherscan_client_for_network.assert_called_once_with("ethereum")
        mock_eth_client.fetch_transaction_metadata.assert_called_once_with("0xabc")

    @patch("sync_crypto.zerion.requests.get")
    def test_fetch_new_transactions_keeps_ethereum_rows_when_enrichment_fails(self, mock_get):
        from sync_crypto.zerion import ZerionClient

        response = MagicMock()
        response.status_code = 200
        response.raise_for_status = MagicMock()
        response.json.return_value = _tx_payload(
            tx_id="tx-1",
            tx_hash="0xabc",
            mined_at="2025-01-01T00:00:00Z",
            chain_id="ethereum",
        )
        mock_get.return_value = response

        client = ZerionClient(api_key="zk_dev_test", etherscan_api_key="eth_key")
        mock_eth_client = MagicMock()
        mock_eth_client.fetch_transaction_metadata.side_effect = RuntimeError("boom")
        client._etherscan_client_for_network = MagicMock(return_value=mock_eth_client)

        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="main",
            address="0xwallet",
            provider="zerion",
            network="ethereum",
        )

        records, _ = client.fetch_new_transactions(wallet)

        assert len(records) == 1
        assert records[0].record_id == "zerion:tx-1:0"
        assert records[0].gas_used is None
        assert records[0].gas_price is None
        assert records[0].input_data is None
        assert records[0].tx_receipt_status is None

    @patch("sync_crypto.zerion.requests.get")
    def test_fetch_new_transactions_defaults_solana_to_solana_chain_filter(self, mock_get):
        from sync_crypto.zerion import ZerionClient

        response = MagicMock()
        response.status_code = 200
        response.raise_for_status = MagicMock()
        response.json.return_value = {"data": [], "links": {}}
        mock_get.return_value = response

        client = ZerionClient(api_key="zk_dev_test")
        wallet = WalletConfig(
            blockchain="solana",
            friendly_name="sol",
            address="So11111111111111111111111111111111111111112",
            provider="zerion",
        )

        client.fetch_new_transactions(wallet)

        params = mock_get.call_args.kwargs["params"]
        assert params["filter[chain_ids]"] == "solana"

    @patch("sync_crypto.zerion.requests.get")
    def test_fetch_new_transactions_retries_5xx_with_smaller_page_size(self, mock_get):
        from sync_crypto.zerion import ZerionClient

        page1 = _tx_payload(
            tx_id="tx-new",
            tx_hash="0xnew",
            mined_at="2025-01-03T00:00:00Z",
            chain_id="solana",
            next_url=(
                "https://api.zerion.io/v1/wallets/So11111111111111111111111111111111111111112/"
                "transactions/?page%5Bafter%5D=cursor-2&page%5Bsize%5D=100"
            ),
        )
        page2 = _tx_payload(
            tx_id="tx-old",
            tx_hash="0xold",
            mined_at="2025-01-02T00:00:00Z",
            chain_id="solana",
        )

        response1 = MagicMock()
        response1.status_code = 200
        response1.raise_for_status = MagicMock()
        response1.json.return_value = page1

        response500 = MagicMock()
        response500.status_code = 500
        response500.raise_for_status.side_effect = requests.HTTPError("500 Server Error")

        response2 = MagicMock()
        response2.status_code = 200
        response2.raise_for_status = MagicMock()
        response2.json.return_value = page2

        mock_get.side_effect = [response1, response500, response2]

        client = ZerionClient(api_key="zk_dev_test")
        wallet = WalletConfig(
            blockchain="solana",
            friendly_name="sol",
            address="So11111111111111111111111111111111111111112",
            provider="zerion",
        )

        records, state = client.fetch_new_transactions(wallet)

        assert len(records) == 2
        assert state["latest_record_ids"] == ["zerion:tx-new:0"]

        first_retry_call = mock_get.call_args_list[1]
        assert first_retry_call.kwargs["params"]["page[after]"] == "cursor-2"
        assert str(first_retry_call.kwargs["params"]["page[size]"]) == "100"

        second_retry_call = mock_get.call_args_list[2]
        assert second_retry_call.kwargs["params"]["page[after]"] == "cursor-2"
        assert str(second_retry_call.kwargs["params"]["page[size]"]) == "50"

    @patch("sync_crypto.zerion.requests.get")
    def test_fetch_new_transactions_prefers_matching_chain_implementation(self, mock_get):
        from sync_crypto.zerion import ZerionClient

        response = MagicMock()
        response.status_code = 200
        response.raise_for_status = MagicMock()
        payload = _tx_payload(
            tx_id="tx-sol",
            tx_hash="solhash",
            mined_at="2025-01-01T00:00:00Z",
            chain_id="solana",
            token_symbol="USDC",
            token_name="USD Coin",
            token_contract=None,
            decimals=6,
            numeric_value="12.3",
        )
        payload["data"][0]["attributes"]["transfers"][0]["fungible_info"]["implementations"] = [
            {"address": "0xwrong", "chain_id": "0g", "decimals": 6},
            {"address": "So11111111111111111111111111111111111111112", "chain_id": "solana", "decimals": 6},
        ]
        response.json.return_value = payload
        mock_get.return_value = response

        client = ZerionClient(api_key="zk_dev_test")
        wallet = WalletConfig(
            blockchain="solana",
            friendly_name="sol",
            address="SoWallet1111111111111111111111111111111111111",
            provider="zerion",
        )

        records, _ = client.fetch_new_transactions(wallet)

        assert len(records) == 1
        assert records[0].network == "solana"
        assert records[0].token_contract == "So11111111111111111111111111111111111111112"

    @patch("sync_crypto.zerion.requests.get")
    def test_fetch_new_transactions_enriches_sparse_solana_rows_with_rpc_transfers(self, mock_get):
        from sync_crypto.zerion import ZerionClient

        wallet_address = "2baaTDzidWekQWQydZcBSwHXgqK1LF9QUUn7VioUrVVD"
        response = MagicMock()
        response.status_code = 200
        response.raise_for_status = MagicMock()
        response.json.return_value = _sparse_solana_trade_payload(wallet_address)
        mock_get.return_value = response

        mock_solana_rpc = MagicMock()
        mock_solana_rpc._rpc_call.return_value = _parsed_solana_trade_tx(wallet_address)

        client = ZerionClient(
            api_key="zk_dev_test",
            solana_rpc_client=mock_solana_rpc,
        )
        wallet = WalletConfig(
            blockchain="solana",
            friendly_name="sol",
            address=wallet_address,
            provider="zerion",
        )

        records, _ = client.fetch_new_transactions(wallet)

        assert len(records) == 3
        assert records[0].token_symbol == "SOL"

        send_row = next(record for record in records if record.token_contract == "RAYMINT")
        assert send_row.value == "27000000"
        assert send_row.method == "trade"
        assert send_row.from_address == wallet_address
        assert send_row.to_address == "MarketRAY"

        receive_row = next(record for record in records if record.token_contract == "USDCMINT")
        assert receive_row.value == "380320450"
        assert receive_row.method == "trade"
        assert receive_row.from_address == "MarketUSDC"
        assert receive_row.to_address == wallet_address

    @patch("sync_crypto.zerion.requests.get")
    def test_enrichment_handles_outer_only_spl_token_transfer(self, mock_get):
        """Regression: a direct ATA-to-ATA transfer signed by the wallet is a
        single OUTER `spl-token transferChecked` instruction with no inner
        instructions. The pre-fix enrichment loop only walked
        `meta.innerInstructions` so these transactions stayed as bare
        `tx_type=execute, value=0` placeholders, dropping ~$51k of real
        USDC inflows for the 2baaTDz/EEbn9C sister wallets in 2021-08.
        """
        from sync_crypto.zerion import ZerionClient

        wallet_address = "EEbn9Cm91zkDRh9M9aHfBnmZzGsSAgiVddPHRaCfi53m"
        dest_ata = "546Y1tZc4hvLbXCH8cjYDMuBfBXtgqDvFXRBnwnqLb37"
        tx_hash = "3h79FszB8QEFGddhjhcr66zAaxPBtwegeUHaizRzRircD8Z1yw62VE61M3gA4E9UaRRKtyWKMqPoxowjba5tv2Ry"

        response = MagicMock()
        response.status_code = 200
        response.raise_for_status = MagicMock()
        response.json.return_value = _bare_execute_solana_payload(wallet_address, tx_hash)
        mock_get.return_value = response

        mock_solana_rpc = MagicMock()
        mock_solana_rpc._rpc_call.return_value = _parsed_solana_direct_transfer_tx(
            wallet_address, dest_ata
        )

        client = ZerionClient(
            api_key="zk_dev_test",
            solana_rpc_client=mock_solana_rpc,
        )
        wallet = WalletConfig(
            blockchain="solana",
            friendly_name="sol",
            address=wallet_address,
            provider="zerion",
        )

        records, _ = client.fetch_new_transactions(wallet)

        send_rows = [r for r in records if r.tx_type == "token_transfer" and r.method == "send"]
        assert len(send_rows) == 1, f"expected 1 send row, got {len(send_rows)}: {records}"
        send_row = send_rows[0]
        assert send_row.value == "20265700000"
        assert send_row.token_contract == "USDCMINT"
        assert send_row.from_address == wallet_address
        assert send_row.to_address == dest_ata

    @patch("sync_crypto.zerion.requests.get")
    def test_enrichment_ignores_receives_into_token_accounts_not_owned_by_wallet(self, mock_get):
        """Regression: Jupiter fill_order references the beneficiary wallet in its
        outer-instruction accounts list, but intermediate USDC/WSOL token accounts
        are owned by the signer, not by the beneficiary. The enrichment must not
        attribute those intermediate transfers to the beneficiary wallet.
        """
        from sync_crypto.zerion import ZerionClient

        beneficiary = "3FXQVRb1kEXeP7pmsrdUnfEXVdszj9dVZyfY3shx4gnt"
        signer = "j1oxqtEHFn7rUkdABJLmtVtz5fFmHFs4tCG3fWJnkHX"

        zerion_payload = {
            "data": [
                {
                    "id": "fill-order-tx",
                    "type": "transactions",
                    "attributes": {
                        "hash": "iWnCcL1NcrdGoqQmwJ52kcA2h9DgyREC8J8V5JY9RK5H5rZFgcwxaeSdgBXZvrYaw33vekGCzRe8iVFK9M42p7s",
                        "status": "confirmed",
                        "mined_at": "2025-08-28T01:15:08Z",
                        "mined_at_block": 362969638,
                        "operation_type": "receive",
                        "fee": {"value": "10305"},
                        "transfers": [
                            {
                                "from": signer,
                                "to": beneficiary,
                                "quantity": {"value": "5483871307", "decimals": 8},
                                "fungible_info": {
                                    "symbol": "HNT",
                                    "name": "Helium",
                                    "implementations": [
                                        {"chain_id": "solana", "decimals": 8, "address": "HNTMINT"},
                                    ],
                                },
                            }
                        ],
                    },
                    "relationships": {"chain": {"data": {"id": "solana"}}},
                }
            ],
            "links": {},
        }

        # Parsed RPC tx: Jupiter fill_order swapping signer's USDC for HNT, then
        # forwarding HNT to beneficiary. Beneficiary is referenced in the outer
        # instruction accounts but owns only the HNT destination ATA.
        parsed_tx = {
            "slot": 362969638,
            "blockTime": 1756343708,
            "meta": {
                "err": None,
                "fee": 10305,
                "preTokenBalances": [
                    {
                        "accountIndex": 2,
                        "mint": "USDCMINT",
                        "owner": signer,
                        "uiTokenAmount": {"amount": "200000000", "decimals": 6},
                    },
                    {
                        "accountIndex": 3,
                        "mint": "USDCMINT",
                        "owner": "JupiterMarket",
                        "uiTokenAmount": {"amount": "0", "decimals": 6},
                    },
                    {
                        "accountIndex": 4,
                        "mint": "HNTMINT",
                        "owner": signer,
                        "uiTokenAmount": {"amount": "0", "decimals": 8},
                    },
                    {
                        "accountIndex": 5,
                        "mint": "HNTMINT",
                        "owner": beneficiary,
                        "uiTokenAmount": {"amount": "0", "decimals": 8},
                    },
                ],
                "postTokenBalances": [
                    {
                        "accountIndex": 2,
                        "mint": "USDCMINT",
                        "owner": signer,
                        "uiTokenAmount": {"amount": "55874980", "decimals": 6},
                    },
                    {
                        "accountIndex": 3,
                        "mint": "USDCMINT",
                        "owner": "JupiterMarket",
                        "uiTokenAmount": {"amount": "144125020", "decimals": 6},
                    },
                    {
                        "accountIndex": 4,
                        "mint": "HNTMINT",
                        "owner": signer,
                        "uiTokenAmount": {"amount": "0", "decimals": 8},
                    },
                    {
                        "accountIndex": 5,
                        "mint": "HNTMINT",
                        "owner": beneficiary,
                        "uiTokenAmount": {"amount": "5483871307", "decimals": 8},
                    },
                ],
                "innerInstructions": [
                    {
                        "index": 0,
                        "instructions": [
                            # Signer swaps USDC into Jupiter market (owned by JupiterMarket).
                            {
                                "program": "spl-token",
                                "parsed": {
                                    "type": "transfer",
                                    "info": {
                                        "amount": "144125020",
                                        "authority": signer,
                                        "source": "SignerUSDCATA",
                                        "destination": "JupiterUSDCATA",
                                    },
                                },
                            },
                            # Signer forwards HNT output to beneficiary's HNT ATA.
                            {
                                "program": "spl-token",
                                "parsed": {
                                    "type": "transfer",
                                    "info": {
                                        "amount": "5483871307",
                                        "authority": signer,
                                        "source": "SignerHNTATA",
                                        "destination": "BeneficiaryHNTATA",
                                    },
                                },
                            },
                        ],
                    }
                ],
            },
            "transaction": {
                "message": {
                    "accountKeys": [
                        {"pubkey": signer, "signer": True},
                        {"pubkey": "JupiterLimitOrderV1", "signer": False},
                        {"pubkey": "SignerUSDCATA", "signer": False},
                        {"pubkey": "JupiterUSDCATA", "signer": False},
                        {"pubkey": "SignerHNTATA", "signer": False},
                        {"pubkey": "BeneficiaryHNTATA", "signer": False},
                        {"pubkey": beneficiary, "signer": False},
                    ],
                    "instructions": [
                        {
                            "programId": "JupiterLimitOrderV1",
                            # Beneficiary appears here, which is why the old guard
                            # let intermediate transfers through incorrectly.
                            "accounts": [
                                signer,
                                "SignerUSDCATA",
                                "JupiterUSDCATA",
                                "SignerHNTATA",
                                "BeneficiaryHNTATA",
                                beneficiary,
                            ],
                        }
                    ],
                },
            },
        }

        response = MagicMock()
        response.status_code = 200
        response.raise_for_status = MagicMock()
        response.json.return_value = zerion_payload
        mock_get.return_value = response

        mock_solana_rpc = MagicMock()
        mock_solana_rpc._rpc_call.return_value = parsed_tx

        client = ZerionClient(api_key="zk_dev_test", solana_rpc_client=mock_solana_rpc)
        wallet = WalletConfig(
            blockchain="solana",
            friendly_name="sol",
            address=beneficiary,
            provider="zerion",
        )

        records, _ = client.fetch_new_transactions(wallet)

        # Only the single HNT receive belongs to the beneficiary wallet.
        assert len(records) == 1
        record = records[0]
        assert record.token_contract == "HNTMINT"
        assert record.value == "5483871307"
        assert record.to_address == beneficiary
        assert record.from_address == signer

    @patch("sync_crypto.zerion.requests.get")
    def test_enrichment_adds_burn_companion_receive_for_solend_redeem(self, mock_get):
        from sync_crypto.zerion import ZerionClient

        wallet_address = "3FXQVRb1kEXeP7pmsrdUnfEXVdszj9dVZyfY3shx4gnt"
        response = MagicMock()
        response.status_code = 200
        response.raise_for_status = MagicMock()
        response.json.return_value = _solend_redeem_payload()
        mock_get.return_value = response

        mock_solana_rpc = MagicMock()
        mock_solana_rpc._rpc_call.return_value = _parsed_solend_redeem_tx(wallet_address)

        client = ZerionClient(api_key="zk_dev_test", solana_rpc_client=mock_solana_rpc)
        wallet = WalletConfig(
            blockchain="solana",
            friendly_name="sol",
            address=wallet_address,
            provider="zerion",
        )

        records, _ = client.fetch_new_transactions(wallet)

        assert len(records) == 2
        execute_row = next(record for record in records if record.method == "execute")
        assert execute_row.value == "0"

        receive_row = next(record for record in records if record.method == "receive")
        assert receive_row.token_contract == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
        assert receive_row.value == "20000002655"
        assert receive_row.to_address == wallet_address

    @patch("sync_crypto.zerion.requests.get")
    def test_enrichment_adds_wallet_mint_receive_for_solend_deposit(self, mock_get):
        from sync_crypto.zerion import ZerionClient

        wallet_address = "3FXQVRb1kEXeP7pmsrdUnfEXVdszj9dVZyfY3shx4gnt"
        response = MagicMock()
        response.status_code = 200
        response.raise_for_status = MagicMock()
        response.json.return_value = _solend_deposit_payload(wallet_address)
        mock_get.return_value = response

        mock_solana_rpc = MagicMock()
        mock_solana_rpc._rpc_call.return_value = _parsed_solend_deposit_tx(wallet_address)

        client = ZerionClient(api_key="zk_dev_test", solana_rpc_client=mock_solana_rpc)
        wallet = WalletConfig(
            blockchain="solana",
            friendly_name="sol",
            address=wallet_address,
            provider="zerion",
        )

        records, _ = client.fetch_new_transactions(wallet)

        assert len(records) == 3

        usdc_send = next(
            record
            for record in records
            if record.method == "send"
            and record.token_contract == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
        )
        assert usdc_send.value == "79614266527"
        assert usdc_send.from_address == wallet_address

        receipt_receive = next(
            record
            for record in records
            if record.method == "receive"
            and record.token_contract == "993dVFL2uXWYeoXuEBFXR4BijeXdTv4s6BzsCjJZuwqk"
        )
        assert receipt_receive.value == "75147995027"
        assert receipt_receive.from_address == "DdZR6zRFiUt4S5mg7AV1uKB2z1f1WzcNYCaTEEWPAuby"
        assert receipt_receive.to_address == wallet_address

        receipt_send = next(
            record
            for record in records
            if record.method == "send"
            and record.token_contract == "993dVFL2uXWYeoXuEBFXR4BijeXdTv4s6BzsCjJZuwqk"
        )
        assert receipt_send.value == "75147995027"
        assert receipt_send.from_address == wallet_address

    @patch("sync_crypto.zerion.requests.get")
    def test_normalizes_all_transfers_to_item_chain_when_transfer_chain_disagrees(self, mock_get):
        # Regression for tx 0x26c2afa4...170738 — a Uniswap V3 swap on
        # Arbitrum where Zerion stamped the native ETH leg with
        # chain_id="abstract" while the two USDC.e legs were correctly
        # on "arbitrum". A single tx hash exists on exactly one chain,
        # so all transfers must inherit the item-level chain.
        from sync_crypto.zerion import ZerionClient

        wallet_address = "0x6d25d07f5c0dccd0d6c7b3342cd83b902464f06b"
        usdc_e_contract = "0xff970a61a04b1ca14834a43f5de4533ebddb5cc8"
        payload = {
            "data": [
                {
                    "id": "tx-arb-swap",
                    "type": "transactions",
                    "attributes": {
                        "hash": "0x26c2afa41c88c4e4ac8d5567ad5bee2b72ec6eb2fc9000a8944addb35f170738",
                        "status": "confirmed",
                        "mined_at": "2022-05-12T05:17:05Z",
                        "mined_at_block": 11869088,
                        "operation_type": "trade",
                        "fee": {"value": "1743596197197375"},
                        "transfers": [
                            {
                                "from": "0xc31e54c7a869b9fcbecc14363cf510d1c41fa443",
                                "to": wallet_address,
                                "quantity": {"numeric": "28793.623841", "decimals": 6},
                                "fungible_info": {
                                    "symbol": "USDC.e",
                                    "name": "USDC (Arbitrum)",
                                    "implementations": [
                                        {"address": usdc_e_contract, "chain_id": "arbitrum", "decimals": 6},
                                    ],
                                },
                                "chain_id": "arbitrum",
                            },
                            {
                                "from": "0x17c14d2c404d167802b16c450d3c99f88f2c4f4d",
                                "to": wallet_address,
                                "quantity": {"numeric": "9596.881788", "decimals": 6},
                                "fungible_info": {
                                    "symbol": "USDC.e",
                                    "name": "USDC (Arbitrum)",
                                    "implementations": [
                                        {"address": usdc_e_contract, "chain_id": "arbitrum", "decimals": 6},
                                    ],
                                },
                                "chain_id": "arbitrum",
                            },
                            {
                                "from": wallet_address,
                                "to": "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45",
                                "quantity": {"numeric": "21", "decimals": 18},
                                "fungible_info": {
                                    "symbol": "ETH",
                                    "name": "Ethereum",
                                    "implementations": [],
                                },
                                "chain_id": "abstract",
                            },
                        ],
                    },
                    "relationships": {
                        "chain": {"data": {"id": "arbitrum"}},
                    },
                }
            ],
            "links": {},
        }

        response = MagicMock()
        response.status_code = 200
        response.raise_for_status = MagicMock()
        response.json.return_value = payload
        mock_get.return_value = response

        client = ZerionClient(api_key="zk_dev_test")
        wallet = WalletConfig(
            blockchain="ethereum",
            friendly_name="main",
            address=wallet_address,
            provider="zerion",
        )

        records, _ = client.fetch_new_transactions(wallet)

        assert len(records) == 3
        networks = {record.network for record in records}
        assert networks == {"arbitrum"}, (
            f"all transfers under one tx hash should share the item-level chain, got {networks}"
        )

    # The parametrize cases below pair a real Zerion `data[]` item with
    # the matching live Helius `getTransaction` response, captured from
    # the 2baa Solana wallet on 2026-04-29. Each case names the expected
    # set of (token_contract, value) -> method tuples so the assertion
    # is specific about which receives must be emitted (or suppressed).
    _LIVE_WALLET = "2baaTDzidWekQWQydZcBSwHXgqK1LF9QUUn7VioUrVVD"

    @pytest.mark.parametrize("label,expected", [
        # Case 1: legacy execute tx; Helius `pre/postTokenBalances` have NO
        # `owner` field. Three intermediate-mint receives (gSAIL/APEX/FAB)
        # must still get emitted because we cannot verify ownership for
        # legacy-shape responses, so we fall back to delta-based inclusion.
        ("legacy_lost_2021_dec_4row", {
            ("5rTCvZq6BcApsC3VV1EEUuTJfaVd8uYhcGjwTy1By6P8", "0"): "execute",
            ("Gsai2KN28MTGcSZ1gKYFswUpFpS7EM9mvdR9c8f6iVXJ", "284211897"): "receive",
            ("51tMb3zBKDiQhNwGqpgwbavaGH54mk8fXFzxTc1xnasg", "4305457923"): "receive",
            ("EdAhkbj5nF9sRM7XN7ewuW8C9XEUMs8P7cnoQ57SYE96", "1483978659557"): "receive",
        }),
        # Case 2: legacy execute tx, single USDC receive.
        ("legacy_lost_2021_nov_a_2row", {
            ("", "0"): "execute",
            ("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "280000000"): "receive",
        }),
        # Case 3: another legacy 3-mint receive at different time/values.
        ("legacy_lost_2021_nov_b_4row", {
            ("5rTCvZq6BcApsC3VV1EEUuTJfaVd8uYhcGjwTy1By6P8", "0"): "execute",
            ("Gsai2KN28MTGcSZ1gKYFswUpFpS7EM9mvdR9c8f6iVXJ", "131295752"): "receive",
            ("51tMb3zBKDiQhNwGqpgwbavaGH54mk8fXFzxTc1xnasg", "2533749792"): "receive",
            ("EdAhkbj5nF9sRM7XN7ewuW8C9XEUMs8P7cnoQ57SYE96", "889196975984"): "receive",
        }),
        # Case 4: modern Jupiter aggregator swap. `owner` IS populated, and
        # the intermediate USDC/RAY transfers belong to routing accounts
        # (not the wallet's ATAs). The strict guard must keep suppressing
        # them — only the two Zerion-side trade legs survive.
        ("modern_jupiter_routing_2024", {
            ("", "11668270813"): "trade",
            ("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "1420000000"): "trade",
        }),
        # Case 5: modern Jupiter trade where the synthetic `:rpc:` send is
        # genuinely the wallet's. Already passing today; must not regress.
        ("modern_survived_2022_jun", {
            ("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "3646487"): "trade",
            ("TuLipcqtGVXP9XR62wM8WWCm6a9vhLs7T1uoWBk6FDs", "1087622"): "trade",
            ("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "5477"): "trade",
        }),
        # Case 6: modern multi-mint receive; owner populated. Already passing.
        ("modern_survived_2022_jan", {
            ("EdAhkbj5nF9sRM7XN7ewuW8C9XEUMs8P7cnoQ57SYE96", "124459233"): "receive",
            ("51tMb3zBKDiQhNwGqpgwbavaGH54mk8fXFzxTc1xnasg", "329584"): "receive",
            ("5rTCvZq6BcApsC3VV1EEUuTJfaVd8uYhcGjwTy1By6P8", "41198024725348"): "receive",
        }),
        # Case 7: simple SOL send; no enrichment expected from either code path.
        ("no_enrichment_2026_apr", {
            ("", "824880"): "send",
        }),
    ])
    @patch("sync_crypto.zerion.requests.get")
    def test_enrichment_against_live_zerion_helius_pairs(self, mock_get, label, expected):
        """End-to-end against captured live data: drives the same enrichment
        path the rebuild uses, with realistic Zerion items and Helius
        responses that exercise the legacy (no-owner) and modern (owner-
        populated) shapes.
        """
        from sync_crypto.zerion import ZerionClient

        zerion_item = load_fixture(f"zerion_item_{label}.json")
        helius_response = load_fixture(f"solana_rpc_{label}.json")

        response = MagicMock()
        response.status_code = 200
        response.raise_for_status = MagicMock()
        response.json.return_value = {"data": [zerion_item], "links": {}}
        mock_get.return_value = response

        mock_solana_rpc = MagicMock()
        mock_solana_rpc._rpc_call.return_value = helius_response

        client = ZerionClient(api_key="zk_dev_test", solana_rpc_client=mock_solana_rpc)
        wallet = WalletConfig(
            blockchain="solana",
            friendly_name="2baa",
            address=self._LIVE_WALLET,
            provider="zerion",
        )

        records, _ = client.fetch_new_transactions(wallet)

        actual = {(r.token_contract or "", r.value): r.method for r in records}
        assert actual == expected, (
            f"\n[{label}] records mismatch\n"
            f"  expected: {sorted(expected.items())}\n"
            f"  actual:   {sorted(actual.items())}"
        )
        # Sanity: receives must land on the wallet, sends originate from it.
        for record in records:
            if record.method == "receive":
                assert record.to_address == self._LIVE_WALLET
            elif record.method in {"send"}:
                assert record.from_address == self._LIVE_WALLET

    @patch("sync_crypto.zerion.requests.get")
    def test_enrichment_recovers_legacy_marinade_stake_mint(self, mock_get):
        """Regression: live 2021-09-22 Marinade stake on EEbn9C wallet
        (tx 4umcmkQ7…). Pre-2022 Helius `pre/postTokenBalances` carry no
        `owner` field, so `_append_wallet_mint_receives` previously
        dropped the +2,580 mSOL stake-mint into the wallet's ATA — even
        though the wallet was right there in the Marinade outer-
        instruction's accounts list. That single missing receive caused
        EEbn9C's mSOL balance to read −2,219.6 phantom instead of
        clearing to ~0 after the 2024 unstake. Fixed by mirroring the
        transfer-side outer-accounts fallback in the mint-receive code
        when the response has no owner data anywhere.
        """
        from sync_crypto.zerion import ZerionClient

        wallet_address = "EEbn9Cm91zkDRh9M9aHfBnmZzGsSAgiVddPHRaCfi53m"
        zerion_item = load_fixture("zerion_item_legacy_marinade_stake_2021_sep.json")
        helius_response = load_fixture("solana_rpc_legacy_marinade_stake_2021_sep.json")

        response = MagicMock()
        response.status_code = 200
        response.raise_for_status = MagicMock()
        response.json.return_value = {"data": [zerion_item], "links": {}}
        mock_get.return_value = response

        mock_solana_rpc = MagicMock()
        mock_solana_rpc._rpc_call.return_value = helius_response

        client = ZerionClient(api_key="zk_dev_test", solana_rpc_client=mock_solana_rpc)
        wallet = WalletConfig(
            blockchain="solana",
            friendly_name="EEbn9C",
            address=wallet_address,
            provider="zerion",
        )

        records, _ = client.fetch_new_transactions(wallet)

        msol_mint = "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So"
        receives = [r for r in records if r.token_contract == msol_mint and r.method == "receive"]
        assert len(receives) == 1, (
            f"expected 1 mSOL receive, got {len(receives)}: "
            f"{[(r.method, r.token_contract, r.value) for r in records]}"
        )
        receive = receives[0]
        assert receive.value == "2580157977558"
        assert receive.to_address == wallet_address

    # _append_wallet_mint_receives must not (a) duplicate an existing base
    # record that already covers the same (mint, value) just because the
    # direction word differs (Zerion's `deposit` vs the synthesizer's
    # forced `receive`), (b) emit zero-value records for `mintTo amount=0`
    # initialisation instructions, or (c) emit two synthetic records with
    # the same record_id when a tx has multiple mintTo at the same outer
    # instruction sharing the same mint.
    @pytest.mark.parametrize("wallet_address,label,expected", [
        # Wormhole UST bridge in: Zerion's `:0` base reports the wallet
        # already received 949,900 UST (operation_type="deposit"). The
        # bridge emits two SPL mintTo instructions in the same outer ix —
        # one for the real amount, one a zero-value sentinel. The current
        # code emits TWO synthetic mint-receive rows that both duplicate
        # the base, share a record_id, and one of them has value=0.
        # Expected after fix: just the two Zerion base records.
        (
            "HgspjimVL6zisiEVTStrbpA4D9D8go4GnTjCFJontaC9",
            "mintreceive_wormhole_ust_bridge",
            {
                ("9vMJfxuKxXBoEa7rM12mYLMwTacLMLDJqHozw96WQL8i", "949900000000"): "deposit",
                ("", "897840"): "deposit",
            },
        ),
        # Liquid-staking deposit (Parrot yPRT): wallet sends PRT, receives
        # yPRT. Zerion's `:0` base reports the yPRT mint with
        # operation_type="mint". The synthesizer emits an extra
        # mint-receive row with method="receive" that duplicates the
        # base's (mint, value). The legitimate `:rpc:1:send:PRT` (the user
        # actually staking PRT) must remain.
        (
            "2baaTDzidWekQWQydZcBSwHXgqK1LF9QUUn7VioUrVVD",
            "mintreceive_lst_yprt_deposit",
            {
                ("yPRTUpLDftNej7p6QofNYgRArRXsm6Mvkzohj4bh4WM", "593265384"): "mint",
                ("PRT88RkA4Kg5z7pKnezeNH4mafTvtQdfFgpQTGRjz44", "593265384"): "send",
            },
        ),
    ])
    @patch("sync_crypto.zerion.requests.get")
    def test_mint_receive_dedupes_against_base_and_skips_zero_value(self, mock_get, wallet_address, label, expected):
        from sync_crypto.zerion import ZerionClient

        zerion_item = load_fixture(f"zerion_item_{label}.json")
        helius_response = load_fixture(f"solana_rpc_{label}.json")

        response = MagicMock()
        response.status_code = 200
        response.raise_for_status = MagicMock()
        response.json.return_value = {"data": [zerion_item], "links": {}}
        mock_get.return_value = response

        mock_solana_rpc = MagicMock()
        mock_solana_rpc._rpc_call.return_value = helius_response

        client = ZerionClient(api_key="zk_dev_test", solana_rpc_client=mock_solana_rpc)
        wallet = WalletConfig(
            blockchain="solana",
            friendly_name="test",
            address=wallet_address,
            provider="zerion",
        )
        records, _ = client.fetch_new_transactions(wallet)

        # No zero-value records — `mintTo amount=0` sentinel instructions
        # must be skipped by the synthesizer.
        zero_value = [(r.record_id, r.value, r.token_symbol) for r in records if r.value == "0"]
        assert not zero_value, (
            f"\n[{label}] zero-value records emitted (should be skipped):\n  "
            + "\n  ".join(repr(z) for z in zero_value)
        )

        # record_ids unique within a tx — colliding ids signal multiple
        # mintTo at the same outer_index+mint emitted with the same key.
        rids = [r.record_id for r in records]
        from collections import Counter
        dupes = [rid for rid, cnt in Counter(rids).items() if cnt > 1]
        assert not dupes, f"\n[{label}] colliding record_ids:\n  " + "\n  ".join(dupes)

        # No synthetic mint-receive may duplicate an existing (mint, value)
        # base record. The full record set must match the expected set.
        actual = {(r.token_contract or "", r.value): r.method for r in records}
        assert actual == expected, (
            f"\n[{label}] records mismatch\n"
            f"  expected: {sorted(expected.items())}\n"
            f"  actual:   {sorted(actual.items())}"
        )

    # The synthesized rpc-derived `:rpc:N:receive:WSOL_MINT` row carries the
    # gross pre/post token-balance delta from the parsed Solana RPC response,
    # while Zerion's `:0` base SOL row carries Zerion's net amount. The two
    # differ by the swap program's transient ATA rent (~0.00275 SOL on the
    # fixtures here), so the existing exact-value dedup never catches them
    # and the wallet ends up with the same SOL credited twice. The fix must
    # treat any rpc-derived WSOL_MINT receive as a duplicate of an inbound
    # base SOL record on the same tx, regardless of value drift.
    @pytest.mark.parametrize("wallet_address,label,expected", [
        # Saber LP withdraw: only an inbound SOL receive. Base = 446.071641
        # SOL; rpc enrichment adds a 446.074390 WSOL receive (+2,749,200
        # lamport drift) and a 0-value WSOL burn-companion. Both rpc rows
        # are duplicates of the base.
        (
            "3FXQVRb1kEXeP7pmsrdUnfEXVdszj9dVZyfY3shx4gnt",
            "legacy_saber_lp_withdraw_2021_sep",
            {("", "446071641276"): "receive"},
        ),
        # Saber mSOL→SOL swap: wallet sends 78 mSOL, receives 78.134520
        # SOL (base). Rpc enrichment emits the legitimate `:rpc:2:send:mSOL`
        # leg AND a `:rpc:2:receive:WSOL` row with 78.137269923 lamports
        # (+2,749,200 drift) that duplicates the base SOL receive. Only
        # the WSOL receive should be skipped; the mSOL send must remain.
        # Note: the base SOL row keeps method="receive" (Zerion's word) —
        # downstream `_rules.json` is responsible for classifying it as a
        # trade leg vs a plain transfer based on sibling-posting context.
        (
            "3FXQVRb1kEXeP7pmsrdUnfEXVdszj9dVZyfY3shx4gnt",
            "legacy_saber_swap_2021_sep",
            {
                ("", "78134520723"): "receive",
                ("mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So", "78000000000"): "trade",
            },
        ),
    ])
    @patch("sync_crypto.zerion.requests.get")
    def test_rpc_wsol_receive_dedupes_against_base_sol_with_value_drift(
        self, mock_get, wallet_address, label, expected,
    ):
        from sync_crypto.zerion import ZerionClient

        zerion_item = load_fixture(f"zerion_item_{label}.json")
        helius_response = load_fixture(f"solana_rpc_{label}.json")

        response = MagicMock()
        response.status_code = 200
        response.raise_for_status = MagicMock()
        response.json.return_value = {"data": [zerion_item], "links": {}}
        mock_get.return_value = response

        mock_solana_rpc = MagicMock()
        mock_solana_rpc._rpc_call.return_value = helius_response

        client = ZerionClient(api_key="zk_dev_test", solana_rpc_client=mock_solana_rpc)
        wallet = WalletConfig(
            blockchain="solana",
            friendly_name="3FXQ",
            address=wallet_address,
            provider="zerion",
        )
        records, _ = client.fetch_new_transactions(wallet)

        actual = {(r.token_contract or "", r.value): r.method for r in records}
        assert actual == expected, (
            f"\n[{label}] records mismatch\n"
            f"  expected: {sorted(expected.items())}\n"
            f"  actual:   {sorted(actual.items())}"
        )

    # closeAccount on a wallet-owned WSOL ATA returns the ATA's lamports
    # (rent + any wSOL balance) to the wallet. The underlying SOL flow was
    # already credited at the swap that funded the ATA — a separate prior
    # tx — so emitting this Zerion base SOL receive double-credits the
    # wallet. Detected via the parsed RPC's closeAccount instruction:
    # owner==wallet, destination==wallet, and the closed account held
    # wSOL pre-tx.
    @patch("sync_crypto.zerion.requests.get")
    def test_self_owned_wsol_ata_close_drops_phantom_sol_receive(self, mock_get):
        from sync_crypto.zerion import ZerionClient

        wallet_address = "3FXQVRb1kEXeP7pmsrdUnfEXVdszj9dVZyfY3shx4gnt"
        zerion_item = load_fixture("zerion_item_legacy_wsol_ata_close_2021_oct.json")
        helius_response = load_fixture("solana_rpc_legacy_wsol_ata_close_2021_oct.json")

        response = MagicMock()
        response.status_code = 200
        response.raise_for_status = MagicMock()
        response.json.return_value = {"data": [zerion_item], "links": {}}
        mock_get.return_value = response

        mock_solana_rpc = MagicMock()
        mock_solana_rpc._rpc_call.return_value = helius_response

        client = ZerionClient(api_key="zk_dev_test", solana_rpc_client=mock_solana_rpc)
        wallet = WalletConfig(
            blockchain="solana",
            friendly_name="3FXQ",
            address=wallet_address,
            provider="zerion",
        )
        records, _ = client.fetch_new_transactions(wallet)

        # The Zerion item is a single SOL receive whose source is the
        # wallet's own WSOL ATA. Post-fix, no record should be emitted —
        # the SOL was already booked at swap time on a separate tx.
        assert records == [], (
            "expected zero records for wallet-owned wSOL ATA close, got: "
            + ", ".join(f"{r.token_symbol}/{r.value}/{r.method}" for r in records)
        )

    # A *wrap* books the wallet's own native SOL into a wSOL ATA: Zerion's
    # base SOL row is a `send` (native out), but the rpc enrichment sees the
    # wallet's wSOL ATA fill and synthesizes a same-value WSOL `receive` on
    # the same tx. That cross-direction mirror is NOT a real external inflow
    # and must be deduped — otherwise the wrapped SOL is credited twice. This
    # is the root cause of 3FXQ's Port Finance pSOL deposits (2Dbi9Y +200.53,
    # 3G9WEB +185.07) leaving +385 phantom SOL. The pre-existing dedup only
    # registers same-direction WSOL fingerprints, so it misses the wrap.
    def test_base_sol_send_dedupes_cross_direction_wsol_wrap_receive(self):
        from sync_crypto.zerion import ZerionClient

        wsol = ZerionClient.WRAPPED_SOL_MINT
        wallet = "3FXQVRb1kEXeP7pmsrdUnfEXVdszj9dVZyfY3shx4gnt"

        # Wrap: base native-SOL send must dedupe a same-value WSOL receive.
        send = TransactionRecord(
            tx_hash="2Dbi9Y",
            blockchain="solana",
            timestamp=1629249935,
            from_address=wallet,
            to_address="Port7uDYB3wk6GJAw4KT1WpTeMtSu9bTcChBHkX2LfR",
            value="200529973277",
            fee="20000",
            status="confirmed",
            tx_type="token_transfer",
            token_symbol="SOL",
            currency="SOL",
            token_contract=None,
            method="send",
        )
        fps = ZerionClient._existing_record_fingerprints(wallet, [send])
        assert (wsol, "200529973277", "receive") in fps

        # Unwrap symmetry: base native-SOL receive dedupes a WSOL send.
        recv = TransactionRecord(
            tx_hash="unwrapTx",
            blockchain="solana",
            timestamp=1,
            from_address="SomeUnwrapSource1111111111111111111111111111",
            to_address=wallet,
            value="446071641276",
            fee="5000",
            status="confirmed",
            tx_type="token_transfer",
            token_symbol="SOL",
            currency="SOL",
            token_contract=None,
            method="receive",
        )
        fps2 = ZerionClient._existing_record_fingerprints(wallet, [recv])
        assert (wsol, "446071641276", "send") in fps2
