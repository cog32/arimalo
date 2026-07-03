Feature: Income tax report

  Generate an income and expenses report for an Australian financial year.
  Transactions with postings to income:* and expenses:* accounts within the
  FY date range are grouped by account and summed.

  Background:
    Given a transactions file named "income_expenses.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed

  Scenario: Income categorised by account within FY
    When I generate an income report for FY "2026" with base currency "AUD"
    Then the income report should have 2 income categories
    And income category "income:salary" should total "10000.00"
    And income category "income:interest" should total "50.00"

  Scenario: Expenses categorised by account within FY
    When I generate an income report for FY "2026" with base currency "AUD"
    Then the income report should have 2 expense categories
    And expense category "expenses:office" should total "200.00"
    And expense category "expenses:internet" should total "80.00"

  Scenario: Income report totals
    When I generate an income report for FY "2026" with base currency "AUD"
    Then the income report total income should be "10050.00"
    And the income report total expenses should be "280.00"
    And the income report net should be "9770.00"

  Scenario: Transactions outside FY are excluded
    When I generate an income report for FY "2023" with base currency "AUD"
    Then the income report should have 0 income categories
    And the income report should have 0 expense categories
