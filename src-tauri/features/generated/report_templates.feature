Feature: Report template rendering

  Reports are generated as markdown files using Tera templates.
  The CGT report uses FIFO lot matching and the income report
  aggregates by account category.  Templates are rendered at
  pipeline rebuild time and stored as markdown files.

  Scenario: CGT report renders to markdown with FIFO data
    Given a transactions file named "cgt_trades.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I render the CGT report template for FY "2026" with base currency "AUD"
    Then the rendered report should contain "Capital Gains Tax Report"
    And the rendered report should contain "FY 2026"
    And the rendered report should contain "2025-08-20"
    And the rendered report should contain "ETH"
    And the rendered report should contain "1000.00"
    And the rendered report should contain "2000.00"
    And the rendered report should contain "Net Capital Gain"

  Scenario: CGT report with no events shows empty message
    Given a transactions file named "cgt_trades.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I render the CGT report template for FY "2020" with base currency "AUD"
    Then the rendered report should contain "No capital gains events"

  Scenario: Income report renders to markdown
    Given a transactions file named "income_expenses.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I render the income report template for FY "2026" with base currency "AUD"
    Then the rendered report should contain "Income Tax Report"
    And the rendered report should contain "FY 2026"

  Scenario: Pipeline generates report files for relevant FYs
    Given a clean sources directory with a file "richard/exchange/manual.transactions":
      """
      2025-01-15 * "Exchange" "Buy ETH"
          assets:exchange:eth 1 ETH {{ 1000.00 AUD }}
          equity:trading:buy -1000.00 AUD

      2025-08-20 * "Exchange" "Sell ETH"
          assets:exchange:eth -1 ETH @@ 2000.00 AUD
          equity:trading:sell 2000.00 AUD
      """
    When I run the pipeline
    Then the generated reports directory should contain "cgt-2026.md"
