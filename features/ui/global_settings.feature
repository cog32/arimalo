Feature: Global Settings — accounts included in balance

  A gear button in the sidebar opens a Global Settings dialog that lets the user
  add nominal account prefixes (e.g. assets:staking) to the set of accounts
  counted in the Balances, Performance and Tax Savings reports. Source-folder
  accounts (wallets, exchanges, banks) always count.

  Scenario: The Global Settings dialog shows the included-accounts editor
    Given the app is running
    When I open the global settings dialog
    Then I should see the included-accounts prefix input
    And I should see the included-accounts list
