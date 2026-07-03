Feature: Account properties (friendly name)

  Account declarations can include a "name" child line that provides
  a human-readable label for the account. The parser extracts these
  into account_properties and the pipeline passes them through.

  Scenario: Parse account name property
    Given a clean sources directory with a CSV and transform
    And an accounts file at "bank/accounts.transactions" with:
      """
      account assets:bank:test AUD
          name My Savings
      """
    When I run the pipeline for month "202501"
    Then the account properties should map "assets:bank:test" to name "My Savings"

  Scenario: Account without name property has no entry
    Given a clean sources directory with a CSV and transform
    When I run the pipeline for month "202501"
    Then the account properties should be empty

  Scenario: Multiple accounts with name properties
    Given a clean sources directory with a CSV and transform
    And an accounts file at "bank/accounts.transactions" with:
      """
      account assets:bank:test AUD
          name My Savings
          opening 100.00

      account assets:bank:checking AUD
          name Everyday Account
      """
    When I run the pipeline for month "202501"
    Then the account properties should map "assets:bank:test" to name "My Savings"
    And the account properties should map "assets:bank:checking" to name "Everyday Account"
