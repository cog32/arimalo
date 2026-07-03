Feature: Transaction search and sort

  Transactions can be searched using field:value terms with AND/OR operators,
  negation, and sorted by date or amount. This is the same query language
  used by the UI and the arimalo-query CLI.

  Background:
    Given a clean sources directory
    And a CSV "richard/crypto/exchange/binance/personal/2025-01.csv":
      | Date       | Description    | Amount   |
      | 2025-01-10 | Buy BTC        | -500.00  |
      | 2025-01-15 | Deposit        | 1000.00  |
      | 2025-01-20 | Sell ETH       | 300.00   |
    And a transform at "richard/crypto/exchange/binance/personal/_transform.rhai" without an account field
    And a CSV "richard/cash/bank/cba/savings/2025-01.csv":
      | Date       | Description    | Amount  |
      | 2025-01-05 | Interest       | 5.00    |
      | 2025-01-25 | Transfer out   | -200.00 |
    And a transform at "richard/cash/bank/cba/savings/_transform.rhai" without an account field
    When I run the pipeline for month "202501"

  # --- Search terms ---

  Scenario: Search by account prefix
    Then searching "account:crypto" should return 3 transactions

  Scenario: Search by payee
    Then searching "payee:Buy" should return 1 transactions

  Scenario: Search by payee regex
    Then searching "payee:Buy|Sell" should return 2 transactions

  Scenario: Search by payee no match
    Then searching "payee:nonexistent" should return 0 transactions

  Scenario: Search with free text matches across all fields
    Then searching "Interest" should return 1 transactions

  Scenario: Search by date
    Then searching "date:2025-01-10" should return 1 transactions

  Scenario: Search by amount condition greater than
    Then searching "amount:>100" should return 2 transactions

  Scenario: Search by amount condition less than
    Then searching "amount:<10" should return 3 transactions

  # --- AND / OR ---

  Scenario: AND combines filters
    Then searching "account:crypto AND payee:Buy" should return 1 transactions

  Scenario: OR widens filters
    Then searching "payee:Interest OR payee:Deposit" should return 2 transactions

  # --- Negation ---

  Scenario: Negated term excludes matches
    Then searching "-payee:Deposit" should return 4 transactions

  Scenario: Negated field with AND
    Then searching "account:crypto AND -payee:Buy" should return 2 transactions

  # --- Sort ---

  Scenario: Sort by amount descending
    Then searching "account:crypto" sorted by "amount" "desc" the first transaction amount should be 1000.00

  Scenario: Sort by amount ascending
    Then searching "account:crypto" sorted by "amount" "asc" the first transaction amount should be -500.00

  Scenario: Sort by date descending
    Then searching "" sorted by "date" "desc" the first transaction date should be "2025-01-25"

  Scenario: Sort by date ascending
    Then searching "" sorted by "date" "asc" the first transaction date should be "2025-01-05"

  # --- Date conditions ---

  Scenario: Date greater than or equal
    Then searching "date:>=2025-01-15" should return 3 transactions

  Scenario: Date less than
    Then searching "date:<2025-01-10" should return 1 transactions

  Scenario: Date range
    Then searching "date:2025-01-10..2025-01-20" should return 3 transactions

  Scenario: Date range combined with account filter
    Then searching "account:crypto AND date:>=2025-01-15" should return 2 transactions

  # --- Default sort ---

  Scenario: Default sort is date descending (most recent first)
    Then searching "" with no explicit sort the first transaction date should be "2025-01-25"

  # --- Limit ---

  Scenario: Limit restricts number of results
    Then searching "" sorted by "date" "asc" with limit 2 should return 2 transactions
