Feature: Accounting system invariants

  Core invariants that any capital gains accounting system must satisfy.
  These are first-principle rules that apply regardless of specific
  transaction structures or matching algorithms.

  # ── 1. Reconciliation: sum of event values equals report totals ──

  Scenario: Event gains sum exactly to report totals
    Given a transactions file named "invariant_reconciliation.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 3 events
    And CGT event 1 should have quantity "7.00"
    And CGT event 1 should have cost basis "700.00"
    And CGT event 1 should have sale proceeds "1750.00"
    And CGT event 1 should have capital gain "1050.00"
    And CGT event 2 should have quantity "3.00"
    And CGT event 2 should have cost basis "300.00"
    And CGT event 2 should have sale proceeds "1050.00"
    And CGT event 2 should have capital gain "750.00"
    And CGT event 3 should have quantity "3.00"
    And CGT event 3 should have cost basis "600.00"
    And CGT event 3 should have sale proceeds "1050.00"
    And CGT event 3 should have capital gain "450.00"
    And the sum of event gains should equal the report total gains
    And the sum of event losses should equal the report total losses
    And the sum of event proceeds should equal the sum of individual proceeds
    And the sum of event cost bases should equal the sum of individual cost bases

  # ── 2. Profit/loss arithmetic: gain = proceeds - cost (exactly) ──

  Scenario: Every event satisfies gain equals proceeds minus cost
    Given a transactions file named "invariant_reconciliation.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then every CGT event should satisfy gain equals proceeds minus cost

  # ── 3. No double-counting: each lot consumed only once ──

  Scenario: Cross-commodity lots are never consumed by wrong commodity
    Given a transactions file named "invariant_cross_commodity.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 1 event
    And CGT event 1 should have commodity "ETH"
    And CGT event 1 should have cost basis "1000.00"
    And CGT event 1 should have sale proceeds "2000.00"
    And CGT event 1 should have capital gain "1000.00"
    And the CGT report should have no warnings

  Scenario: Quantity sold across events never exceeds quantity bought per commodity
    Given a transactions file named "invariant_reconciliation.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then total quantity sold per commodity should not exceed quantity bought

  # ── 4. Temporal ordering: out-of-order input produces correct FIFO ──

  Scenario: Reverse-ordered transactions produce same result as chronological
    Given a transactions file named "invariant_out_of_order.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 1 event
    And CGT event 1 should have buy date "2025-01-15"
    And CGT event 1 should have quantity "5.00"
    And CGT event 1 should have cost basis "500.00"
    And CGT event 1 should have sale proceeds "1500.00"
    And CGT event 1 should have capital gain "1000.00"

  # ── 5. No negative holdings: oversell warning and aftermath ──

  Scenario: Sequential oversell does not corrupt subsequent lot matching
    Given a transactions file named "invariant_sequential_oversell.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should have warnings containing "Sold more than held"
    # First sell: 5 matched (cost=500) + 2 unmatched (cost=0)
    And CGT event 1 should have quantity "5.00"
    And CGT event 1 should have cost basis "500.00"
    And CGT event 2 should have quantity "2.00"
    And CGT event 2 should have cost basis "0.00"
    # Second sell: 3 matched against new lot (cost=600)
    And CGT event 3 should have quantity "3.00"
    And CGT event 3 should have cost basis "600.00"
    And CGT event 3 should have sale proceeds "900.00"
    And CGT event 3 should have capital gain "300.00"

  # ── 6. Holding period: same-day, exact boundary, leap year ──

  Scenario: Same-day buy and sell has zero holding days and no discount
    Given a transactions file named "invariant_same_day.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 1 event
    And CGT event 1 should have holding days "0"
    And CGT event 1 should not be discount eligible
    And CGT event 1 should have capital gain "250.00"
    And CGT event 1 should have discounted gain "250.00"

  Scenario: Exactly 12 months is discount eligible, 1 day short is not
    Given a transactions file named "invariant_discount_boundary.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 2 events
    And CGT event 1 should be discount eligible
    And CGT event 2 should not be discount eligible

  # ── 7. Loss handling: discount never applies to losses ──

  Scenario: Loss on long-held asset does not receive CGT discount
    Given a transactions file named "invariant_loss_no_discount.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 1 event
    And CGT event 1 should have capital gain "-1000.00"
    And CGT event 1 should have discounted gain "-1000.00"
    And the CGT report total losses should be "1000.00"
    And the CGT report total gains should be "0.00"

  Scenario: Capital losses offset gains before the CGT discount is applied
    Given a transactions file named "invariant_loss_offsets_discount.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 2 events
    And CGT event 1 should have capital gain "1000.00"
    And CGT event 2 should have capital gain "-500.00"
    And the CGT report total gains should be "1000.00"
    And the CGT report total losses should be "500.00"
    And the CGT report net capital gain should be "500.00"
    And the CGT report total discounted gain should be "250.00"

  # ── 8. Rounding: fractional splits reconcile without drift ──

  Scenario: Three equal fractional splits reconcile to original lot cost
    Given a transactions file named "invariant_fractional_splits.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 3 events
    And the sum of event cost bases should equal "300.00"
    And the sum of event quantities should equal "100.00"
    And every CGT event should satisfy gain equals proceeds minus cost

  # ── 9. Complete matching: every sell produces events ──

  Scenario: Multiple sells all produce events with no silent drops
    Given a transactions file named "invariant_reconciliation.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the sum of event quantities should equal "13.00"
