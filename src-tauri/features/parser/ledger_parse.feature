Feature: Parse .transactions files

  Scenario: Valid file parses successfully
    Given a transactions file named "valid.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    And the first transaction payee should be "Binance"
    And the first transaction narration should be "Buy SOL"
    And the first transaction meta should include "txn:"

  Scenario: Invalid file returns diagnostics
    Given a transactions file named "invalid.transactions"
    When I run the ledger parser on that file
    Then the parse should fail
    And diagnostics should include "missing amount"

  Scenario: Example file parses header fields correctly
    Given a transactions file named "example.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    And the first transaction payee should be "Kraken"
    And the first transaction narration should be "Sell BTC"
    And the first transaction meta should include "src:kraken:trade:def456"

  Scenario: Account declarations set opening balances
    Given a transactions file named "account_opening.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    And the balance for account "assets:cash:bank:cba:smartaccess" should be "100.0" "AUD"

  Scenario: Hyphenated commodity names parse successfully
    Given a transactions file named "hyphenated_commodity.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
