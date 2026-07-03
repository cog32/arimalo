Feature: Account gap detection

  Scenario: Detect missing months for an account
    Given a generated directory with archive ledgers:
      | file           | content_account     | dates                 |
      | ledger-202401  | assets:bank:savings | 2024-01-05,2024-01-15 |
      | ledger-202402  | assets:bank:savings | 2024-02-10            |
      | ledger-202404  | assets:bank:savings | 2024-04-01            |
    When I run gap detection on the generated directory
    Then the gaps for "assets:bank:savings" should be:
      | month   |
      | 2024-03 |

  Scenario: Multiple accounts with different gap patterns
    Given a generated directory with archive ledgers:
      | file           | content_account     | dates      |
      | ledger-202401  | assets:bank:savings | 2024-01-05 |
      | ledger-202401  | assets:bank:cdia    | 2024-01-10 |
      | ledger-202403  | assets:bank:savings | 2024-03-01 |
      | ledger-202403  | assets:bank:cdia    | 2024-03-15 |
    When I run gap detection on the generated directory
    Then the gaps for "assets:bank:savings" should be:
      | month   |
      | 2024-02 |
    And the gaps for "assets:bank:cdia" should be:
      | month   |
      | 2024-02 |

  Scenario: No gaps returns empty
    Given a generated directory with archive ledgers:
      | file           | content_account     | dates      |
      | ledger-202401  | assets:bank:savings | 2024-01-05 |
      | ledger-202402  | assets:bank:savings | 2024-02-10 |
    When I run gap detection on the generated directory
    Then there should be no gaps for "assets:bank:savings"
