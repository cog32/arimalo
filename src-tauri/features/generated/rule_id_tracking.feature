Feature: Track which rule matched each transaction

  When a rule matches a transaction during pipeline processing, the rule ID
  should be stored in the transaction metadata so the frontend can link back
  to the rule for editing.

  Scenario: Matched rule ID is stored in transaction meta
    Given a clean sources directory with a CSV and transform
    And a rules file at "bank/_rules.json" matching "imported" with contra "expenses:groceries"
    When I run the pipeline
    Then transactions with narration "imported" should have meta containing "rule:"
