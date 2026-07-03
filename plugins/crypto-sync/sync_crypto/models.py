"""Data models for crypto transaction syncing."""
from enum import Enum
from typing import Optional, TypeAlias

from pydantic import BaseModel, Field, field_validator


class Blockchain(str, Enum):
    ETHEREUM = "ethereum"
    SOLANA = "solana"
    COSMOS = "cosmos"


class WalletHistoryProvider(str, Enum):
    ZERION = "zerion"


class WalletConfig(BaseModel):
    blockchain: Blockchain
    friendly_name: str
    address: str
    provider: Optional[WalletHistoryProvider] = None
    network: Optional[str] = None

    @field_validator("friendly_name")
    @classmethod
    def friendly_name_not_empty(cls, v: str) -> str:
        if not v.strip():
            raise ValueError("friendly_name must not be empty")
        return v

    @field_validator("address")
    @classmethod
    def normalize_address(cls, v: str) -> str:
        if not v.strip():
            raise ValueError("address must not be empty")
        v = v.strip()
        # EVM addresses (0x...) are case-insensitive — normalise to lowercase so a
        # wallet always maps to ONE folder/account regardless of the case written in
        # wallets.json. Solana (base58) and Cosmos (bech32) are case-sensitive: leave
        # them untouched.
        if v[:2].lower() == "0x":
            return v.lower()
        return v

    @field_validator("network")
    @classmethod
    def normalize_network(cls, v: Optional[str]) -> Optional[str]:
        if v is None:
            return None
        normalized = v.strip().lower()
        return normalized or None


DEFAULT_BINANCE_TRANSFER_TYPES = [
    "MAIN_FUNDING",
    "FUNDING_MAIN",
]


class BinanceConfig(BaseModel):
    exchange: str = "binance"
    friendly_name: str
    symbols: list[str]
    transfer_types: list[str] = Field(
        default_factory=lambda: list(DEFAULT_BINANCE_TRANSFER_TYPES)
    )

    @field_validator("exchange")
    @classmethod
    def exchange_must_be_binance(cls, v: str) -> str:
        if str(v).strip().lower() != "binance":
            raise ValueError("exchange must be 'binance'")
        return "binance"

    @field_validator("friendly_name")
    @classmethod
    def binance_friendly_name_not_empty(cls, v: str) -> str:
        if not v.strip():
            raise ValueError("friendly_name must not be empty")
        return v

    @field_validator("symbols")
    @classmethod
    def normalize_symbols(cls, v: list[str]) -> list[str]:
        normalized = []
        seen = set()
        for raw in v:
            symbol = str(raw).strip().upper()
            if not symbol or symbol in seen:
                continue
            seen.add(symbol)
            normalized.append(symbol)
        if not normalized:
            raise ValueError("symbols must contain at least one Binance symbol")
        return normalized

    @field_validator("transfer_types")
    @classmethod
    def normalize_transfer_types(cls, v: list[str]) -> list[str]:
        normalized = []
        seen = set()
        for raw in v:
            transfer_type = str(raw).strip().upper()
            if not transfer_type or transfer_type in seen:
                continue
            seen.add(transfer_type)
            normalized.append(transfer_type)
        if not normalized:
            raise ValueError("transfer_types must contain at least one transfer type")
        return normalized


class TransactionRecord(BaseModel):
    record_id: Optional[str] = None
    provider: Optional[str] = None
    network: Optional[str] = None
    tx_hash: str
    blockchain: str
    timestamp: int
    from_address: str
    to_address: str
    value: str
    fee: str
    status: str
    tx_type: Optional[str] = None
    token_name: Optional[str] = None
    token_symbol: Optional[str] = None
    token_contract: Optional[str] = None
    token_decimals: Optional[int] = None
    block_number: Optional[int] = None
    gas_used: Optional[str] = None
    gas_price: Optional[str] = None
    method: Optional[str] = None
    currency: Optional[str] = None
    method_id: Optional[str] = None
    function_name: Optional[str] = None
    input_data: Optional[str] = None
    tx_receipt_status: Optional[str] = None
    transaction_index: Optional[int] = None
    cumulative_gas_used: Optional[str] = None
    confirmations: Optional[int] = None

    def to_csv_row(self) -> dict:
        """Return a dict suitable for CSV writing."""
        return self.model_dump()


class BinanceAccountingEvent(BaseModel):
    event_id: str
    exchange: str
    account_name: str
    timestamp: int
    event_type: str
    status: str
    asset: Optional[str] = None
    asset_amount: Optional[str] = None
    counter_asset: Optional[str] = None
    counter_amount: Optional[str] = None
    fee_asset: Optional[str] = None
    fee_amount: Optional[str] = None
    symbol: Optional[str] = None
    side: Optional[str] = None
    source_account: Optional[str] = None
    destination_account: Optional[str] = None
    network: Optional[str] = None
    address: Optional[str] = None
    address_tag: Optional[str] = None
    tx_id: Optional[str] = None
    order_id: Optional[str] = None
    trade_id: Optional[str] = None
    quote_id: Optional[str] = None
    transfer_id: Optional[str] = None
    reference: Optional[str] = None
    metadata_json: Optional[str] = None

    def to_csv_row(self) -> dict:
        """Return a dict suitable for CSV writing."""
        return self.model_dump()


SyncSource: TypeAlias = WalletConfig | BinanceConfig
