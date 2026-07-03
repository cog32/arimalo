Feature: Payee labels

  Labels in _labels.json are pre-pass transforms that rename payees
  and commodities before categorization rules are applied.
  Clicking a payee in the UI should save a label, not a rule.

  Scenario: Label renames payee via _labels.json
    Given a clean sources directory with a CSV "bank/2025-01.csv":
      | Date       | Description     | Amount |
      | 2025-01-15 | Coffee purchase | -4.50  |
    And a transform at "bank/_transform.rhai" that maps Date/Description/Amount to AUD
    And a labels file at "bank/_labels.json" matching "*Coffee*" with payee "Coffee Shop"
    When I run the pipeline
    Then transactions with narration "imported" should have payee "Coffee Shop"
