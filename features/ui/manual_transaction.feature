Feature: Add manual transactions in the UI

  Scenario: Add a manual transaction and see it in the table
    Given the app is running
    When I add a manual transaction with payee "Manual UI" and notes "Coffee"
    Then I should see a transaction row for payee "Manual UI"

  Scenario: The manual modal shows the selected account as fixed context
    Given the app is running
    When I parse the transactions file "sample-data/example.transactions"
    And I select the account "assets:cash:usd"
    And I click the "Add New" button
    Then the account field should show "assets:cash:usd"
