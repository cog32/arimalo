Feature: Account management (rename and delete)

  Users can rename or delete account folders through the UI.
  Renaming moves all files to a new folder path, changing the derived
  account name. Deleting removes the folder and all its contents.

  Scenario: Rename account folder changes the derived account name
    Given a clean sources directory with a CSV "richard/crypto/exchange/binance/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Buy ATOM    | -8.65  |
    And a transform at "richard/crypto/exchange/binance/_transform.rhai" that maps Date/Description/Amount to AUD
    When I run the pipeline for month "202501"
    Then the transaction should use account "assets:crypto:exchange:binance"
    When I rename account folder "richard/crypto/exchange/binance" to "richard/crypto/exchange/binance/personal"
    Then the transaction should use account "assets:crypto:exchange:binance:personal"
    And the folder "richard/crypto/exchange/binance/personal" should exist under sources

  Scenario: Rename preserves all account files
    Given a clean sources directory with a CSV "richard/crypto/exchange/binance/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Buy ATOM    | -8.65  |
    And a transform at "richard/crypto/exchange/binance/_transform.rhai" that maps Date/Description/Amount to AUD
    And a rules file at "richard/crypto/exchange/binance/_rules.json" matching "ATOM" with payee "Atom Trade"
    When I rename account folder "richard/crypto/exchange/binance" to "richard/crypto/exchange/binance/spot"
    Then a "_transform.rhai" file should exist in "richard/crypto/exchange/binance/spot"
    And a "_rules.json" file should exist in "richard/crypto/exchange/binance/spot"
    And a "2025-01.csv" file should exist in "richard/crypto/exchange/binance/spot"

  Scenario: Rename to existing folder is rejected
    Given a clean sources directory with a CSV "richard/crypto/exchange/binance/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Buy ATOM    | -8.65  |
    And a transform at "richard/crypto/exchange/binance/_transform.rhai" that maps Date/Description/Amount to AUD
    And a CSV "richard/crypto/exchange/kraken/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Buy ETH     | -10.00 |
    And a transform at "richard/crypto/exchange/kraken/_transform.rhai" that maps Date/Description/Amount to AUD
    When I try to rename account folder "richard/crypto/exchange/binance" to "richard/crypto/exchange/kraken"
    Then the operation should fail with "already exists"

  Scenario: Delete account folder removes it and rebuilds
    Given a clean sources directory with a CSV "richard/crypto/exchange/binance/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Buy ATOM    | -8.65  |
    And a transform at "richard/crypto/exchange/binance/_transform.rhai" that maps Date/Description/Amount to AUD
    And a CSV "richard/crypto/exchange/kraken/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Buy ETH     | -10.00 |
    And a transform at "richard/crypto/exchange/kraken/_transform.rhai" that maps Date/Description/Amount to AUD
    When I run the pipeline for month "202501"
    Then the active ledger should include payee "Buy ATOM"
    And the active ledger should include payee "Buy ETH"
    When I delete account folder "richard/crypto/exchange/binance"
    Then the active ledger should not include payee "Buy ATOM"
    And the active ledger should include payee "Buy ETH"
    And the folder "richard/crypto/exchange/binance" should not exist under sources

  Scenario: Delete non-existent folder is rejected
    Given a clean sources directory
    When I try to delete account folder "richard/nonexistent"
    Then the operation should fail with "does not exist"
