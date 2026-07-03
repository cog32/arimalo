Feature: Folder-derived accounts

  The folder structure under sources defines the account layout:
    {owner}/{asset-class}/{custody-type}/{institution}/{account}
  This maps to hledger account: assets:{asset-class}:{custody-type}:{institution}:{account}

  The owner (top-level directory) is an organisational grouping.
  The remaining path segments form the account hierarchy under "assets:".

  When a transform does not specify an account field, the account is
  derived automatically from the folder structure. This eliminates the
  need for an explicit accounts.transactions file.

  Scenario: Deep folder derives account name
    Given a clean sources directory with a CSV "richard/cash/bank/cba/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "richard/cash/bank/cba/savings/_transform.rhai" without an account field
    When I run the pipeline for month "202501"
    Then the transaction should use account "assets:cash:bank:cba:savings"

  Scenario: Partial-depth folder derives account name
    Given a clean sources directory with a CSV "richard/cash/bank/ubank/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Transfer    | 100.00 |
    And a transform at "richard/cash/bank/ubank/_transform.rhai" without an account field
    When I run the pipeline for month "202501"
    Then the transaction should use account "assets:cash:bank:ubank"

  Scenario: Account declaration is auto-generated from folder structure
    Given a clean sources directory with a CSV "richard/cash/bank/cba/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "richard/cash/bank/cba/savings/_transform.rhai" without an account field
    And no "accounts.transactions" file exists
    When I run the pipeline for month "202501"
    Then the output should include an auto-generated account declaration for "assets:cash:bank:cba:savings"
