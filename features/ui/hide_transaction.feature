Feature: Hide a single transaction from the accounts table

  The actions column on every transaction row in the accounts view has
  a small "x" button. For CSV-sourced transactions its title reads
  "Hide transaction": pressing it must add that transaction's id to
  the ignored list and remove the row from the visible table without
  requiring a manual rebuild or page reload.

  Scenario: Clicking the row hide button removes the transaction from the table
    Given the app is running
    When I parse the transactions file "src-tauri/features/fixtures/example.transactions"
    And I select the account "assets:exchange:kraken:btc"
    And I should see a transaction row for payee "Kraken"
    When I click the hide button on the transaction row for payee "Kraken"
    Then I should not see a transaction row for payee "Kraken"
