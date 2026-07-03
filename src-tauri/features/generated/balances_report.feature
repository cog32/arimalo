Feature: Balances report

  Point-in-time portfolio snapshot as of a given date, with per-commodity
  quantities aggregated from postings and valued in a base currency via the
  price graph. Commodities without a resolvable price produce a warning and
  are excluded from holdings.

  Background:
    Given a transactions file named "balances_portfolio.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    Given a prices file named "balances_portfolio.prices"
    When I run the prices parser on that file
    Then the prices parse should succeed

  Scenario: Holdings aggregated under the scoped account at the as-of date
    When I generate a balances report as of "2026-06-30" in "AUD" scoped to "assets:crypto"
    Then the balances report should have 2 holdings
    And balances holding "BTC" should have quantity "0.5"
    And balances holding "ETH" should have quantity "2.0"

  Scenario: Holdings valued in AUD via the price graph
    When I generate a balances report as of "2026-06-30" in "AUD" scoped to "assets:crypto"
    Then balances holding "BTC" should have value "25000.00"
    And balances holding "ETH" should have value "9000.00"
    And the balances report total value should be "34000.00"

  Scenario: Transactions after the as-of date are excluded
    # The 2026-07-15 buy of 0.1 BTC sits after the 2026-06-30 snapshot, so BTC
    # must remain at 0.5, not 0.6.
    When I generate a balances report as of "2026-06-30" in "AUD" scoped to "assets:crypto"
    Then balances holding "BTC" should have quantity "0.5"

  Scenario: Commodities without a resolvable price are warned, not shown
    When I generate a balances report as of "2026-06-30" in "AUD" scoped to "assets:crypto"
    Then the balances report should not contain "SPAM"
    And the balances report warnings should mention "SPAM"

  Scenario: Scope filter narrows to postings beneath the prefix
    # Without the scope, fiat:bank AUD postings would also contribute; with
    # scope "assets:crypto" only BTC + ETH (valued) survive — SPAM is warned.
    When I generate a balances report as of "2026-06-30" in "AUD" scoped to "assets:crypto"
    Then the balances report should have 2 holdings

  Scenario: Holdings expose a per-leaf-account breakdown
    # BTC sits across binance (0.30) + ledger (0.20) after the cold-wallet move.
    # ETH lives entirely on binance.
    When I generate a balances report as of "2026-06-30" in "AUD" scoped to "assets:crypto"
    Then balances holding "BTC" should have 2 account breakdown rows
    And balances holding "BTC" account "assets:crypto:binance:btc" should have quantity "0.30"
    And balances holding "BTC" account "assets:crypto:ledger:btc" should have quantity "0.20"
    And balances holding "BTC" account quantities should sum to its total
    And balances holding "ETH" should have 1 account breakdown rows

  Scenario: Transactions touching ignore:* are dropped entirely
    # The 2026-02-01 SCAM receipt offsets to ignore:spam. The whole txn must
    # be skipped — no SCAM in holdings AND no missing-price warning for SCAM.
    When I generate a balances report as of "2026-06-30" in "AUD" scoped to "assets:crypto"
    Then the balances report should not contain "SCAM"
    And the balances report warnings should not mention "SCAM"

  Scenario: Allowlist restricts holdings to primary source-folder accounts
    # The contras fixture lays 1000 USDC on a primary (binance:usdc) and
    # leaves +500 USDC stranded on a non-primary contra
    # (assets:crypto:transfer). With scope alone, the parent USDC quantity
    # collapses to 500 because the contra is summed in. With an allowlist of
    # primary accounts, the contra is excluded — the parent quantity is the
    # 1000 USDC actually held on the primary, and the breakdown contains
    # only that account.
    Given a transactions file named "balances_with_contras.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    Given a prices file named "balances_with_contras.prices"
    When I run the prices parser on that file
    Then the prices parse should succeed
    When I generate a balances report as of "2026-06-30" in "AUD" scoped to "assets:crypto" restricted to accounts "assets:crypto:binance:btc, assets:crypto:binance:eth, assets:crypto:binance:usdc"
    Then balances holding "USDC" should have quantity "1000"
    And balances holding "USDC" should have 1 account breakdown rows
    And balances holding "USDC" account "assets:crypto:binance:usdc" should have quantity "1000"
    And balances holding "USDC" account quantities should sum to its total
    And the balances report total value should be "35500.00"

  Scenario: Without allowlist, contra postings inflate the parent total
    # Documents the un-filtered behavior: with only scope (no allowlist),
    # postings on assets:crypto:transfer are counted toward USDC. The
    # primary holds 1000 USDC; the contra holds +500 (1000 inbound minus
    # the offset, plus the stranded 500); the parent USDC quantity is the
    # net 500. This is the bug the allowlist fixes.
    Given a transactions file named "balances_with_contras.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    Given a prices file named "balances_with_contras.prices"
    When I run the prices parser on that file
    Then the prices parse should succeed
    When I generate a balances report as of "2026-06-30" in "AUD" scoped to "assets:crypto"
    Then balances holding "USDC" should have quantity "500"
