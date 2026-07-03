Feature: Auto-detect self-transfers from declared accounts

  When a transaction's narration or meta contains an identifier that matches
  a declared account name (from accounts.transactions), the pipeline should
  automatically categorise it as a self-transfer without needing a manual rule.

  Scenario: Transaction mentioning a declared account is auto-categorised as self-transfer
    Given a clean sources directory with a CSV "richard/crypto/wallet/ethereum/2025-01.csv":
      | Date       | Description                                          | Amount |
      | 2025-01-15 | Transfer to 0x6d25d07f5c0dccd0d6c7b3342cd83b902464f06b | -1.00  |
    And a transform at "richard/crypto/wallet/ethereum/_transform.rhai" for account "assets:crypto:wallet:ethereum"
    And a per-folder accounts file in "richard/crypto/wallet/ethereum" declaring "assets:crypto:wallet:ethereum:0x6d25d07f5c0dccd0d6c7b3342cd83b902464f06b ETH"
    When I run the pipeline
    Then transactions with narration "imported" should use contra "assets:transfer"
    And transactions with narration "imported" should have payee "Self Transfer"

  Scenario: Explicit rules take priority over auto self-transfer
    Given a clean sources directory with a CSV "richard/crypto/wallet/ethereum/2025-01.csv":
      | Date       | Description                                          | Amount |
      | 2025-01-15 | Transfer to 0x6d25d07f5c0dccd0d6c7b3342cd83b902464f06b | -1.00  |
    And a transform at "richard/crypto/wallet/ethereum/_transform.rhai" for account "assets:crypto:wallet:ethereum"
    And a per-folder accounts file in "richard/crypto/wallet/ethereum" declaring "assets:crypto:wallet:ethereum:0x6d25d07f5c0dccd0d6c7b3342cd83b902464f06b ETH"
    And a rules file at "richard/crypto/wallet/ethereum/_rules.json" matching "*0x6d25d07f*" with contra "assets:special:wallet"
    When I run the pipeline
    Then transactions with narration "imported" should use contra "assets:special:wallet"

  Scenario: No auto-match when address is not a declared account
    Given a clean sources directory with a CSV "richard/crypto/wallet/ethereum/2025-01.csv":
      | Date       | Description                              | Amount |
      | 2025-01-15 | Transfer to 0xdeadbeefdeadbeefdeadbeef   | -1.00  |
    And a transform at "richard/crypto/wallet/ethereum/_transform.rhai" for account "assets:crypto:wallet:ethereum"
    And a per-folder accounts file in "richard/crypto/wallet/ethereum" declaring "assets:crypto:wallet:ethereum:0x6d25d07f5c0dccd0d6c7b3342cd83b902464f06b ETH"
    When I run the pipeline
    Then transactions with narration "imported" should use contra "expenses:unknown"
