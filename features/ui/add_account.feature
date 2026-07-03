Feature: Add a new account via the UI

  Scenario: Add a new account using the "+ Add Account" button
    Given the app is running
    When I click the "Add Account" button
    And I enter account name "assets:bank:savings"
    And I submit the new account form
    Then the sidebar should include the account "assets:bank:savings"
