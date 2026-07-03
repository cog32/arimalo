Feature: Income report nets gains against losses

  When an income account has both credits (gains) and debits (losses),
  the income report should net them — not sum their absolute values.

  Background:
    Given a transactions file named "income_pnl.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed

  Scenario: Trading PnL nets losses against gains
    When I generate an income report for FY "2026" with base currency "AUD"
    Then the income report should have 1 income categories
    And income category "income:trading:pnl" should total "1200.00"
    And the income report total income should be "1200.00"
