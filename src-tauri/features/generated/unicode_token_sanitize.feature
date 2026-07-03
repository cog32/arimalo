Feature: Token symbol sanitization in transform narrations

  The sanitize_commodity function in Rhai transforms uses Unicode-aware
  character detection. It keeps Unicode letters and ASCII digits, and falls
  back to "SPAM" when the result is empty or exceeds 24 characters.
  Narrations must use the sanitized commodity, not the raw token symbol.

  Scenario: Narration uses sanitized commodity for overlong token symbol
    Given a clean sources directory with a CSV "wallet/tokens.csv":
      | Date       | from_address | value | token_symbol                      | tx_type        | method | status  |
      | 2025-11-07 | 0xabc123     | 5.00  | ABCDEFGHIJKLMNOPQRSTUVWXY_toolong | token_transfer | send   | success |
    And a transform at "wallet/_transform.rhai" with token sanitization
    When I run the pipeline
    Then the active ledger should include narration "token_transfer:send SPAM"
    And transactions should have commodity "SPAM"

  Scenario: Narration preserves clean ASCII token symbol
    Given a clean sources directory with a CSV "wallet/tokens.csv":
      | Date       | from_address | value | token_symbol | tx_type        | method | status  |
      | 2025-11-07 | 0xabc123     | 5.00  | SWELL        | token_transfer | send   | success |
    And a transform at "wallet/_transform.rhai" with token sanitization
    When I run the pipeline
    Then the active ledger should include narration "token_transfer:send SWELL"
    And transactions should have commodity "SWELL"

  Scenario: Narration omits commodity for non-token transactions
    Given a clean sources directory with a CSV "wallet/tokens.csv":
      | Date       | from_address | value | token_symbol | tx_type     | method   | status  |
      | 2025-11-07 | 0xabc123     | 5.00  |              | transaction | transfer | success |
    And a transform at "wallet/_transform.rhai" with token sanitization
    When I run the pipeline
    Then the active ledger should include narration "transaction:transfer"
    And transactions should have commodity "ETH"
