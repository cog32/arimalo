@wip
Feature: Transaction aggregation groups by narration

  Transactions on the same date, venue, and commodity should only be
  aggregated together if they also share the same narration (notes).
  Different trade types (e.g. "SELL SOLUSDT" vs "Interest USDT") must
  remain in separate groups even when they share a commodity.

  The group header should display the narration with a count suffix,
  e.g. "SELL SOLUSDT (3)" instead of a generic "3 transactions".

  Scenario: Different narrations on same date+venue+commodity are not grouped
    Given the app has 3 "SELL SOLUSDT" and 3 "Interest USDT" transactions on "2025-03-21" from "Bybit" in "USDT"
    When I view the transaction list for "assets:exchange"
    Then I should see 2 aggregated groups
    And one group should show "SELL SOLUSDT (3)"
    And another group should show "Interest USDT (3)"

  Scenario: Same narration transactions are still grouped together
    Given the app has 5 "SELL SOLUSDT" transactions on "2025-03-21" from "Bybit" in "USDT"
    When I view the transaction list for "assets:exchange"
    Then I should see 1 aggregated group showing "SELL SOLUSDT (5)"
