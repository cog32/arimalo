Feature: Incremental pipeline — only regenerate what changed

  The pipeline behaves like a compiler: each source folder produces its
  own generated output file. When a source folder changes, only that
  folder's output is regenerated. Everything else is untouched.

  Background:
    Given a clean sources directory

  Scenario: Only the changed folder is regenerated
    Given a CSV "bank/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "bank/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    And a CSV "bank/checking/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-20 | Salary      | 3000   |
    And a transform at "bank/checking/_transform.rhai" that maps Date/Description/Amount to AUD
    When I run the pipeline
    Then a ledger file should exist at "bank/savings/ledger.transactions"
    And a ledger file should exist at "bank/checking/ledger.transactions"
    When a CSV "bank/savings/2025-01.csv" is modified:
      | Date       | Description   | Amount |
      | 2025-01-15 | Coffee Shop   | -4.50  |
      | 2025-01-16 | Grocery Store | -25.00 |
    And I run the pipeline with changed folder hint "bank/savings"
    Then only the "bank/savings" folder should have been reprocessed
    And the "bank/checking" ledger file should be unchanged

  Scenario: Unchanged folders are read from their generated files, not re-parsed
    Given a CSV "bank/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "bank/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    When I run the pipeline
    And the sources directory is touched to bypass global cache
    And I run the pipeline again
    Then the second run should report 0 CSVs transformed and all cached

  Scenario: Rule change in a folder only regenerates that folder
    Given a CSV "bank/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "bank/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    When I run the pipeline
    And I add a rule to "bank/savings" matching "Coffee" with payee "Local Cafe"
    And I run the pipeline with changed folder hint "bank/savings"
    Then the ledger at "bank/savings/ledger.transactions" should contain "Local Cafe"

  Scenario: Hinted run does not flag unchanged sibling folders with pre-existing price annotations
    # Two sibling wallets; auto_link prices both during the baseline run.
    Given USD price data for "SOL" and "ETH"
    And a swap CSV at "richard/crypto/wallet/alpha/2024-05.csv" trading "USDC" for "SOL"
    And a swap CSV at "richard/crypto/wallet/beta/2024-05.csv" trading "USDC" for "ETH"
    When I run the pipeline
    And I add a rule to "richard/crypto/wallet/alpha" matching "Buy SOL" with payee "Alpha Trade"
    And I run the pipeline with changed folder hint "richard/crypto/wallet/alpha"
    Then changed_folders should contain "richard/crypto/wallet/alpha"
    And changed_folders should not contain "richard/crypto/wallet/beta"
    And the "richard/crypto/wallet/beta" ledger file should be unchanged

  Scenario: Parent-folder hint applies rules to child leaf folders
    # Rules live in the parent folder; the hint must reach the leaves.
    Given a swap CSV at "richard/crypto/wallet/alpha/2024-05.csv" trading "USDC" for "SOL"
    When I run the pipeline
    And I add a rule to "richard/crypto/wallet" matching "Buy SOL" with payee "Parent Rule"
    And I run the pipeline with changed folder hint "richard/crypto/wallet"
    Then the ledger at "richard/crypto/wallet/alpha/ledger.transactions" should contain "Parent Rule"
