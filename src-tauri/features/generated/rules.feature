Feature: Transaction categorization rules

  Scenario: Rule sets payee based on narration match
    Given a clean sources directory with a CSV and transform
    And a rules file at "bank/_rules.json" matching "imported" with payee "Test Merchant"
    When I run the pipeline
    Then transactions with narration "imported" should have payee "Test Merchant"

  Scenario: Rule sets contra account
    Given a clean sources directory with a CSV and transform
    And a rules file at "bank/_rules.json" matching "imported" with contra "expenses:groceries"
    When I run the pipeline
    Then transactions with narration "imported" should use contra "expenses:groceries"

  Scenario: Wildcard rule matches prefix
    Given a clean sources directory with a CSV and transform
    And a rules file at "bank/_rules.json" matching "import*" with payee "Wildcard Match"
    When I run the pipeline
    Then transactions with narration "imported" should have payee "Wildcard Match"

  Scenario: Rules invalidate build cache
    Given a pipeline has been run once with a transform
    When a rules file is added to "bank/_rules.json" matching "CacheTest" with payee "Ruled"
    And I run the pipeline again
    Then the CSV should be re-transformed (not cached)

  Scenario: Transform suggestion maps Description to narration not payee
    Given a CSV with headers "Date,Description,Amount"
    And the target account is "assets:bank:test"
    When I generate a transform suggestion
    Then the suggestion should map "narration" from "Description"
    And the suggestion should have blank payee
