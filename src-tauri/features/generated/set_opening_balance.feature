Feature: Set opening balance for existing accounts

  Accounts imported from CSV/OFX sources show as "Unverified Balance"
  because they have no opening balance declaration. Users need to set
  an opening balance on existing accounts to verify their balance.

  Scenario: Set opening balance on account with no existing opening
    Given a clean sources directory with a CSV "bank/test/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "bank/test/_transform.rhai" that maps Date/Description/Amount to AUD
    And an accounts file at "bank/test/accounts.transactions" with:
      """
      account assets:bank:test AUD
      """
    When I run the pipeline for month "202501"
    Then the account "assets:bank:test" should not have an opening balance
    When I set the opening balance for "assets:bank:test" to "1000.00" "AUD"
    Then the account "assets:bank:test" should have an opening balance
    And the accounts file at "bank/test/accounts.transactions" should contain "opening 1000.00 AUD"

  Scenario: Update existing opening balance
    Given a clean sources directory with a CSV "bank/test/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "bank/test/_transform.rhai" that maps Date/Description/Amount to AUD
    And an accounts file at "bank/test/accounts.transactions" with:
      """
      account assets:bank:test AUD
          opening 500.00 AUD
      """
    When I set the opening balance for "assets:bank:test" to "1000.00" "AUD"
    Then the accounts file at "bank/test/accounts.transactions" should contain "opening 1000.00 AUD"
    And the accounts file at "bank/test/accounts.transactions" should not contain "opening 500.00 AUD"

  Scenario: Set opening balance preserves other properties
    Given a clean sources directory with a CSV "bank/test/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "bank/test/_transform.rhai" that maps Date/Description/Amount to AUD
    And an accounts file at "bank/test/accounts.transactions" with:
      """
      account assets:bank:test AUD
          name My Savings
      """
    When I set the opening balance for "assets:bank:test" to "250.00" "AUD"
    Then the accounts file at "bank/test/accounts.transactions" should contain "name My Savings"
    And the accounts file at "bank/test/accounts.transactions" should contain "opening 250.00 AUD"

  Scenario: Set opening balance on folder-derived account with no accounts file
    Given a clean sources directory with a CSV "richard/cash/bank/ubank/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Transfer    | 100.00 |
    And a transform at "richard/cash/bank/ubank/savings/_transform.rhai" without an account field
    And no "accounts.transactions" file exists
    When I run the pipeline for month "202501"
    Then the transaction should use account "assets:cash:bank:ubank:savings"
    When I set the opening balance for "assets:cash:bank:ubank:savings" to "500.00" "AUD" in account set "richard"
    Then the accounts file at "richard/cash/bank/ubank/savings/accounts.transactions" should contain "account assets:cash:bank:ubank:savings"
    And the accounts file at "richard/cash/bank/ubank/savings/accounts.transactions" should contain "opening 500.00 AUD"

  Scenario: Set opening balance when deepest folder does not exist
    Given a clean sources directory with a CSV "richard/cash/bank/ubank/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Transfer    | 100.00 |
    And a transform at "richard/cash/bank/ubank/_transform.rhai" without an account field
    And an accounts file at "richard/cash/bank/ubank/accounts.transactions" with:
      """
      account assets:cash:bank:ubank AUD
      """
    When I run the pipeline for month "202501"
    When I set the opening balance for "assets:cash:bank:ubank:savings" to "500.00" "AUD" in account set "richard"
    Then the accounts file at "richard/cash/bank/ubank/savings/accounts.transactions" should contain "account assets:cash:bank:ubank:savings"
    And the accounts file at "richard/cash/bank/ubank/savings/accounts.transactions" should contain "opening 500.00 AUD"
