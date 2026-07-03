Feature: Hidden accounts filtering

  Scenario: Transactions posting to configured hidden accounts are filtered out
    Given a clean sources directory with a CSV and transform
    And a rules file at "bank/_rules.json" matching "imported" with contra "ignore:spam"
    When I run the pipeline
    Then transactions with narration "imported" should use contra "ignore:spam"
    When I filter hidden accounts with prefix "ignore:"
    Then the filtered result should have 0 transactions

  Scenario: Filtering preserves transactions not posting to hidden accounts
    Given a clean sources directory with a CSV and transform
    And a "manual.transactions" file with payee "KeepMe"
    And a rules file at "bank/_rules.json" matching "imported" with contra "ignore:spam"
    When I run the pipeline
    When I filter hidden accounts with prefix "ignore:"
    Then the filtered result should have 1 transactions
    And the filtered result should include payee "KeepMe"
    And the filtered result should not include account "ignore:spam"

  Scenario: No hidden accounts configured means no filtering
    Given a clean sources directory with a CSV and transform
    And a rules file at "bank/_rules.json" matching "imported" with contra "ignore:spam"
    When I run the pipeline
    When I filter hidden accounts with no prefixes
    Then the filtered result should have 1 transactions
