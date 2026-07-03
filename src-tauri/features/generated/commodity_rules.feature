Feature: Commodity rename rules

  Scenario: Rule renames commodity on matching transactions
    Given a clean sources directory with a CSV "bank/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "bank/_transform.rhai" that outputs commodity "UNKNOWN_TOKEN"
    And a rules file at "bank/_rules.json" matching commodity "UNKNOWN_TOKEN" with commodity "WETH"
    When I run the pipeline for month "202501"
    Then transactions should have commodity "WETH"

  Scenario: Commodity rule does not affect non-matching transactions
    Given a clean sources directory with a CSV "bank/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "bank/_transform.rhai" that outputs commodity "ETH"
    And a rules file at "bank/_rules.json" matching commodity "UNKNOWN_TOKEN" with commodity "WETH"
    When I run the pipeline for month "202501"
    Then transactions should have commodity "ETH"

  Scenario: Commodity rename preserves raw amount_commodity and sets display
    Given a clean sources directory with a CSV "bank/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "bank/_transform.rhai" that outputs commodity "0xecf8f87f"
    And a rules file at "bank/_rules.json" matching commodity "0xecf8f87f" with commodity "WETH"
    When I run the pipeline for month "202501"
    Then transactions should have commodity "WETH"
    And transaction amount_commodity should be "0xecf8f87f"
    And transaction display_amount_commodity should be "WETH"

  Scenario: Commodity rename does not block categorization rule
    Given a clean sources directory with a CSV "bank/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Swap tokens | -4.50  |
    And a transform at "bank/_transform.rhai" that outputs commodity "0xecf8f87f"
    And a rules file at "bank/_rules.json" with commodity rename "0xecf8f87f" to "SPAM" and payee rule "*Swap*" to "DEX Swap"
    When I run the pipeline for month "202501"
    Then transactions should have commodity "SPAM"
    And transactions with narration "imported" should have payee "DEX Swap"

  Scenario: Commodity rename does not block account assignment rule
    Given a clean sources directory with a CSV "bank/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Swap tokens | -4.50  |
    And a transform at "bank/_transform.rhai" that outputs commodity "0xecf8f87f"
    And a rules file at "bank/_rules.json" with commodity rename "0xecf8f87f" to "SPAM" and contra rule "*Swap*" to "expenses:trading"
    When I run the pipeline for month "202501"
    Then transactions should have commodity "SPAM"
    And transactions with narration "imported" should use contra "expenses:trading"

  Scenario: Payee-only rule does not block categorization rule
    Given a clean sources directory with a CSV "bank/2025-01.csv":
      | Date       | Description   | Amount |
      | 2025-01-15 | Swap tokens   | -4.50  |
    And a transform at "bank/_transform.rhai" that outputs commodity "AUD"
    And a rules file at "bank/_rules.json" with payee rule "*Swap*" to "Jupiter v6" and contra rule "*imported*" to "equity:trading:buy"
    When I run the pipeline for month "202501"
    Then transactions with narration "imported" should have payee "Jupiter v6"
    And transactions with narration "imported" should use contra "equity:trading:buy"

  Scenario: Categorization rule wins over payee-only rule on same pattern
    Given a clean sources directory with a CSV "bank/2025-01.csv":
      | Date       | Description   | Amount |
      | 2025-01-15 | Swap tokens   | -4.50  |
    And a transform at "bank/_transform.rhai" that outputs commodity "AUD"
    And a rules file at "bank/_rules.json" with categorization rule "*Swap*" payee "Jupiter" contra "expenses:defi:jupiter" and payee-only rule "*Swap*" to "SOL token account"
    When I run the pipeline for month "202501"
    Then transactions with narration "imported" should have payee "Jupiter"
    And transactions with narration "imported" should use contra "expenses:defi:jupiter"
