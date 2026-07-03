Feature: Label persists when rule targets ignore account

  A label in _labels.json renames a payee address.  When a separate
  categorisation rule in _rules.json matches on the renamed payee
  (via display_payee) and sets amount_account to ignore:spam, the
  label (display_payee) must still be visible in the output.

  Scenario: Label survives a categorisation rule that matches on display_payee
    Given a clean sources directory with a CSV "bank/2025-01.csv":
      | Date       | Description                                | Amount |
      | 2025-01-15 | 0x1849964c441d9720979f74c2e688709680264ab6 | -4.50  |
    And a transform at "bank/_transform.rhai" that maps Date/Description/Amount to AUD
    And a labels file at "bank/_labels.json" matching "*0x1849964c*" on field "payee" with payee "SCAMMER"
    And a rules file at "bank/_rules.json" matching "*SCAMMER*" on field "payee" with contra "ignore:spam"
    When I run the pipeline
    Then transactions with narration "imported" should have payee "SCAMMER"
    And transactions with narration "imported" should use contra "ignore:spam"
