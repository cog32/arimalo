@wip
Feature: Set price for a commodity via the value cell

  Scenario: Value cell shows clickable affordance when value column is visible
    Given the app is running with base currency "USD"
    And I select an account with a non-USD commodity
    Then each value cell should have the class "value-clickable"

  Scenario: Clicking a value cell opens the set-price modal
    Given the app is running with base currency "USD"
    And I select an account with commodity "BTC"
    When I click a value cell for a BTC transaction dated "2026-01-15"
    Then I should see the set-price modal
    And the date field should be "2026-01-15"
    And the commodity field should show "BTC"
    And the quote currency should default to "USD"

  Scenario: Clicking an em-dash value cell opens the set-price modal
    Given the app is running with base currency "USD"
    And I select an account with commodity "SOL" that has no price data
    When I click the em-dash value cell
    Then I should see the set-price modal
    And the commodity field should show "SOL"

  Scenario: Saving a price writes a P directive and refreshes values
    Given the app is running with base currency "USD"
    And I select an account with commodity "BTC"
    When I click a value cell for a BTC transaction dated "2026-01-15"
    And I enter price "30000.00"
    And I click "Save"
    Then the value cell should show the converted amount
    And the file "_prices/BTC.txt" should contain "P 2026-01-15 BTC 30000.00 USD"

  Scenario: Cancel closes the modal without saving
    Given the app is running with base currency "USD"
    And I select an account with commodity "BTC"
    When I click a value cell for a BTC transaction dated "2026-01-15"
    And I click "Cancel"
    Then I should not see the set-price modal
