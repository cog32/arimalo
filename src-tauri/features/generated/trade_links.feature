Feature: Trade linking

  Scenario: Save and retrieve a trade link
    Given a trade link store
    When I save a trade link between "txn:sell-eth" and "txn:buy-usdc"
    Then get_trade_links should return 1 link
    And the trade link should pair "txn:sell-eth" with "txn:buy-usdc"

  Scenario: Deterministic trade link ID
    Given a trade link store
    When I save a trade link between "txn:aaa" and "txn:bbb"
    And I save a trade link between "txn:bbb" and "txn:aaa"
    Then get_trade_links should return 1 link

  Scenario: Delete a trade link
    Given a trade link store
    When I save a trade link between "txn:sell-eth" and "txn:buy-usdc"
    And I delete that trade link
    Then get_trade_links should return 0 links

  Scenario: Suggest trade links from exchange transactions
    Given a clean sources directory with exchange transactions
    When I run the pipeline
    And I request trade link suggestions
    Then I should receive 1 trade suggestion
    And the suggestion should pair the ETH sell with the USDC buy

  Scenario: Already-linked transactions are not re-suggested
    Given a clean sources directory with exchange transactions
    When I run the pipeline
    And I save a trade link between the two exchange transactions
    And I request trade link suggestions
    Then I should receive 0 trade suggestions

  Scenario: Zero-amount transactions are never suggested as trades
    Given a clean sources directory with exchange transactions including zero amounts
    When I run the pipeline
    And I request trade link suggestions
    Then I should receive 0 trade suggestions

  Scenario: Auto-linked swap adds price annotation from denominator side
    Given a transactions file named "fifo_pooled_equity_swap.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    Given the transactions are auto-linked for equity swaps with prices for "USDT" at "1.50" in "AUD"
    Then the SWELL posting should have a price annotation

  Scenario: Pre-linked swap (with :sell/:buy) adds price from counterpart
    Given a transactions file named "fifo_swap_prelinked.transactions"
    When I run the ledger parser on that file
    Then the parse should succeed
    Given the transactions are auto-linked for equity swaps with prices for "SOL" at "200.00" in "AUD"
    Then the SKBDI posting should have a price annotation

  Scenario: Same-txn-ID trade link is stored as a single link
    Given a trade link store
    When I save a trade link between "txn:same-hash" and "txn:same-hash"
    Then get_trade_links should return 1 link

  Scenario: Dust-value transactions are not suggested when price data available
    Given a clean sources directory with dust-value exchange transactions
    And price data valuing ETH at 3000 USD and USDC at 1 USD
    When I run the pipeline
    And I request trade link suggestions with base currency "USD"
    Then I should receive 0 trade suggestions

  Scenario: Suggest trade links for multi-fill exchange trades
    Given a clean sources directory with multi-fill exchange transactions
    When I run the pipeline
    And I request trade link suggestions
    Then I should receive 3 trade suggestions
