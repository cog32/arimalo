Feature: Income tax report — per-asset line items

  Each income/expense posting in the FY produces a line item in the report.
  Events carry date, account (income:* / expenses:*), commodity, quantity,
  per-unit price, and base-currency value, so the UI can group them by asset
  (mirroring the Capital Gains report's expandable groups).

  Background:
    Given a transactions file named "income_with_assets.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed

  Scenario: Income events broken out per asset/commodity
    When I generate an income report for FY "2026" with base currency "AUD"
    Then the income report should have 5 income events
    And the income report should have 2 income events for commodity "ETH"
    And the income report should have 1 income event for commodity "BTC"
    And the income report should have 1 income event for commodity "AUD"
    And the income report should have 1 income event for commodity "USDC"

  Scenario: Each income event records date, quantity, price, value and account
    When I generate an income report for FY "2026" with base currency "AUD"
    Then income event "ETH" on "2025-09-10" should have quantity "0.50"
    And income event "ETH" on "2025-09-10" should have price "3000.00"
    And income event "ETH" on "2025-09-10" should have value "1500.00"
    And income event "ETH" on "2025-09-10" should have account "income:staking:eth"

  Scenario: Fiat-only income produces a single base-currency event
    When I generate an income report for FY "2026" with base currency "AUD"
    Then income event "AUD" on "2025-07-15" should have quantity "5000.00"
    And income event "AUD" on "2025-07-15" should have price "1.00"
    And income event "AUD" on "2025-07-15" should have value "5000.00"
    And income event "AUD" on "2025-07-15" should have account "income:salary"

  Scenario: Income event records the asset account it landed in
    When I generate an income report for FY "2026" with base currency "AUD"
    Then income event "ETH" on "2025-09-10" should have asset account "assets:crypto:wallet:eth"
    And income event "AUD" on "2025-07-15" should have asset account "assets:bank:checking"

  Scenario: Fees recorded as positive on income:* contribute negatively
    When I generate an income report for FY "2026" with base currency "AUD"
    Then income event "USDC" on "2025-08-20" should have quantity "-100.00"
    And income event "USDC" on "2025-08-20" should have price "1.50"
    And income event "USDC" on "2025-08-20" should have value "-150.00"
    And income event "USDC" on "2025-08-20" should have account "income:trading:fees"

  Scenario: Sum of income event values matches the income category total
    When I generate an income report for FY "2026" with base currency "AUD"
    Then income category "income:trading:fees" should total "-150.00"

  Scenario: Expense events captured per asset/commodity
    When I generate an income report for FY "2026" with base currency "AUD"
    Then the income report should have 1 expense event
    And the income report should have 1 expense event for commodity "AUD"
    And expense event "AUD" on "2025-08-10" should have value "80.00"
    And expense event "AUD" on "2025-08-10" should have account "expenses:internet"

  Scenario: Events outside the FY are excluded
    When I generate an income report for FY "2023" with base currency "AUD"
    Then the income report should have 0 income events
    And the income report should have 0 expense events
