Feature: Transform fees

  Scenario: Transform with numeric fee produces 3-posting transaction
    Given a clean sources directory with a CSV "exchange/trades.csv":
      | Date                | Description | Amount | Fee  |
      | 2025-01-15 10:00:00 | Buy BTC     | -1000  | 2.50 |
    And a transform with fee at "exchange/_transform.rhai" using commodity "USD"
    When I run the pipeline
    Then transaction 1 should have 3 postings
    And posting 1 of transaction 1 should have account "assets:exchange"
    And posting 1 of transaction 1 should have amount "-1002.5"
    And posting 2 of transaction 1 should have account "expenses:unknown"
    And posting 2 of transaction 1 should have amount "1000"
    And posting 3 of transaction 1 should have account "income:trading:fees"
    And posting 3 of transaction 1 should have amount "2.5"

  Scenario: Transform with "amount commodity" fee uses specified commodity
    Given a clean sources directory with a CSV "exchange/trades.csv":
      | Date                | Description | Amount | Fee      |
      | 2025-01-15 10:00:00 | Buy BTC     | -1000  | 2.50 ETH |
    And a transform with compound fee at "exchange/_transform.rhai" using commodity "USD"
    When I run the pipeline
    Then transaction 1 should have 3 postings
    And posting 3 of transaction 1 should have account "income:trading:fees"
    And posting 3 of transaction 1 should have amount "2.5"
    And posting 3 of transaction 1 should have commodity "ETH"

  Scenario: Zero fee produces standard 2-posting transaction
    Given a clean sources directory with a CSV "exchange/trades.csv":
      | Date                | Description | Amount | Fee |
      | 2025-01-15 10:00:00 | Buy BTC     | -1000  | 0   |
    And a transform with fee at "exchange/_transform.rhai" using commodity "USD"
    When I run the pipeline
    Then transaction 1 should have 2 postings

  Scenario: Absent fee field produces standard 2-posting transaction
    Given a clean sources directory with a CSV "exchange/trades.csv":
      | Date                | Description | Amount |
      | 2025-01-15 10:00:00 | Buy BTC     | -1000  |
    And a transform at "exchange/_transform.rhai" that maps Date/Description/Amount to USD
    When I run the pipeline
    Then transaction 1 should have 2 postings

  Scenario: Contra absorbs fee correctly
    Given a clean sources directory with a CSV "exchange/trades.csv":
      | Date                | Description | Amount | Fee  |
      | 2025-01-15 10:00:00 | Buy BTC     | -1000  | 2.50 |
    And a transform with fee at "exchange/_transform.rhai" using commodity "USD"
    When I run the pipeline
    Then the postings of transaction 1 should sum to zero

  Scenario: meta_extra appended to transaction metadata
    Given a clean sources directory with a CSV "exchange/trades.csv":
      | Date                | Description | Amount | Refid  |
      | 2025-01-15 10:00:00 | Buy BTC     | -1000  | ABC123 |
    And a transform with meta_extra at "exchange/_transform.rhai" using commodity "USD"
    When I run the pipeline
    Then transaction 1 meta should include "src:ABC123"

  Scenario: Shared src ref triggers trade link suggestion
    Given a clean sources directory with a CSV "exchange/trades.csv":
      | Date                | Description | Amount | Refid  | Asset |
      | 2025-01-15 10:00:00 | Trade       | -1000  | REF001 | USD   |
      | 2025-01-15 10:00:00 | Trade       | 0.5    | REF001 | BTC   |
    And a transform with src ref at "exchange/_transform.rhai"
    When I run the pipeline
    And I request trade link suggestions
    Then I should receive 1 trade suggestion
