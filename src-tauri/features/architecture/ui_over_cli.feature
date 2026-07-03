Feature: UI is a thin wrapper over the CLI

  The Tauri UI does not implement its own data processing. It uses the
  same query and pipeline functions that the CLI tools use. The UI
  commands (query_search, load_account_tree, rebuild_pipeline) call
  the same library code as arimalo-query, arimalo-regenerate, etc.

  This ensures:
  - CLI and UI always produce identical results
  - Features can be tested via CLI without a running app
  - The UI is a presentation layer, not a data layer

  Scenario: UI query produces same results as CLI query
    Given a generated set with transactions
    When I query "account:assets:savings" via the CLI
    And I query "account:assets:savings" via the UI command
    Then both results should contain the same transactions
    And both results should have the same total count

  Scenario: UI rebuild uses the same pipeline as CLI regenerate
    Given a sources directory with CSVs
    When I run arimalo-regenerate via CLI
    And I run rebuild_pipeline via the UI command
    Then the generated output files should be identical

  Scenario: UI trade link uses the same rule format as CLI
    Given a generated set with two linkable transactions
    When I link them via the UI
    Then the _rules.json should be readable by the CLI pipeline
    And running arimalo-regenerate should produce the same linked output
