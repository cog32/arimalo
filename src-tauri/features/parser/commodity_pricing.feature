Feature: Parse commodity pricing annotations

  Scenario: Per-unit price annotation is parsed
    Given a transactions file named "price_per_unit.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    And posting 1 of transaction 1 should have a per-unit price of "32000.00" "USD"

  Scenario: Total price annotation is parsed
    Given a transactions file named "price_total.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    And posting 1 of transaction 1 should have a total price of "160.00" "USD"

  Scenario: Cost and price annotations together
    Given a transactions file named "cost_and_price.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    And posting 1 of transaction 1 should have a per-unit cost of "23.00" "USD"
    And posting 1 of transaction 1 should have cost fields including "venue:binance"
    And posting 1 of transaction 1 should have a total price of "230.00" "USD"

  Scenario: Total cost annotation is parsed
    Given a transactions file named "cost_total.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    And posting 1 of transaction 1 should have a total cost of "230.00" "USD"
    And posting 1 of transaction 1 should have cost fields including "fee:0.10 USD"
    And posting 1 of transaction 1 should have cost fields including "venue:binance"

  Scenario: Posting without annotations has no cost or price
    Given a transactions file named "price_per_unit.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    And posting 2 of transaction 1 should have no price annotation
    And posting 2 of transaction 1 should have no cost annotation

  Scenario: Remainder field preserved alongside parsed annotations
    Given a transactions file named "price_per_unit.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    And posting 1 of transaction 1 should have remainder "@ 32000.00 USD"

  Scenario: Daily prices file parses successfully
    Given a prices file named "daily.prices"
    When I run the prices parser on that file
    Then the prices parse should succeed
    And there should be 3 price directives
    And price directive 1 should be "BTC" at "30000.00" "USD" on "2026-01-15"
    And price directive 3 should be "BTC" at "31000.00" "USD" on "2026-01-17"

  Scenario: Intraday prices with microsecond precision
    Given a prices file named "intraday.prices"
    When I run the prices parser on that file
    Then the prices parse should succeed
    And there should be 2 price directives
    And price directive 1 should be "BTC" at "30150.00" "USD" on "2026-01-15T10:05:03.123456Z"

  Scenario: Prices file with comments and blank lines
    Given a prices file named "with_comments.prices"
    When I run the prices parser on that file
    Then the prices parse should succeed
    And there should be 2 price directives

  Scenario: Invalid prices file returns diagnostics
    Given a prices file named "invalid.prices"
    When I run the prices parser on that file
    Then the prices parse should fail
    And there should be 1 price directives
