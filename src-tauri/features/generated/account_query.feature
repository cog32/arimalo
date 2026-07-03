Feature: Account prefix query

  Transactions can be filtered by account prefix, returning all transactions
  where any posting matches the prefix exactly or as a parent in the hierarchy.

  Background:
    Given a clean sources directory
    And a CSV "richard/crypto/exchange/binance/personal/2025-01.csv":
      | Date       | Description    | Amount  |
      | 2025-01-10 | Buy BTC        | -500.00 |
      | 2025-01-15 | Deposit        | 1000.00 |
    And a transform at "richard/crypto/exchange/binance/personal/_transform.rhai" without an account field
    And a CSV "richard/crypto/exchange/kraken/personal/2025-02.csv":
      | Date       | Description    | Amount  |
      | 2025-02-01 | Buy ETH        | -200.00 |
    And a transform at "richard/crypto/exchange/kraken/personal/_transform.rhai" without an account field
    And a CSV "richard/cash/bank/cba/savings/2025-01.csv":
      | Date       | Description    | Amount  |
      | 2025-01-20 | Interest       | 5.00    |
    And a transform at "richard/cash/bank/cba/savings/_transform.rhai" without an account field
    When I run the pipeline for month "202502"

  Scenario: Query with prefix returns matching transactions
    Then filtering by prefix "assets:crypto" should return 3 transactions

  Scenario: Query with deeper prefix narrows results
    Then filtering by prefix "assets:crypto:exchange:binance" should return 2 transactions

  Scenario: Query with leaf account returns exact match
    Then filtering by prefix "assets:crypto:exchange:kraken:personal" should return 1 transactions

  Scenario: Query with unrelated prefix returns no transactions
    Then filtering by prefix "assets:fiat" should return 0 transactions

  Scenario: Query with no prefix returns all transactions
    Then filtering by prefix "" should return 4 transactions

  Scenario: Query aggregates balances from child accounts
    Then querying prefix "assets:crypto" should return aggregated balances from 2 accounts

  Scenario: Query leaf returns single account balance
    Then querying prefix "assets:crypto:exchange:kraken:personal" should return aggregated balances from 1 accounts
