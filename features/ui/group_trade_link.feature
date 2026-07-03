Feature: Link aggregated trade groups

  Aggregated exchange fills (e.g. multiple BUY HNTUSDT fills) create two
  commodity groups: one for inflows, one for outflows. These should show
  a link icon on the group header and be linkable as trade pairs.

  Scenario: Aggregated exchange fills show a link icon on group headers
    Given the app is running
    When I parse the transactions file "src-tauri/features/fixtures/multi_fill_exchange.transactions"
    And I select the account "equity:trading"
    Then I should see 2 aggregated group headers
    And the first group header should have a chain link button

  Scenario: Clicking the group link icon links all pairs
    Given the app is running
    When I parse the transactions file "src-tauri/features/fixtures/multi_fill_exchange.transactions"
    And I select the account "equity:trading"
    And I click the group chain link button
    Then the aggregated groups should dissolve into swap rows
