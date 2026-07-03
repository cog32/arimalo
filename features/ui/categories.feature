Feature: Categories page lists nominal and contra accounts

  The Categories view (parallel to Accounts) exposes every account NOT backed by
  a source folder — income, expenses, equity, liabilities, and the synthetic
  asset contras such as staking and lending. These are deliberately hidden from
  the Accounts sidebar, which lists only folder-backed asset accounts. The
  Categories view reuses the same drill-down tree and transaction table, and its
  rows are loaded via the union query (query_global) because no source folder
  derives to a nominal/contra account name (the folder-pruned query_search would
  return nothing).

  Scenario: Nominal accounts are navigable under Categories
    Given the app is running
    When I parse the transactions file "src-tauri/features/fixtures/example.transactions"
    And I switch to the Categories view
    Then the Categories sidebar should show the root "income"
    When I drill into the category "income"
    And I drill into the category "income:trading"
    Then the sidebar should include the account "income:trading:pnl"

  Scenario: Selecting a category loads its transactions via the union query
    Given the app is running
    When I parse the transactions file "src-tauri/features/fixtures/example.transactions"
    And I switch to the Categories view
    And I drill into the category "income"
    And I drill into the category "income:trading"
    And I select the category account "income:trading:pnl"
    Then I should see at least one transaction row
