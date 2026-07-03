Feature: Income report scoped to base account

  When a base account scope is provided, only transactions with at
  least one posting under that scope are included in the report.

  Background:
    Given a transactions file named "income_scoped.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed

  Scenario: Scoped to crypto excludes banking income
    When I generate an income report for FY "2026" with base currency "AUD" scoped to "assets:crypto"
    Then the income report should have 2 income categories
    And income category "income:trading:pnl" should total "1000.00"
    And income category "income:airdrops" should total "200.00"
    And the income report total income should be "1200.00"

  Scenario: Unscoped report includes all income
    When I generate an income report for FY "2026" with base currency "AUD"
    Then the income report should have 4 income categories
    And the income report total income should be "6250.00"
