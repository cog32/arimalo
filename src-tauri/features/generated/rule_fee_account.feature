Feature: Rule amount_account and fee_account replace non-asset posting accounts

  Scenario: Rule with amount_account replaces contra account and preserves fee
    Given a clean sources directory with a CSV "exchange/trades.csv":
      | Date                | Description | Amount | Fee  |
      | 2025-01-15 10:00:00 | Buy BTC     | -1000  | 2.50 |
    And a transform with fee at "exchange/_transform.rhai" using commodity "USD"
    And a rules file at "exchange/_rules.json" matching "*Buy*" with contra "equity:trading"
    When I run the pipeline
    Then transaction 1 should have 3 postings
    And posting 2 of transaction 1 should have account "equity:trading"
    And posting 3 of transaction 1 should have account "income:trading:fees"

  Scenario: Rule with amount_account and fee_account replaces both accounts
    Given a clean sources directory with a CSV "exchange/trades.csv":
      | Date                | Description | Amount | Fee  |
      | 2025-01-15 10:00:00 | Buy BTC     | -1000  | 2.50 |
    And a transform with fee at "exchange/_transform.rhai" using commodity "USD"
    And a rules file at "exchange/_rules.json" matching "*Buy*" with contra "equity:trading" and fee_account "expenses:fees:exchange"
    When I run the pipeline
    Then transaction 1 should have 3 postings
    And posting 2 of transaction 1 should have account "equity:trading"
    And posting 3 of transaction 1 should have account "expenses:fees:exchange"

  Scenario: Rule with amount_account on a 2-posting transaction
    Given a clean sources directory with a CSV "exchange/trades.csv":
      | Date                | Description | Amount |
      | 2025-01-15 10:00:00 | Buy BTC     | -1000  |
    And a transform at "exchange/_transform.rhai" that maps Date/Description/Amount to USD
    And a rules file at "exchange/_rules.json" matching "*Buy*" with contra "equity:trading"
    When I run the pipeline
    Then transaction 1 should have 2 postings
    And posting 2 of transaction 1 should have account "equity:trading"

  Scenario: Fee-only transaction with fee_account collapses to 2 postings
    Given a clean sources directory with a CSV "exchange/trades.csv":
      | Date                | Description | Amount | Fee  |
      | 2025-01-15 10:00:00 | Fee charge  | 0      | 2.50 |
    And a transform with fee at "exchange/_transform.rhai" using commodity "USD"
    And a rules file at "exchange/_rules.json" matching "*Fee*" with fee_account "expenses:fees:exchange"
    When I run the pipeline
    Then transaction 1 should have 2 postings
    And posting 1 of transaction 1 should have account "assets:exchange"
    And posting 1 of transaction 1 should have amount "-2.5"
    And posting 2 of transaction 1 should have account "expenses:fees:exchange"
    And posting 2 of transaction 1 should have amount "2.5"

  Scenario: Fee-only with both accounts keeps 3 postings
    Given a clean sources directory with a CSV "exchange/trades.csv":
      | Date                | Description | Amount | Fee  |
      | 2025-01-15 10:00:00 | Fee charge  | 0      | 2.50 |
    And a transform with fee at "exchange/_transform.rhai" using commodity "USD"
    And a rules file at "exchange/_rules.json" matching "*Fee*" with postings "equity:trading | expenses:fees:exchange"
    When I run the pipeline
    Then transaction 1 should have 2 postings
    And posting 1 of transaction 1 should have amount "-2.5"
    And posting 2 of transaction 1 should have account "expenses:fees:exchange"
