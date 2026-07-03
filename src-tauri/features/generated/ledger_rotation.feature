Feature: Generated ledger log rotation

  Scenario: Rotate previous month ledger into archive
    Given a generated ledger file for month "202601"
    When I rotate the generated ledger with current month "202602"
    Then the archive ledger file "ledger-202601.transactions" should exist
    And the active ledger file should be empty

  Scenario: Do not rotate when month matches
    Given a generated ledger file for month "202601"
    When I rotate the generated ledger with current month "202601"
    Then the archive ledger file "ledger-202601.transactions" should not exist
    And the active ledger file should not be empty

