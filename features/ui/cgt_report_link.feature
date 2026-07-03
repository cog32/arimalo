@wip
Feature: CGT report link navigates to correct account

  When clicking a transaction link in the CGT report, the app should
  resolve the actual account the transaction belongs to, even if it
  differs from the account shown in the report row.

  Scenario: Report link navigates to the transaction's actual account
    Given the app is showing a CGT report
    When I click a report link for txn "txn:ABC-123" with account "assets:crypto:exchange:kraken:personal"
    And the transaction actually belongs to "assets:crypto:exchange:hyperliquid:personal"
    Then the selected account should be "assets:crypto:exchange:hyperliquid:personal"
    And the search filter should contain "meta:txn:ABC-123"

  Scenario: Report link falls back to provided account when lookup fails
    Given the app is showing a CGT report
    When I click a report link for txn "txn:MISSING-999" with account "assets:crypto:exchange:kraken:personal"
    And the transaction cannot be found
    Then the selected account should be "assets:crypto:exchange:kraken:personal"
    And the search filter should contain "meta:txn:MISSING-999"
