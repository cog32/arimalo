"""Tests for sol-labels plugin.

Tests commodity sanitization, Solscan label fetching, and lookup integration.
"""

import json
import os
import re
import pytest
from unittest.mock import patch, MagicMock
import label as label_mod
from label import (
    sanitize_commodity,
    make_commodity_rule,
    extract_covered_addresses,
    merge_rules,
    fetch_solscan_labels,
    lookup_addresses,
    save_json_file,
)

# Must match src-tauri/src/ledger_parser.rs commodity_re()
COMMODITY_RE = re.compile(r"^[A-Za-z0-9_][A-Za-z0-9_.\-]*$")


class TestSanitizeCommodity:
    """sanitize_commodity produces valid ledger commodities."""

    @pytest.mark.parametrize(
        "raw, expected",
        [
            # LP pairs with slash become BASE_QUOTE_LP
            ("PORT/USDC", "PORT_USDC_LP"),
            ("LIQ/USDC", "LIQ_USDC_LP"),
            # LP pairs with hyphen become BASE_QUOTE_LP
            ("RAY-SOL", "RAY_SOL_LP"),
            ("RAY-USDC", "RAY_USDC_LP"),
            ("RAY-SRM", "RAY_SRM_LP"),
            ("SNY-USDC", "SNY_USDC_LP"),
            ("mSOL-SOL", "mSOL_SOL_LP"),
            # Plain symbols pass through unchanged
            ("USDC", "USDC"),
            ("SOL", "SOL"),
            ("WSOL", "WSOL"),
            ("JUP", "JUP"),
            ("stSOL", "stSOL"),
            # Hex-style contract addresses pass through
            ("0xecf8f87f", "0xecf8f87f"),
            # Multi-word names get underscores
            ("Token Program account", "Token_Program_account"),
            ("Wrapped SOL", "Wrapped_SOL"),
            # Leading special chars stripped
            ("$CWIF", "CWIF"),
        ],
    )
    def test_known_symbols(self, raw, expected):
        assert sanitize_commodity(raw) == expected

    def test_empty_returns_unknown(self):
        assert sanitize_commodity("") == "UNKNOWN"

    @pytest.mark.parametrize(
        "raw",
        [
            "PORT/USDC",
            "LIQ/USDC",
            "RAY-SOL",
            "RAY-USDC",
            "RAY-SRM",
            "SNY-USDC",
            "mSOL-SOL",
            "USDC",
            "SOL",
            "BONK",
            "stSOL",
            "0xecf8f87f",
            "WSOL",
            "JUP",
            "Token Program account",
            "Wrapped SOL",
            "$CWIF",
        ],
    )
    def test_all_outputs_match_commodity_regex(self, raw):
        """Every sanitized commodity must be parseable by the ledger parser."""
        result = sanitize_commodity(raw)
        assert COMMODITY_RE.match(result), (
            f"sanitize_commodity({raw!r}) = {result!r} "
            f"does not match commodity regex"
        )


class TestMakeCommodityRule:
    """make_commodity_rule produces rules with valid commodities."""

    def test_rule_commodity_is_sanitized(self):
        rule = make_commodity_rule(
            "4CGxvZdwiZgVMLXiTdJHTkJRUTpTSJCtmtCRbSkAxerE", "PORT/USDC"
        )
        assert rule["commodity"] == "PORT_USDC_LP"
        assert COMMODITY_RE.match(rule["commodity"])

    def test_rule_structure(self):
        rule = make_commodity_rule(
            "SoLEao8wTzSfqhuou8rcYsVoLjthVmiXuEjzdNPMnCz", "mSOL-SOL"
        )
        assert rule["id"] == "auto-sol-token-SoLEao8wTz"
        assert rule["pattern"] == "*SoLEao8wTzSfqhuou8rcYsVoLjthVmiXuEjzdNPMnCz*"
        assert rule["match_field"] == "commodity"
        assert rule["commodity"] == "mSOL_SOL_LP"
        assert rule["comment"] == "auto:sol-labels"


class TestFetchSolscanLabels:
    """fetch_solscan_labels fetches and caches Solscan program/address labels."""

    def test_parses_program_and_address_sections(self, tmp_path):
        mock_response = json.dumps({
            "program": {
                "9DrvZvyWh1HuAoZxvYWMvkf2XCzryCpGgHqrMjyDWpmo": "Kamino Lending",
                "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4": "Jupiter v6",
            },
            "address": {
                "AC5RDfQFmDS1deWZos921JfqscXdByf8BKHs5ACWjtW2": "Bybit",
            },
            "hacker": {
                "HackAddr111111111111111111111111111111111111": "SomeHacker",
            },
        }).encode()

        mock_resp = MagicMock()
        mock_resp.read.return_value = mock_response
        mock_resp.__enter__ = lambda s: s
        mock_resp.__exit__ = MagicMock(return_value=False)

        with patch("label.urlopen", return_value=mock_resp):
            result = fetch_solscan_labels(str(tmp_path))

        assert result["9DrvZvyWh1HuAoZxvYWMvkf2XCzryCpGgHqrMjyDWpmo"] == "Kamino Lending"
        assert result["JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"] == "Jupiter v6"
        assert result["AC5RDfQFmDS1deWZos921JfqscXdByf8BKHs5ACWjtW2"] == "Bybit"
        # hacker section should not be included
        assert "HackAddr111111111111111111111111111111111111" not in result

    def test_uses_cache_when_fresh(self, tmp_path):
        cached = {"Addr1111": "Cached Protocol"}
        save_json_file(os.path.join(tmp_path, "solscan_labels.json"), cached)

        with patch("label.urlopen") as mock_urlopen:
            result = fetch_solscan_labels(str(tmp_path))

        mock_urlopen.assert_not_called()
        assert result == cached

    def test_returns_empty_on_network_error(self, tmp_path):
        from urllib.error import URLError
        with patch("label.urlopen", side_effect=URLError("timeout")):
            result = fetch_solscan_labels(str(tmp_path))
        assert result == {}


class TestExtractCoveredAddresses:
    """extract_covered_addresses skips generic fallback commodity rules."""

    ADDR_A = "4CGxvZdwiZgVMLXiTdJHTkJRUTpTSJCtmtCRbSkAxerE"
    ADDR_B = "SoLEao8wTzSfqhuou8rcYsVoLjthVmiXuEjzdNPMnCz"

    def test_normal_commodity_rule_is_covered(self):
        rules = [{"id": "auto-sol-token-4CGxvZdwiZ", "pattern": f"*{self.ADDR_A}*",
                  "match_field": "commodity", "commodity": "USDC", "comment": "auto:sol-labels"}]
        _, covered = extract_covered_addresses(rules)
        assert self.ADDR_A in covered

    def test_generic_account_suffix_not_covered(self):
        """Commodity rules with _account suffix are stale and should be re-resolved."""
        rules = [{"id": "auto-sol-token-4CGxvZdwiZ", "pattern": f"*{self.ADDR_A}*",
                  "match_field": "commodity", "commodity": "Token_Program_account", "comment": "auto:sol-labels"}]
        _, covered = extract_covered_addresses(rules)
        assert self.ADDR_A not in covered

    def test_generic_mint_suffix_not_covered(self):
        """Commodity rules with _mint suffix (address-based) should be re-resolved."""
        rules = [{"id": "auto-sol-token-SoLEao8wTz", "pattern": f"*{self.ADDR_B}*",
                  "match_field": "commodity", "commodity": f"{self.ADDR_B}_token_mint", "comment": "auto:sol-labels"}]
        _, covered = extract_covered_addresses(rules)
        assert self.ADDR_B not in covered

    def test_payee_rules_unaffected(self):
        rules = [{"id": "auto-sol-4CGxvZdwiZ", "pattern": f"*{self.ADDR_A}*",
                  "payee": "Token Program account", "comment": "auto:sol-labels"}]
        payee_covered, _ = extract_covered_addresses(rules)
        assert self.ADDR_A in payee_covered


class TestMergeRules:
    """merge_rules updates existing auto-generated rules in place."""

    def test_updates_existing_auto_rule(self, tmp_path):
        rules_path = os.path.join(tmp_path, "_rules.json")
        save_json_file(rules_path, {"rules": [
            {"id": "auto-sol-token-AAAA", "pattern": "*AAAA*", "match_field": "commodity",
             "commodity": "Token_Program_account", "comment": "auto:sol-labels"},
        ]})
        new = [{"id": "auto-sol-token-AAAA", "pattern": "*AAAA*", "match_field": "commodity",
                "commodity": "AAAA_token_mint", "comment": "auto:sol-labels"}]
        merge_rules(rules_path, new)
        data = json.loads(open(rules_path).read())
        assert data["rules"][0]["commodity"] == "AAAA_token_mint"

    def test_does_not_update_manual_rule(self, tmp_path):
        rules_path = os.path.join(tmp_path, "_rules.json")
        save_json_file(rules_path, {"rules": [
            {"id": "auto-sol-token-BBBB", "pattern": "*BBBB*", "match_field": "commodity",
             "commodity": "MyToken", "comment": "manually edited"},
        ]})
        new = [{"id": "auto-sol-token-BBBB", "pattern": "*BBBB*", "match_field": "commodity",
                "commodity": "BBBB_token_mint", "comment": "auto:sol-labels"}]
        merge_rules(rules_path, new)
        data = json.loads(open(rules_path).read())
        assert data["rules"][0]["commodity"] == "MyToken"

    def test_inserts_truly_new_rules(self, tmp_path):
        rules_path = os.path.join(tmp_path, "_rules.json")
        save_json_file(rules_path, {"rules": []})
        new = [{"id": "auto-sol-token-CCCC", "pattern": "*CCCC*", "match_field": "commodity",
                "commodity": "SOL", "comment": "auto:sol-labels"}]
        merge_rules(rules_path, new)
        data = json.loads(open(rules_path).read())
        assert len(data["rules"]) == 1
        assert data["rules"][0]["commodity"] == "SOL"


class TestKnownAddresses:
    """KNOWN_ADDRESSES (curated) is consulted during lookup and refreshes empty cache."""

    ADDR = "2MxtQq5TE4aSgGHceyyTkPapdNzyVftPZ7dBwgb3Lraz"

    def test_resolves_from_known_addresses(self, tmp_path, monkeypatch):
        monkeypatch.setattr(label_mod, "KNOWN_ADDRESSES", {self.ADDR: "Marginfi V2"})
        labels, _ = lookup_addresses(
            {self.ADDR}, str(tmp_path), "http://fake", {}, {}
        )
        assert labels[self.ADDR] == "Marginfi V2"

    def test_empty_cache_entry_refreshed(self, tmp_path, monkeypatch):
        monkeypatch.setattr(label_mod, "KNOWN_ADDRESSES", {self.ADDR: "Marginfi V2"})
        save_json_file(os.path.join(tmp_path, "label_cache.json"), {self.ADDR: ""})
        labels, _ = lookup_addresses(
            {self.ADDR}, str(tmp_path), "http://fake", {}, {}
        )
        assert labels[self.ADDR] == "Marginfi V2"


class TestLookupWithSolscanLabels:
    """lookup_addresses resolves addresses via Solscan labels."""

    KAMINO_ADDR = "9DrvZvyWh1HuAoZxvYWMvkf2XCzryCpGgHqrMjyDWpmo"

    def test_solscan_labels_resolve_unknown_address(self, tmp_path):
        """An address not in hardcoded lists or token lists is found via Solscan."""
        solscan = {self.KAMINO_ADDR: "Kamino Lending"}
        with patch("label.rpc_get_multiple_accounts", return_value=[]):
            labels, warnings = lookup_addresses(
                {self.KAMINO_ADDR}, str(tmp_path), "http://fake", {}, solscan
            )
        assert labels[self.KAMINO_ADDR] == "Kamino Lending"

    def test_stale_cache_updated_by_solscan(self, tmp_path):
        """A previously empty cache entry gets resolved when Solscan labels are available."""
        # Seed cache with empty entry (previously unknown)
        cache = {self.KAMINO_ADDR: ""}
        save_json_file(os.path.join(tmp_path, "label_cache.json"), cache)

        solscan = {self.KAMINO_ADDR: "Kamino Lending"}
        labels, warnings = lookup_addresses(
            {self.KAMINO_ADDR}, str(tmp_path), "http://fake", {}, solscan
        )
        assert labels[self.KAMINO_ADDR] == "Kamino Lending"

    def test_hardcoded_takes_priority_over_solscan(self, tmp_path):
        """KNOWN_EXCHANGES/KNOWN_PROGRAMS should take priority over Solscan labels."""
        # Use a known exchange address
        binance_addr = "28nYGHJyUVcVdxZtzKByBXEj127XnrUkrE3VaGuWj1ZU"
        solscan = {binance_addr: "Solscan Binance Label"}
        labels, warnings = lookup_addresses(
            {binance_addr}, str(tmp_path), "http://fake", {}, solscan
        )
        assert labels[binance_addr] == "Binance"
