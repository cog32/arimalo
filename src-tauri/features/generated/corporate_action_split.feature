Feature: Corporate action stock split via contra-equity pattern

  Broker exports (e.g. CommSec's ConfirmationDetails.csv) often omit
  corporate actions such as stock splits. To prevent the commodity balance
  going negative, the user adds a manual 2-leg ledger entry with the
  positive units on the asset account and the offsetting negative leg on
  the equity:corporate-actions:split contra account.

  The engine has no first-class split primitive, so the split-added units
  flow through FIFO as a zero-cost lot. This is a documented limitation
  of the contra-equity pattern (split does not re-allocate basis across
  pre-split lots).

  Background:
    Given a transactions file named "corporate_action_split.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed

  Scenario: Split entry parses, balances, and increases the asset holding
    # Pre-split: bought 10 ETHTEST for 1000 AUD on 2023-07-01.
    # 10:1 split on 2023-12-15 adds +90 ETHTEST via the equity contra.
    # Post-split: sold 100 ETHTEST for 500 AUD on 2024-09-01.
    # Net asset balance is 0 (fully sold); contra balance shows the -90.
    Then the balance for account "assets:exchange:test" should be "0" "ETHTEST"
    And the balance for account "equity:corporate-actions:split" should be "-90" "ETHTEST"
    And the balance for account "equity:trading:buy" should be "-1000.00" "AUD"
    And the balance for account "equity:trading:sell" should be "500.00" "AUD"

  Scenario: Split itself does not produce a CGT disposal event
    # FY 2024 (1 Jul 2023 - 30 Jun 2024) contains the buy and the split.
    # The split has no negative assets:* leg, so the engine must not
    # treat it as a disposal.
    When I generate a CGT report for FY "2024" with base currency "AUD"
    Then the CGT report should contain 0 events

  Scenario: Post-split sale consumes split-added units as a zero-cost FIFO lot
    # FY 2025 (1 Jul 2024 - 30 Jun 2025) contains the sale of all 100 units
    # for 500 AUD. FIFO consumes the 10-unit lot at 100 AUD/unit first
    # (cost 1000.00, proceeds 50.00 pro-rata), then the 90-unit split lot
    # at zero cost (proceeds 450.00 pro-rata). Documents the limitation:
    # the split lot has zero cost basis.
    When I generate a CGT report for FY "2025" with base currency "AUD"
    Then the CGT report should contain 2 events
    And CGT event 1 should have commodity "ETHTEST"
    And CGT event 1 should have quantity "10.00"
    And CGT event 1 should have cost basis "1000.00"
    And CGT event 1 should have sale proceeds "50.00"
    And CGT event 1 should have capital gain "-950.00"
    And CGT event 2 should have commodity "ETHTEST"
    And CGT event 2 should have quantity "90.00"
    And CGT event 2 should have cost basis "0.00"
    And CGT event 2 should have sale proceeds "450.00"
    And CGT event 2 should have capital gain "450.00"
