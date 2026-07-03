Feature: Auto-suggest transform for CSV import

  Scenario: Heuristic maps standard Date/Description/Amount headers
    Given a CSV with headers "Date,Description,Amount"
    And the target account is "assets:bank:savings"
    When I generate a transform suggestion
    Then the suggestion should map "date" from "Date"
    And the suggestion should map "narration" from "Description"
    And the suggestion should map "amount" from "Amount"
    And the suggestion should have blank payee

  Scenario: Heuristic maps Transaction Date and Narration variants
    Given a CSV with headers "Transaction Date,Narration,Debit,Credit,Balance"
    And the target account is "assets:bank:commbank"
    When I generate a transform suggestion
    Then the suggestion should map "date" from "Transaction Date"
    And the suggestion should map "narration" from "Narration"

  Scenario: Heuristic produces valid Rhai that compiles
    Given a CSV with headers "Date,Description,Amount"
    And the target account is "assets:bank:test"
    When I generate a transform suggestion
    Then the suggestion should compile as valid Rhai

  Scenario: Heuristic handles Debit/Credit columns when no Amount column
    Given a CSV with headers "Date,Description,Debit,Credit"
    And the target account is "assets:bank:test"
    When I generate a transform suggestion
    Then the suggestion should derive amount from Debit and Credit

  Scenario: Real bank CSV with dollar signs and non-ISO dates imports successfully
    Given a clean sources directory
    And the target account is "assets:bank:savings"
    And a CSV fixture file "bank_real.csv"
    When I generate and apply the suggested transform
    Then the active ledger should contain 1 transactions
    And the active ledger should include narration "Opening Deposit"

  Scenario: Fallback produces a reasonable default when no headers match
    Given a CSV with headers "Col1,Col2,Col3"
    And the target account is "assets:bank:test"
    When I generate a transform suggestion
    Then the suggestion should contain placeholder comments
    And the suggestion should compile as valid Rhai
