Feature: Sidebar shows only asset accounts

  The sidebar navigation displays asset accounts directly (e.g., bitcoin, cba)
  without the "assets" group header. Non-asset account types (liabilities,
  equity, income, expenses) are not shown in the sidebar — they still exist
  as transaction posting counterparts but are not navigable.

  Scenario: Sidebar lists asset sub-accounts without the assets group
    Given the app is running
    When I parse the transactions file "sample-data/example.transactions"
    Then the sidebar should include the account "assets:cash:usd"
    And the sidebar should not include the account group "liabilities"
    And the sidebar should not include the account group "expenses"
    And the sidebar should not include the account group "income"
