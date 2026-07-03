Feature: UI updates work on the local DOM, non-intrusively

  Architectural contract: re-renders should diff the new HTML against
  the existing DOM and only swap the parts that actually changed.
  Unchanged subtrees keep their identity — preserving in-flight CSS
  transitions, focused inputs, scroll position, and any DOM marker
  attached by user code or tests.

  Scenario: Row identity is preserved across a re-render
    Given the app is running
    When I parse the transactions file "src-tauri/features/fixtures/multi_fill_exchange.transactions"
    And I select the account "assets:exchange:bybit"
    And I should see a transaction row for payee "Bybit"
    When I tag the first transaction row with marker "preserved-by-morphdom"
    And I trigger a render by clearing then restoring the search input
    Then the first transaction row still has marker "preserved-by-morphdom"
