@wip
Feature: Income report shows asset-grouped line items

  The Income report mirrors the Capital Gains report's expandable shape:
  events are grouped by commodity/asset, and clicking a header reveals
  per-event line items (date, account, quantity, price, value).

  Scenario: Asset groups render as collapsed headers by default
    Given the app is showing an income report with events
    Then each commodity group is rendered as a single header row
    And the header row shows the commodity, total quantity and total value
    And no per-event detail rows are visible

  Scenario: Clicking an asset header expands its line items
    Given the app is showing an income report with events
    When I click the header row for commodity "ETH"
    Then the per-event detail rows for "ETH" become visible
    And each detail row shows the date, category, quantity, price and value

  Scenario: Clicking the same header again collapses it
    Given the app is showing an income report with events
    When I click the header row for commodity "ETH"
    And I click the header row for commodity "ETH" again
    Then no per-event detail rows for "ETH" are visible

  Scenario: Date in a detail row links to the source transaction
    Given the app is showing an income report with events
    When I expand commodity "ETH"
    And I click the date cell of an event
    Then the app navigates to the asset account for that event
    And the search filter contains the event's "txn:" id
