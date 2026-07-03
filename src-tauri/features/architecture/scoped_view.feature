Feature: Scoped view — only load what the user is looking at

  The UI never loads the full transaction set into memory. Each view
  queries only the folders relevant to the selected account, paginated.
  The per-folder ledger.transactions files are the data store — queries
  read directly from them.

  Background:
    Given a clean sources directory

  Scenario: Viewing an account loads only that folder's transactions
    Given 1000 transactions across "bank/savings"
    And 500 transactions across "bank/checking"
    When I run the pipeline
    And I query "account:assets:savings" with limit 500
    Then the query should return at most 500 transactions
    And every returned transaction should have a posting to "assets:savings"
    And no returned transaction should have a posting to "assets:checking"

  Scenario: Scrolling loads the next page without reloading previous
    Given 1000 transactions across "bank/savings"
    When I run the pipeline
    And I query "account:assets:savings" with offset 0 and limit 500
    Then the query should return 500 transactions with total count 1000
    When I query "account:assets:savings" with offset 500 and limit 500
    Then the query should return 500 transactions
    And the second page should not overlap with the first page

  Scenario: Viewing a parent account reads all child folders
    Given a CSV "bank/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "bank/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    And a CSV "bank/checking/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-20 | Salary      | 3000   |
    And a transform at "bank/checking/_transform.rhai" that maps Date/Description/Amount to AUD
    When I run the pipeline
    And I query "account:assets" with limit 500
    Then the query should contain payee "Coffee Shop"
    And the query should contain payee "Salary"
    And the total count should be 2

  Scenario: Viewing a child account excludes ancestor and sibling folders
    Given a CSV "cash/bank/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-10 | Ancestor    | -10.00 |
    And a transform at "cash/bank/_transform.rhai" that maps Date/Description/Amount to AUD
    And a CSV "cash/bank/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Exact       | -4.50  |
    And a transform at "cash/bank/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    And a CSV "cash/bank/checking/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-20 | Sibling     | 3000   |
    And a transform at "cash/bank/checking/_transform.rhai" that maps Date/Description/Amount to AUD
    When I run the pipeline
    And I query "account:assets:bank:savings" with limit 500
    Then the query should contain payee "Exact"
    And the query should not contain payee "Ancestor"
    And the query should not contain payee "Sibling"
    And the total count should be 1

  Scenario: Show Ignored reveals a hidden transaction in its own account without touching the total
    Given a CSV "bank/savings/2025-01.csv":
      | Date       | Description  | Amount |
      | 2025-01-15 | Coffee Shop  | -4.50  |
      | 2025-01-20 | Junk Airdrop | 100.00 |
    And a transform at "bank/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    And a rules file at "bank/savings/_rules.json" matching "Junk Airdrop" with contra "ignore:hidden"
    When I run the pipeline
    And the generated config hides accounts "ignore:"
    And I query "account:assets:savings" with show ignored off
    Then the query should contain payee "Coffee Shop"
    And the query should not contain payee "Junk Airdrop"
    And the aggregated balance for "AUD" should be -4.50
    When I query "account:assets:savings" with show ignored on
    Then the query should contain payee "Coffee Shop"
    And the query should contain payee "Junk Airdrop"
    And the aggregated balance for "AUD" should be -4.50

  Scenario: Sidebar loads from summaries without parsing transactions
    Given a CSV "bank/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "bank/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    When I run the pipeline
    Then a summary file should exist at "bank/savings/summary.json"
    And loading the account tree should return balance for "assets:savings"
