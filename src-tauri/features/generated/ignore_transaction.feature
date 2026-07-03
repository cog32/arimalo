Feature: Hide and delete transactions

  Scenario: Ignored CSV transaction is excluded from pipeline output
    Given a clean sources directory with a CSV and transform
    And a "manual.transactions" file with payee "ShouldStay"
    When I run the pipeline
    Then the active ledger should include payee from CSV
    And the active ledger should include payee "ShouldStay"
    When I hide the CSV transaction
    And I run the pipeline
    Then the active ledger should not include payee from CSV
    And the active ledger should include payee "ShouldStay"

  Scenario: Per-folder ledger file no longer contains a hidden CSV transaction
    Given a clean sources directory with a CSV and transform
    When I run the pipeline
    And I hide the CSV transaction
    And I run the pipeline
    Then the per-folder ledger at "bank" should not contain payee "CSV Entry"

  Scenario: Hiding the same CSV transaction twice does not duplicate the ignored entry
    Given a clean sources directory with a CSV and transform
    When I run the pipeline
    And I hide the CSV transaction
    And I hide the CSV transaction
    Then the ignored file should have 1 entry

  Scenario: Hiding a transaction cleans up pre-existing duplicates in the ignored file
    Given a clean sources directory with a CSV and transform
    And the ignored file already contains "txn:dup-a" twice and "txn:dup-b" once
    When I run the pipeline
    And I hide the CSV transaction
    Then the ignored file should have 3 entry

  Scenario: Deleted manual transaction is excluded from pipeline output
    Given a clean sources directory with a CSV and transform
    And a "manual.transactions" file with payee "ManualDelete"
    When I run the pipeline
    Then the active ledger should include payee "ManualDelete"
    When I delete the manual transaction with payee "ManualDelete"
    And I run the pipeline
    Then the active ledger should not include payee "ManualDelete"
    And the active ledger should include payee from CSV
