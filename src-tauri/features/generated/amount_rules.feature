Feature: Amount-based filtering for rules

  Scenario: Rule with amount condition matches only qualifying outflows
    Given a clean sources directory with a CSV "bank/2025-01.csv":
      | Date       | Description  | Amount  |
      | 2025-01-10 | Payment A    | -50.00  |
      | 2025-01-11 | Payment B    | -150.00 |
      | 2025-01-12 | Payment C    | -200.00 |
    And a transform at "bank/_transform.rhai" that maps Date/Description/Amount to AUD
    And a rules file at "bank/_rules.json" matching "*Payment*" with contra "expenses:large" and amount condition "<-100"
    When I run the pipeline
    Then only transactions with amount less than -100 should use contra "expenses:large"
    And the transaction with description "Payment A" should use contra "expenses:unknown"

  Scenario: Amount condition combines with pattern match (AND)
    Given a clean sources directory with a CSV "bank/2025-01.csv":
      | Date       | Description  | Amount  |
      | 2025-01-10 | Coffee       | -5.00   |
      | 2025-01-11 | Payment Big  | -500.00 |
      | 2025-01-12 | Coffee       | -200.00 |
    And a transform at "bank/_transform.rhai" that maps Date/Description/Amount to AUD
    And a rules file at "bank/_rules.json" matching "*Coffee*" with contra "expenses:coffee" and amount condition "<-100"
    When I run the pipeline
    Then only the coffee transaction with amount less than -100 should use contra "expenses:coffee"

  Scenario: Rules without amount condition still work (backwards compat)
    Given a clean sources directory with a CSV and transform
    And a rules file at "bank/_rules.json" matching "imported" with payee "Test Merchant"
    When I run the pipeline
    Then transactions with narration "imported" should have payee "Test Merchant"

  Scenario: Rule with range amount condition
    Given a clean sources directory with a CSV "bank/2025-01.csv":
      | Date       | Description  | Amount  |
      | 2025-01-10 | Transfer     | -50.00  |
      | 2025-01-11 | Transfer     | -150.00 |
      | 2025-01-12 | Transfer     | -500.00 |
    And a transform at "bank/_transform.rhai" that maps Date/Description/Amount to AUD
    And a rules file at "bank/_rules.json" matching "*Transfer*" with contra "expenses:mid" and amount condition "-200..-100"
    When I run the pipeline
    Then only the transaction with amount between -200 and -100 should use contra "expenses:mid"
