@wip
Feature: Balances report navigation and interaction

  The Balances report is a point-in-time portfolio snapshot: per-commodity
  quantities valued in the base currency, as of the end of the selected FY (or
  the "to" date in custom range mode). It appears as a third button under the
  existing Tax sidebar menu.

  Scenario: Balances button is reachable from the sidebar
    Given the app is showing the Reports view
    When I click the "Balances" button in the report menu
    Then the report content should display the Balances summary card
    And the report content should list per-commodity holdings

  Scenario: Switching FY refetches the report as of that FY end
    Given the app is showing a Balances report
    When I change the FY dropdown to "2025"
    Then the balances report header should show "as of 2025-06-30"
    And each holding's quantity reflects only transactions up to 2025-06-30

  Scenario: Scope input narrows holdings to postings beneath the prefix
    Given the app is showing a Balances report
    When I set the account scope to "assets:crypto"
    Then only commodities held in assets:crypto postings appear
    And fiat commodities from sibling scopes are excluded

  Scenario: Commodities without a resolvable AUD price appear in warnings
    Given the app is showing a Balances report for a vault containing an unpriced commodity
    Then the warnings section lists "No AUD price for" that commodity
    And that commodity is not in the holdings table

  Scenario: Portfolio weight bars are proportional and sum to ~100%
    Given the app is showing a Balances report with 3 holdings
    Then each row has a .balanceWeightBar whose inline width matches its portfolio_weight
    And the weight percentages in the table sum to approximately 100%

  Scenario: Custom date mode uses the "to" date as the as-of date
    Given the app is showing a Balances report
    When I switch the date controls to Custom mode
    And I set the "to" date to "2026-03-31" and press Go
    Then the balances report header should show "as of 2026-03-31"
    And a transaction dated 2026-04-15 is excluded from the snapshot

  Scenario: Filter input narrows the holdings table by commodity substring
    Given the app is showing a Balances report with BTC, ETH, and SOL
    When I type "BT" into the balances filter input
    Then the holdings table shows only BTC

  Scenario: Clicking a commodity row reveals its leaf-account breakdown
    Given the app is showing a Balances report with BTC split across two leaf accounts
    When I click the BTC row in the holdings table
    Then a sub-row is rendered for each leaf account contributing to BTC
    And each sub-row shows the leaf account name, its quantity, and its value
    And the chevron on the BTC row rotates to indicate the expanded state
    And clicking BTC again collapses the sub-rows

  Scenario: Expansion state is per-commodity and independent
    Given the app is showing a Balances report with BTC and ETH expanded
    When I collapse BTC
    Then the ETH breakdown remains visible
    And only BTC's chevron returns to the collapsed orientation
