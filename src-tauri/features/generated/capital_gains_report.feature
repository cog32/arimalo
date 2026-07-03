Feature: Capital gains tax report

  Generate a CGT report using FIFO (First In, First Out) lot matching.
  When a commodity is sold, the system automatically identifies which
  acquisition lots are consumed (oldest first) and computes cost basis.
  Only sell events within the FY are included.  CGT discount applies when
  the holding period exceeds the configured threshold (default 12 months / 50%).

  Background:
    Given a transactions file named "cgt_trades.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed

  Scenario: FIFO matches sell events to oldest acquisitions
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 2 events
    And CGT event 1 should have sell date "2025-08-20"
    And CGT event 1 should have commodity "ETH"
    And CGT event 1 should have cost basis "1000.00"
    And CGT event 1 should have sale proceeds "2000.00"
    And CGT event 1 should have capital gain "1000.00"
    And CGT event 2 should have sell date "2025-09-15"
    And CGT event 2 should have commodity "BTC"
    And CGT event 2 should have cost basis "500.00"
    And CGT event 2 should have sale proceeds "600.00"
    And CGT event 2 should have capital gain "100.00"

  Scenario: CGT discount applied for holdings over 12 months
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then CGT event 1 should be discount eligible
    And CGT event 1 should have discounted gain "500.00"
    And CGT event 2 should not be discount eligible
    And CGT event 2 should have discounted gain "100.00"

  Scenario: CGT report totals
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report total gains should be "1100.00"
    And the CGT report total losses should be "0.00"
    And the CGT report net capital gain should be "1100.00"
    And the CGT report total discounted gain should be "600.00"

  Scenario: Sell outside financial year is excluded
    When I generate a CGT report for FY "2025" with base currency "AUD"
    Then the CGT report should contain 0 events

  Scenario: FIFO consumes multiple lots oldest first
    Given a transactions file named "fifo_basic.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 2 events
    And CGT event 1 should have sell date "2025-08-10"
    And CGT event 1 should have buy date "2025-01-15"
    And CGT event 1 should have commodity "ETH"
    And CGT event 1 should have quantity "10.00"
    And CGT event 1 should have cost basis "1000.00"
    And CGT event 1 should have sale proceeds "3000.00"
    And CGT event 1 should have capital gain "2000.00"
    And CGT event 2 should have sell date "2025-08-10"
    And CGT event 2 should have buy date "2025-03-20"
    And CGT event 2 should have quantity "2.00"
    And CGT event 2 should have cost basis "400.00"
    And CGT event 2 should have sale proceeds "600.00"
    And CGT event 2 should have capital gain "200.00"

  Scenario: Partial lot consumption leaves remainder
    Given a transactions file named "fifo_partial.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 1 event
    And CGT event 1 should have quantity "3.00"
    And CGT event 1 should have cost basis "150.00"
    And CGT event 1 should have sale proceeds "300.00"
    And CGT event 1 should have capital gain "150.00"

  Scenario: Multi-commodity inventories tracked independently
    Given a transactions file named "fifo_multi_commodity.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 2 events
    And CGT event 1 should have commodity "ETH"
    And CGT event 1 should have cost basis "500.00"
    And CGT event 1 should have sale proceeds "750.00"
    And CGT event 1 should have capital gain "250.00"
    And CGT event 2 should have commodity "BTC"
    And CGT event 2 should have cost basis "25000.00"
    And CGT event 2 should have sale proceeds "30000.00"
    And CGT event 2 should have capital gain "5000.00"

  Scenario: Swap creates disposal and acquisition lot
    Given a transactions file named "fifo_swap.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 2 events
    And CGT event 1 should have sell date "2025-10-15"
    And CGT event 1 should have commodity "ETH"
    And CGT event 1 should have cost basis "2500.00"
    And CGT event 1 should have sale proceeds "3000.00"
    And CGT event 1 should have capital gain "500.00"
    And CGT event 2 should have sell date "2025-11-01"
    And CGT event 2 should have buy date "2025-10-15"
    And CGT event 2 should have commodity "USDC"
    And CGT event 2 should have cost basis "3000.00"
    And CGT event 2 should have sale proceeds "3100.00"
    And CGT event 2 should have capital gain "100.00"

  Scenario: Selling more than held produces warning
    Given a transactions file named "fifo_sell_more.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 2 events
    And CGT event 1 should have quantity "5.00"
    And CGT event 1 should have cost basis "500.00"
    And CGT event 1 should have sale proceeds "500.00"
    And CGT event 1 should have capital gain "0.00"
    And CGT event 2 should have quantity "5.00"
    And CGT event 2 should have cost basis "0.00"
    And CGT event 2 should have sale proceeds "500.00"
    And CGT event 2 should have capital gain "500.00"
    And the CGT report should have warnings containing "Sold more than held"

  Scenario: Transfer between accounts is not a taxable event
    Given a transactions file named "fifo_transfer.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 1 event
    And CGT event 1 should have sell date "2025-08-10"
    And CGT event 1 should have commodity "ETH"
    And CGT event 1 should have quantity "3.00"
    And CGT event 1 should have cost basis "300.00"
    And CGT event 1 should have sale proceeds "600.00"
    And CGT event 1 should have capital gain "300.00"

  Scenario: Two-leg equity:trading swap derives proceeds from counterpart
    Given a transactions file named "fifo_equity_swap.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 1 event
    And CGT event 1 should have sell date "2025-08-10"
    And CGT event 1 should have commodity "ETH"
    And CGT event 1 should have quantity "10.00"
    And CGT event 1 should have cost basis "1000.00"
    And CGT event 1 should have sale proceeds "30000.00"
    And CGT event 1 should have capital gain "29000.00"

  Scenario: Two-leg swap with PriceGraph resolves proceeds via price data
    Given a transactions file named "fifo_equity_swap_prices.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    Given prices for "USDC" with base "AUD" at "2025-08-10" of "1.29"
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 1 event
    And CGT event 1 should have commodity "ETH"
    And CGT event 1 should have sale proceeds "38700.00"
    And CGT event 1 should have capital gain "37700.00"

  Scenario: CGT discount with FIFO across mixed holding periods
    Given a transactions file named "fifo_discount.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 2 events
    And CGT event 1 should have buy date "2024-01-15"
    And CGT event 1 should have quantity "5.00"
    And CGT event 1 should have cost basis "500.00"
    And CGT event 1 should have sale proceeds "1500.00"
    And CGT event 1 should have capital gain "1000.00"
    And CGT event 1 should be discount eligible
    And CGT event 1 should have discounted gain "500.00"
    And CGT event 2 should have buy date "2025-06-01"
    And CGT event 2 should have quantity "3.00"
    And CGT event 2 should have cost basis "600.00"
    And CGT event 2 should have sale proceeds "900.00"
    And CGT event 2 should have capital gain "300.00"
    And CGT event 2 should not be discount eligible
    And CGT event 2 should have discounted gain "300.00"

  Scenario: Auto-linked plain equity:trading swap resolves proceeds from counterpart
    Given a transactions file named "fifo_pooled_equity_swap.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    Given the transactions are auto-linked for equity swaps
    Given prices for "USDT" with base "AUD" at "2025-08-10" of "1.50"
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 1 event
    And CGT event 1 should have sell date "2025-08-10"
    And CGT event 1 should have commodity "SWELL"
    And CGT event 1 should have quantity "1000.00"
    And CGT event 1 should have cost basis "20.00"
    And CGT event 1 should have sale proceeds "42.00"
    And CGT event 1 should have capital gain "22.00"

  Scenario: Linked swap gives cost basis to unpriced token via counterpart
    Given a transactions file named "fifo_swap_unpriced.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    Given prices for "SOL" with base "AUD" at "2025-03-01" of "200.00"
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 1 event
    And CGT event 1 should have sell date "2025-09-15"
    And CGT event 1 should have commodity "SKBDI"
    And CGT event 1 should have quantity "1763.00"
    And CGT event 1 should have cost basis "1356.00"
    And CGT event 1 should have sale proceeds "750.00"
    And CGT event 1 should have capital gain "-606.00"

  Scenario: CGT uses price annotation on posting for cost basis and proceeds
    Given a transactions file named "fifo_swap_price_annotation.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 1 event
    And CGT event 1 should have commodity "SKBDI"
    And CGT event 1 should have cost basis "1357.51"
    And CGT event 1 should have sale proceeds "881.50"
    And CGT event 1 should have capital gain "-476.01"

  Scenario: Auto-linked multi-fill swap pairs correctly by amount
    Given a transactions file named "fifo_pooled_equity_multi_fill.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    Given the transactions are auto-linked for equity swaps
    Given prices for "USDT" with base "AUD" at "2025-08-10" of "1.50"
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 3 events
    And CGT event 1 should have commodity "SWELL"
    And CGT event 1 should have quantity "100.00"
    And CGT event 1 should have sale proceeds "4.20"
    And CGT event 2 should have quantity "400.00"
    And CGT event 2 should have sale proceeds "16.80"
    And CGT event 3 should have quantity "500.00"
    And CGT event 3 should have sale proceeds "21.00"

  Scenario: Same-leg-tagged multi-fill matches by rank, not by first sibling
    # Both sides of every fill got tagged equity:trading:sell (rule that
    # didn't differentiate sign). The same-leg fallback in
    # find_equity_swap_sibling must rank-pair by `assets:` posting amount
    # so each ETH disposal pairs with its OWN USD receipt — not all three
    # sharing the first 600 USD = 900 AUD value (which would understate
    # the larger fills and overstate the smaller).
    Given a transactions file named "fifo_same_leg_equity_multi_fill.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    Given prices for "USD" with base "AUD" at "2025-08-10" of "1.50"
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 3 events
    And CGT event 1 should have commodity "ETH"
    And CGT event 1 should have quantity "60.00"
    And CGT event 1 should have sale proceeds "900.00"
    And CGT event 2 should have quantity "30.00"
    And CGT event 2 should have sale proceeds "450.00"
    And CGT event 3 should have quantity "10.00"
    And CGT event 3 should have sale proceeds "150.00"

  Scenario: Tagged equity:trading multi-fill matches each sell to its correct buy
    Given a transactions file named "fifo_tagged_equity_multi_fill.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    Given prices for "USD" with base "AUD" at "2025-08-10" of "1.50"
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 3 events
    And CGT event 1 should have sell date "2025-08-10"
    And CGT event 1 should have commodity "ETH"
    And CGT event 1 should have quantity "60.00"
    And CGT event 1 should have cost basis "600.00"
    And CGT event 1 should have sale proceeds "900.00"
    And CGT event 1 should have capital gain "300.00"
    And CGT event 2 should have quantity "30.00"
    And CGT event 2 should have cost basis "300.00"
    And CGT event 2 should have sale proceeds "450.00"
    And CGT event 2 should have capital gain "150.00"
    And CGT event 3 should have quantity "10.00"
    And CGT event 3 should have cost basis "100.00"
    And CGT event 3 should have sale proceeds "150.00"
    And CGT event 3 should have capital gain "50.00"

  Scenario: Auto-linked swap price annotation survives disk round-trip
    Given a transactions file named "fifo_pooled_equity_swap.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    Given the transactions are auto-linked for equity swaps with prices for "USDT" at "1.50" in "AUD"
    Given the transactions are serialized to text and re-parsed
    Given prices for "USDT" with base "AUD" at "2025-08-10" of "1.50"
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 1 event
    And CGT event 1 should have commodity "SWELL"
    And CGT event 1 should have cost basis "20.00"
    And CGT event 1 should have sale proceeds "42.00"
    And CGT event 1 should have capital gain "22.00"

  Scenario: Trading fees on buy and sell flow through to cost basis and proceeds
    # Buy carries a 10 AUD fee folded into the cost annotation (1010 total cost),
    # sell carries a 5 AUD fee netted into the @@ proceeds annotation (1995).
    # Expected capital gain = 1995 - 1010 = 985 AUD.
    Given a transactions file named "cgt_fee_on_disposal.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 1 event
    And CGT event 1 should have commodity "ETH"
    And CGT event 1 should have cost basis "1010.00"
    And CGT event 1 should have sale proceeds "1995.00"
    And CGT event 1 should have capital gain "985.00"
    And CGT event 1 should be discount eligible
    And CGT event 1 should have discounted gain "492.50"

  Scenario: Sell with fee on income:trading:fees nets proceeds via price annotation
    # Hyperliquid convention: the sell fee is booked as a positive posting on
    # income:trading:fees rather than expenses:*. The @@ annotation on the
    # asset disposal pins CGT proceeds to the net figure (1992 AUD), so the
    # fee correctly reduces the capital gain (992, not 1000).
    Given a transactions file named "cgt_fee_income_offset.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 1 event
    And CGT event 1 should have commodity "ETH"
    And CGT event 1 should have cost basis "1000.00"
    And CGT event 1 should have sale proceeds "1992.00"
    And CGT event 1 should have capital gain "992.00"

  Scenario: CGT report uses a non-AUD base currency (USD) directly
    # Same shape as the simple buy/sell, but evaluated with base currency USD.
    # No price conversion required — all postings are already in USD.
    Given a transactions file named "cgt_non_aud_base.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    When I generate a CGT report for FY "2026" with base currency "USD"
    Then the CGT report should contain 1 event
    And CGT event 1 should have commodity "ETH"
    And CGT event 1 should have cost basis "1000.00"
    And CGT event 1 should have sale proceeds "1500.00"
    And CGT event 1 should have capital gain "500.00"
    And CGT event 1 should be discount eligible
    And CGT event 1 should have discounted gain "250.00"

  Scenario: CGT report with non-AUD base converts foreign-currency proceeds via PriceGraph
    # Base currency USD; the trade is denominated in EUR. PriceGraph supplies
    # EUR→USD conversions at the buy and sell dates. Cost basis = 900 * 1.10
    # = 990 USD; proceeds = 1100 * 1.20 = 1320 USD; capital gain = 330 USD.
    Given a transactions file named "cgt_non_aud_base_prices.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    Given prices for "EUR" with base "USD" at "2024-01-15" of "1.10"
    Given prices for "EUR" with base "USD" at "2025-08-20" of "1.20"
    When I generate a CGT report for FY "2026" with base currency "USD"
    Then the CGT report should contain 1 event
    And CGT event 1 should have commodity "ETH"
    And CGT event 1 should have cost basis "990.00"
    And CGT event 1 should have sale proceeds "1320.00"
    And CGT event 1 should have capital gain "330.00"

  Scenario: Rule-tagged multi-account swap prices each buy from its own account's sell
    Given a transactions file named "fifo_tagged_multi_account_fill.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    Given the transactions are auto-linked for equity swaps with prices for "USDT" at "1.50" in "AUD"
    Given prices for "USDT" with base "AUD" at "2025-08-10" of "1.50"
    When I generate a CGT report for FY "2026" with base currency "AUD"
    Then the CGT report should contain 2 events
    And CGT event 1 should have commodity "VIRTUAL"
    And CGT event 1 should have buy date "2025-01-15"
    And CGT event 1 should have quantity "5.00"
    And CGT event 1 should have cost basis "15.00"
    And CGT event 1 should have sale proceeds "15.00"
    And CGT event 1 should have capital gain "0.00"
    And CGT event 2 should have commodity "VIRTUAL"
    And CGT event 2 should have buy date "2025-01-16"
    And CGT event 2 should have quantity "30.00"
    And CGT event 2 should have cost basis "450.00"
    And CGT event 2 should have sale proceeds "450.00"
    And CGT event 2 should have capital gain "0.00"
