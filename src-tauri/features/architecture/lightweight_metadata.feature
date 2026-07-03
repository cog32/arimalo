Feature: Lightweight metadata changes — no full reload

  Linking transactions, adding labels, or modifying rules that affect a
  single folder should only update that folder's generated file. The
  change writes metadata (e.g. swap:txn:xxx, equity:trading:sell) to
  the affected transactions and regenerates that one folder. No other
  folders are touched, and the UI does not reload the full dataset.

  Background:
    Given a clean sources directory

  Scenario: Trade link only regenerates the affected folder
    Given a CSV "richard/exchange/binance/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | BUY ETH     | -100   |
      | 2025-01-15 | SELL BTC    | 100    |
    And a transform at "richard/exchange/binance/_transform.rhai" that maps Date/Description/Amount to AUD
    And a CSV "richard/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-20 | Salary      | 3000   |
    And a transform at "richard/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    When I run the pipeline
    And I link the two transactions in "richard/exchange/binance" as a trade
    Then only the "richard/exchange/binance" folder should have been reprocessed
    And the "richard/savings" ledger file should be unchanged
    And the pipeline should complete in under 1 second

  Scenario: Adding a rule to one folder does not touch other folders
    Given a CSV "bank/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "bank/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    And a CSV "bank/checking/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-20 | Salary      | 3000   |
    And a transform at "bank/checking/_transform.rhai" that maps Date/Description/Amount to AUD
    When I run the pipeline
    And I add a rule to "bank/savings" matching "Coffee" with payee "Local Cafe"
    And I run the pipeline with changed folder hint "bank/savings"
    Then only the "bank/savings" folder should have been reprocessed
    And the "bank/checking" ledger file should be unchanged
