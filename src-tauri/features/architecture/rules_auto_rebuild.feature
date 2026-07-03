Feature: Rule changes auto-rebuild and reach the UI

  The Tauri file watcher recursively watches the sources directory. When
  a `_rules.json` file is modified — by the UI, a CLI tool, or a plugin
  — the next pipeline run must (a) re-apply rules to the affected
  folders, (b) write the updated ledger to disk, and (c) report a
  non-zero `output_files_written` so the frontend's `pipeline-rebuilt`
  handler triggers a `loadGeneratedLedger` (the handler ignores events
  with `output_files_written == 0`).

  This is the contract the UI relies on: the user changes a rule (or a
  plugin like `sol-labels` does), and the running app reflects the new
  categorisation without a manual reload.

  Background:
    Given a clean sources directory

  Scenario: Modifying _rules.json between runs reflects the new payee in the ledger
    Given a CSV "bank/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | imported    | -10.00 |
    And a transform at "bank/_transform.rhai" that maps Date/Description/Amount to USD
    And a rules file at "bank/_rules.json" matching "imported" with payee "Original"
    When I run the pipeline
    Then transactions with narration "imported" should have payee "Original"
    When a rules file is added to "bank/_rules.json" matching "imported" with payee "Updated"
    And I run the pipeline again
    Then transactions with narration "imported" should have payee "Updated"
    And the pipeline should report output files written > 0

  Scenario: A plugin adding _rules.json from outside the UI is picked up on next run
    Given a CSV "bank/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | imported    | -10.00 |
    And a transform at "bank/_transform.rhai" that maps Date/Description/Amount to USD
    When I run the pipeline
    When a rules file is added to "bank/_rules.json" matching "imported" with payee "Plugin-Added"
    And I run the pipeline again
    Then transactions with narration "imported" should have payee "Plugin-Added"
    And the pipeline should report output files written > 0

  # The user-visible bug: running a plugin that writes _rules.json must
  # trigger the pipeline rebuild and emit a `pipeline-rebuilt` event with
  # `output_files_written > 0` so the frontend reloads the active ledger.
  # Today, `run_plugin_cmd` runs the plugin subprocess but does NOT chain
  # a pipeline rebuild — it relies on the OS file watcher firing, which
  # is unreliable on macOS for plugin-written files. The fix is to make
  # the plugin Tauri command run the pipeline explicitly after the
  # plugin completes (same pattern as `save_rule`, `hide_transaction`
  # etc).
  Scenario: Running a plugin that writes _rules.json auto-rebuilds the ledger
    Given a CSV "bank/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | imported    | -10.00 |
    And a transform at "bank/_transform.rhai" that maps Date/Description/Amount to USD
    And a plugins directory with a plugin "rule-writer" with manifest:
      """
      [plugin]
      name = "Rule Writer"
      version = "0.1.0"
      script = "write_rules.py"
      """
    And the plugin "rule-writer" has a script "write_rules.py" with content:
      """
      import json, sys, os
      ctx = json.load(sys.stdin)
      rules_path = os.path.join(ctx["sources_dir"], "bank", "_rules.json")
      with open(rules_path, "w") as f:
          json.dump({"rules": [{"id": "plugin-rule", "pattern": "imported", "payee": "Plugin-Set"}]}, f)
      print(json.dumps({"files_written": ["bank/_rules.json"], "records_fetched": 1, "warnings": []}))
      """
    When I run the pipeline
    And I run plugin "rule-writer"
    Then transactions with narration "imported" should have payee "Plugin-Set"
