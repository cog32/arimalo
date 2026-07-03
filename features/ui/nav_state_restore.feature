@wip
Feature: Navigation state is restored on reload

  When the app is reloaded, the heading and sidebar should show the same
  account. The saved navigation state (selectedAccount + drillPath) must
  survive the pre-parse render that runs before pipeline data arrives.

  Scenario: Heading matches sidebar after reload
    Given the app is running
    When I parse the transactions file "sample-data/example.transactions"
    And I select the account "assets:cash:usd"
    Then the heading should show "cash:usd"
    And the sidebar should highlight "assets:cash:usd"
    When the app is reloaded
    Then the heading should show "cash:usd"
    And the sidebar should highlight "assets:cash:usd"

  Scenario: Saved account that no longer exists falls back with matching sidebar
    Given the app is running
    When I parse the transactions file "sample-data/example.transactions"
    And the saved nav state has selectedAccount "assets:deleted:account" and drillPath ["deleted"]
    When the app is reloaded
    Then the heading and sidebar should show the same account
