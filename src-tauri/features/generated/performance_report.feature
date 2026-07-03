Feature: Performance report

  A rolling performance view over a window: realised capital gains and income
  bucketed by month, plus mark-to-market value vs FIFO cost basis at each
  month-end. Total return = realised + income + unrealised (as of the window
  end). Computed live, ex-cash. The fixture buys 10 ETH @ 100 AUD on 2025-07-10,
  sells 4 ETH for 1000 AUD on 2025-09-15 (FIFO cost 400 → realised 600), and
  earns 50 AUD interest on 2025-11-20, with month-end ETH prices.

  Background:
    Given a transactions file named "performance_basic.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    Given a prices file named "performance_basic.prices"
    When I run the prices parser on that file
    Then the prices parse should succeed

  Scenario: Opening baseline plus twelve month-end points across a financial year
    When I generate a performance report from "2025-07-01" to "2026-06-30" in "AUD"
    Then the performance report should have 13 points
    And performance point "Jul 2025" should have value "200.00"
    And performance point "Jul 2025" should have unrealised "200.00"
    And performance point "Sep 2025" should have realised "600.00"
    And performance point "Sep 2025" should have value "1500.00"
    And performance point "Sep 2025" should have unrealised "900.00"
    And performance point "Nov 2025" should have income "50.00"
    And performance point "Jun 2026" should have value "1730.00"
    And performance point "Jun 2026" should have unrealised "1080.00"

  Scenario: Window totals and headline total return
    When I generate a performance report from "2025-07-01" to "2026-06-30" in "AUD"
    Then the performance report total realised should be "600.00"
    And the performance report total income should be "50.00"
    And the performance report unrealised change should be "1080.00"
    And the performance report closing value should be "1730.00"
    And the performance report total return should be "1730.00"

  Scenario: A mid-month window end adds an exact endpoint snapshot
    When I generate a performance report from "2025-06-13" to "2026-06-12" in "AUD"
    Then the performance report should have 14 points
    And the first performance point date should be "2025-06-12"
    And the last performance point date should be "2026-06-12"
