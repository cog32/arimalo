Feature: Generated ledger ingestion (superseded by CSV pipeline)
  # These scenarios test legacy behaviour.
  # New pipeline scenarios are in processing_pipeline.feature.

  Scenario: Pipeline builds ledger from CSV sources
    Given a clean sources directory with a CSV and transform
    When I run the pipeline
    Then the active ledger should include payee "CSV Entry"

  Scenario: Manual transaction is merged via pipeline
    Given a clean sources directory with a CSV and transform
    And a "manual.transactions" file with payee "Manual"
    When I run the pipeline
    Then the active ledger should include payee "Manual"
    And the active ledger should include meta tag "txn:"
