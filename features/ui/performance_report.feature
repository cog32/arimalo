@wip
Feature: Performance report navigation and interaction

  The Performance report is a rolling 12-month portfolio view: realised capital
  gains and income bucketed by month, plus mark-to-market value vs FIFO cost
  basis at each month-end, drawn as a line/area chart above a monthly table. It
  appears under a new "Portfolio" group in the Reports sidebar, below the Tax
  group. Total return = realised + income + unrealised (as of the window end).

  Scenario: Performance button is reachable from the sidebar Portfolio group
    Given the app is showing the Reports view
    Then the report menu shows a "Portfolio" heading
    When I click the "Performance" button in the report menu
    Then the report content should display the performance summary card
    And the report content should display the performance chart
    And the report content should display the monthly performance table

  Scenario: Headline splits total return into realised, income and unrealised
    Given the app is showing a Performance report
    Then the summary shows a total return figure
    And the summary shows separate Realised, Income and Unrealised sub-stats
    And the Unrealised sub-stat is labelled "as of" the window end date

  Scenario: The chart plots invested value against cost basis
    Given the app is showing a Performance report
    Then the chart shows an "Invested value" series and a "Cost basis" series
    And hovering a month shows that month's realised, income, unrealised and value

  Scenario: Switching the financial year refetches the 12-month window
    Given the app is showing a Performance report
    When I change the FY dropdown to "2025"
    Then the performance window covers that financial year
    And the monthly table shows one row per month-end in the window

  Scenario: Custom mode uses the chosen window end
    Given the app is showing a Performance report
    When I switch to Custom date mode and set the dates
    Then the performance report recomputes for the chosen window

  Scenario: Scoping narrows the report to an account subtree
    Given the app is showing a Performance report
    When I set the Scope input to "assets:crypto"
    Then the report recomputes using only accounts under that prefix

  Scenario: The chart survives an unrelated re-render
    Given the app is showing a Performance report
    When an unrelated re-render occurs (e.g. editing the Scope input)
    Then the chart SVG node is preserved, not recreated

  Scenario: An empty window shows a placeholder and no chart
    Given a Performance report whose window contains no holdings or activity
    Then the report shows an empty-state message
    And no chart is rendered

  Scenario: A growth-by-category chart compares the children at the current scope
    Given the app is showing a Performance report
    Then below the value chart a "Growth by category" chart is shown
    And it draws one line per direct-child account at the current scope
    And every line is rebased to 0% at the window's opening snapshot
    And the lines share a dashed 0% baseline so growth rates compare on one axis

  Scenario: Growth lines re-bucket when the scope changes
    Given the app is showing a Performance report
    When I set the Scope input to "assets"
    Then the growth chart shows a line for each direct child of "assets" (cash, crypto, …)
    When I set the Scope input to "assets:crypto"
    Then the growth chart shows a line for each direct child of "assets:crypto"

  Scenario: Working/contra accounts are not drawn as growth lines
    Given the app is showing a Performance report scoped to "assets"
    Then the growth chart shows no line for transfer, lending, bridge, wrap or staking contras
    And those balances still count inside their parent group at the top level
