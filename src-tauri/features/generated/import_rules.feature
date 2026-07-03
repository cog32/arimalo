Feature: Import rules from CSV

  Scenario: Import CSV adds rules to _rules.json and triggers pipeline rebuild
    Given a clean sources directory with a CSV and transform
    And a CSV rules file with contents:
      | pattern    | payee          | contra             | comment       |
      | imported   | Test Merchant  | expenses:groceries | grocery rule  |
      | *receipt*  | Receipt Store  |                    |               |
    When I import the rules CSV into "bank"
    Then the rules file "bank/_rules.json" should contain 2 rules
    And the pipeline should have been rebuilt

  Scenario: Empty payee/contra/comment cells are treated as None
    Given a clean sources directory with a CSV and transform
    And a CSV rules file with contents:
      | pattern  | payee         | contra           | comment      |
      | donated  |               | expenses:charity |              |
      | *tip*    | Tip Recipient |                  | tipping rule |
    When I import the rules CSV into "bank"
    Then the rules file "bank/_rules.json" should contain 2 rules
    And rule 0 in "bank/_rules.json" should have no payee
    And rule 0 in "bank/_rules.json" should have contra "expenses:charity"
    And rule 0 in "bank/_rules.json" should have no comment
    And rule 1 in "bank/_rules.json" should have payee "Tip Recipient"
    And rule 1 in "bank/_rules.json" should have no contra
    And rule 1 in "bank/_rules.json" should have comment "tipping rule"

  Scenario: Import into account with existing rules appends
    Given a clean sources directory with a CSV and transform
    And a rules file at "bank/_rules.json" matching "existing" with payee "OldPayee"
    And a CSV rules file with contents:
      | pattern  | payee    | contra | comment |
      | newstuff | NewPayee |        |         |
    When I import the rules CSV into "bank"
    Then the rules file "bank/_rules.json" should contain 2 rules

  Scenario: Import invalidates build cache
    Given a pipeline has been run once with a transform
    And a CSV rules file with contents:
      | pattern   | payee       | contra | comment |
      | CacheTest | CachePayee  |        |         |
    When I import the rules CSV into "bank"
    Then the CSV should be re-transformed (not cached)
