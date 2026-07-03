Feature: Processing pipeline

  Scenario: Simple CSV is transformed into ledger transactions
    Given a clean sources directory with a CSV "commbank/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
      | 2025-01-16 | Salary      | 3500   |
    And a transform at "commbank/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    When I run the pipeline for month "202501"
    Then the active ledger should contain 2 transactions
    And the active ledger should include payee "Coffee Shop"
    And the active ledger should include payee "Salary"

  Scenario: Transform hierarchy — child folder transform overrides parent
    Given a clean sources directory
    And an institution transform at "commbank/_transform.rhai" using account "assets:bank:commbank"
    And an account transform at "commbank/credit/_transform.rhai" using account "liabilities:credit:commbank"
    And a CSV "commbank/credit/2025-01.csv" with one row
    When I run the pipeline
    Then the transaction should use account "assets:credit"

  Scenario: Manual transactions are merged with CSV transactions
    Given a clean sources directory with a CSV and transform
    And a "manual.transactions" file with payee "ManualEntry"
    When I run the pipeline
    Then the active ledger should include payee from CSV
    And the active ledger should include payee "ManualEntry"

  Scenario: Build cache skips unchanged CSVs
    Given a clean sources directory with a CSV and transform
    When I run the pipeline twice
    Then the second run should report 0 CSVs transformed and all cached

  Scenario: Changing a transform invalidates cache
    Given a pipeline has been run once with a transform
    When the transform is modified
    And I run the pipeline again
    Then the CSV should be re-transformed (not cached)

  Scenario: Stable transaction IDs across rebuilds
    Given a clean sources directory with a CSV and transform
    When I run the pipeline twice
    Then all transaction txn: IDs should be identical between runs

  Scenario: Per-folder summary.json is byte-identical across reruns
    Given a clean sources directory with a CSV "richard/savings/2025-01.csv":
      | Date       | Description | Amount  |
      | 2025-01-15 | Coffee Shop |   -4.50 |
      | 2025-01-16 | Salary      | 3500.00 |
      | 2025-01-17 | Refund      |   12.34 |
      | 2025-01-18 | Groceries   |  -82.10 |
    And a transform at "richard/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    When I run the pipeline twice
    Then the per-folder summary.json should be byte-identical between runs

  Scenario: Deterministic output ordering
    Given CSVs from two sources with interleaved dates
    When I run the pipeline
    Then transactions should be sorted by date, then by source path

  Scenario: Monthly rotation of output
    Given CSVs with transactions in January and February
    When I run the pipeline for month "202502"
    Then January transactions should be in archive/ledger-202501.transactions
    And February transactions should be in ledger.transactions

  Scenario: Loading ledger includes archived transactions
    Given CSVs with transactions in January and February
    When I run the pipeline for month "202502"
    Then loading all ledgers should include payee "January Item"
    And loading all ledgers should include payee "February Item"

  Scenario: Account declarations are written to output
    Given an "accounts.transactions" file declaring "assets:bank:commbank AUD"
    When I run the pipeline
    Then the output should include the account declaration

  Scenario: Import CSV copies file to account folder and rebuilds
    Given a clean sources directory with a transform at "bank/commbank/_transform.rhai"
    And a CSV fixture file "sample.csv"
    When I import the CSV to account folder "bank/commbank"
    Then the file should exist in "bank/commbank/sample.csv"
    And the pipeline should have transformed 1 CSV

  Scenario: Pipeline early-exits when nothing changed
    Given a clean sources directory with a CSV "commbank/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "commbank/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    When I run the pipeline for month "202501"
    And I run the pipeline again
    Then the pipeline should have early-exited

  Scenario: Frontend rebuild_pipeline command respects cache
    Given a clean sources directory with a CSV "commbank/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "commbank/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    When I run the pipeline for month "202501"
    And I run the pipeline again as the frontend would
    Then the pipeline should have early-exited

  Scenario: Add account creates nested folder and appears in owner_accounts
    Given a clean sources directory with a CSV "richard/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "richard/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    When I add account "assets:crypto:wallet:ethereum" with currency "ETH" to account set "richard"
    Then the folder "richard/crypto/wallet/ethereum/imports" should exist under sources
    And owner_accounts for "richard" should include "assets:crypto:wallet:ethereum"
    And the ledger for set "richard" should include a balance for "assets:crypto:wallet:ethereum"

  Scenario: Three-level empty folder appears in owner_accounts
    Given a clean sources directory with a CSV "richard/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "richard/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    And an empty folder "richard/crypto/wallet/solana/2baaTDzWallet" under sources
    When I run the pipeline for month "202501"
    Then owner_accounts for "richard" should include "assets:crypto:wallet:solana:2baaTDzWallet"

  Scenario: Pipeline detects source changes on restart
    Given a clean sources directory with a CSV "richard/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "richard/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    When I run the pipeline
    Then the pipeline should have early-exited is false
    When I run the pipeline again
    Then the pipeline should have early-exited
    When a "manual.transactions" file is added to "richard/savings" with payee "OfflineAdd"
    And I run the pipeline again
    Then the pipeline should have early-exited is false
    And the active ledger should include payee "OfflineAdd"

  Scenario: Manual transaction is saved to the specified account folder
    Given a clean sources directory with a CSV "richard/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "richard/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    When I append a manual transaction with payee "BuyETH" to account folder "richard/ethereum"
    Then a "manual.transactions" file should exist in "richard/ethereum"
    And no "manual.transactions" file should exist at the sources root
    When I run the pipeline
    Then the active ledger should include payee "BuyETH"

  Scenario: Manual transactions get stable txn IDs for trade linking
    Given a clean sources directory with a CSV "richard/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "richard/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    When I append a manual transaction with payee "BuyETH" to account folder "richard/ethereum"
    And I run the pipeline
    Then the active ledger should contain "txn:man-"
    When I run the pipeline again
    Then the active ledger should contain "txn:man-"

  Scenario: Pipeline does not create stray top-level folders for nested accounts
    Given a clean sources directory with a CSV "richard/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "richard/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    When I append a manual transaction with payee "BuyETH" to account folder "richard/ethereum"
    And I run the pipeline
    Then the only top-level source folders should be "richard"

  Scenario: Second pipeline run skips unchanged output files
    Given a clean sources directory with a CSV "richard/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "richard/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    When I run the pipeline for month "202502"
    And the sources directory is touched to bypass global cache
    And I run the pipeline again
    Then the pipeline should report 0 output files written
    And the pipeline should report output files skipped > 0

  Scenario: Changing one CSV only rewrites affected output files
    Given a clean sources directory with a CSV "richard/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "richard/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    And a CSV "richard/checking/2025-02.csv":
      | Date       | Description | Amount |
      | 2025-02-15 | Rent        | -1200  |
    And a transform at "richard/checking/_transform.rhai" that maps Date/Description/Amount to AUD
    When I run the pipeline for month "202503"
    Then the pipeline should report output files written > 0
    When a CSV "richard/savings/2025-01.csv" is modified:
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -5.00  |
    And I run the pipeline again
    Then the pipeline should report output files written > 0
    And the pipeline should report output files skipped > 0

  Scenario: Stale output files are pruned when source folder is removed
    Given a clean sources directory with a CSV "richard/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "richard/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    And a CSV "richard/checking/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-20 | Groceries   | -50    |
    And a transform at "richard/checking/_transform.rhai" that maps Date/Description/Amount to AUD
    When I run the pipeline for month "202502"
    Then the active ledger should include payee "Groceries"
    When the source folder "richard/checking" is removed
    And I run the pipeline again
    Then the active ledger should not include payee "Groceries"

  Scenario: Manual-only folder is registered in account_folders and ledger
    Given a clean sources directory
    When a "manual.transactions" file is added to "richard/crypto/wallet/generic" with payee "Osmosis DEX"
    And I run the pipeline
    Then owner_accounts for "richard" should include "assets:crypto:wallet:generic"
    And account_folders should map "assets:crypto:wallet:generic" to "richard/crypto/wallet/generic"
    And the active ledger should include payee "Osmosis DEX"

  Scenario: Unchanged manual.transactions uses cache
    Given a clean sources directory with a CSV "richard/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "richard/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    When a "manual.transactions" file is added to "richard/savings" with payee "ManualEntry"
    And I run the pipeline twice
    Then the second run should report 0 CSVs transformed and all cached
    And the active ledger should include payee "ManualEntry"

  Scenario: Shared txn_id rows are not cloned by equity swap propagation
    Given a clean sources directory with a CSV "richard/eth/txns.csv":
      | Date       | Description | Amount | TxHash  |
      | 2025-01-10 | PoolRouter  | -5.00  | 0xabc01 |
      | 2025-01-10 | DEXPool     | -80.00 | 0xabc01 |
      | 2025-01-10 | PoolRouter  | 3.00   | 0xabc01 |
    And a txn_id-aware transform at "richard/eth/_transform.rhai"
    And a rules file at "richard/eth/_rules.json" matching "*DEXPool*" with account "equity:trading:sell"
    When I run the pipeline
    Then the folder ledger "eth" should contain 3 transactions
    And the folder ledger "eth" should contain payee "PoolRouter"
    And the folder ledger "eth" should contain payee "DEXPool"
