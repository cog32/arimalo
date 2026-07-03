Feature: Trade link rule generation

  Trade links should generate field-specific rules in _rules.json so that
  contra account overrides persist through pipeline regeneration.

  Scenario: Field-specific rule matches only on the specified field
    Given a clean sources directory with a CSV and transform
    And a rules file at "bank/_rules.json" with a field-specific rule matching "txn:csv-*" on field "meta" with contra "equity:trading:sell"
    When I run the pipeline
    Then the active ledger should contain "equity:trading:sell"

  Scenario: Field-specific rules take priority over general rules
    Given a clean sources directory with a CSV and transform
    And a rules file at "bank/_rules.json" matching "imported" with contra "expenses:food"
    And a rules file at "bank/_rules.json" with a field-specific rule matching "txn:csv-*" on field "meta" with contra "equity:trading:sell"
    When I run the pipeline
    Then the active ledger should contain "equity:trading:sell"

  Scenario: General rule still applies when no field-specific rule matches
    Given a clean sources directory with a CSV and transform
    And a rules file at "bank/_rules.json" matching "imported" with contra "expenses:food"
    When I run the pipeline
    Then the active ledger should contain "expenses:food"

  Scenario: Saving a trade link generates rules in _rules.json
    Given a clean sources directory with exchange transactions
    When I run the pipeline
    And I save a trade link with rules for account folder "exchange"
    Then the rules file "exchange/_rules.json" should contain 2 rules
    And rule 0 in "exchange/_rules.json" should have contra "equity:trading:sell"
    And rule 0 in "exchange/_rules.json" should have a match_field of "meta"
    And rule 1 in "exchange/_rules.json" should have contra "equity:trading:buy"
    And rule 1 in "exchange/_rules.json" should have a match_field of "meta"

  Scenario: Deleting a trade link removes rules from _rules.json
    Given a clean sources directory with exchange transactions
    When I run the pipeline
    And I save a trade link with rules for account folder "exchange"
    And I delete the trade link with rules for account folder "exchange"
    Then the rules file "exchange/_rules.json" should contain 0 rules

  Scenario: Trade link rules survive pipeline rebuild
    Given a clean sources directory with exchange transactions
    When I run the pipeline
    And I save a trade link with rules for account folder "exchange"
    And I run the pipeline
    Then the active ledger should contain "equity:trading:sell"
    And the active ledger should contain "equity:trading:buy"

  # On a shared-txn swap the two legs each carry their own leg: id, so the
  # trade-link rules anchor on those leg ids (not the shared txn id). That keeps
  # them at the top precedence tier — above any prior per-leg categorisation —
  # so the trade actually wins on rebuild.
  Scenario: Saving a trade link for a shared-txn-id swap generates leg-anchored rules
    Given a clean sources directory with a shared-txn-id swap
    When I run the pipeline
    And I save a trade link with rules for account folder "exchange"
    Then the rules file "exchange/_rules.json" should contain 2 rules
    And rule 0 in "exchange/_rules.json" should have contra "equity:trading:sell"
    And rule 0 in "exchange/_rules.json" should have a match_field of "meta"
    And rule 0 in "exchange/_rules.json" should have a pattern starting with "leg:"
    And rule 1 in "exchange/_rules.json" should have contra "equity:trading:buy"
    And rule 1 in "exchange/_rules.json" should have a match_field of "meta"
    And rule 1 in "exchange/_rules.json" should have a pattern starting with "leg:"

  Scenario: Shared-txn-id trade link rewrites both legs to sell and buy contras
    Given a clean sources directory with a shared-txn-id swap
    When I run the pipeline
    And I save a trade link with rules for account folder "exchange"
    And I run the pipeline
    Then the active ledger should contain "equity:trading:sell"
    And the active ledger should contain "equity:trading:buy"

  Scenario: A shared-txn-id swap stamps a distinct leg id per leg
    Given a clean sources directory with a shared-txn-id swap
    When I run the pipeline
    Then the active ledger should contain 2 distinct leg ids
