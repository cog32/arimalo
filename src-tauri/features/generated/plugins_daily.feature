Feature: Daily plugin backfill

  The app can run a subset of plugins ("daily" plugins) as an automatic
  backfill. Each daily plugin is incremental, so a run fills whatever prices
  are missing. A once-per-day guard skips plugins that already succeeded today
  so repeated launches don't re-fetch.

  Background:
    Given a clean sources directory

  # --- daily flag parsing ---

  Scenario: Daily flag defaults to false when absent from the manifest
    Given a plugins directory with a plugin "non-daily" with manifest:
      """
      [plugin]
      name = "Non Daily"
      version = "0.1.0"
      script = "run.py"
      """
    When I discover plugins
    Then plugin "non-daily" should not be marked daily

  Scenario: Daily flag is parsed from the manifest
    Given a plugins directory with a plugin "is-daily" with manifest:
      """
      [plugin]
      name = "Is Daily"
      version = "0.1.0"
      script = "run.py"
      daily = true
      """
    When I discover plugins
    Then plugin "is-daily" should be marked daily

  # --- selection ---

  Scenario: Daily run executes only daily-flagged plugins
    Given a plugins directory with a plugin "daily-writer" with manifest:
      """
      [plugin]
      name = "Daily Writer"
      version = "0.1.0"
      script = "write.py"
      daily = true
      """
    And the plugin "daily-writer" has a script "write.py" with content:
      """
      import json, sys, os
      ctx = json.load(sys.stdin)
      d = os.path.join(ctx["sources_dir"], "_prices")
      os.makedirs(d, exist_ok=True)
      open(os.path.join(d, "DAILY.txt"), "w").write("P 2026-01-15 DAILY 1.00 USD\n")
      print(json.dumps({"files_written": ["_prices/DAILY.txt"], "records_fetched": 1, "warnings": []}))
      """
    And a plugins directory with a plugin "manual-writer" with manifest:
      """
      [plugin]
      name = "Manual Writer"
      version = "0.1.0"
      script = "write.py"
      """
    And the plugin "manual-writer" has a script "write.py" with content:
      """
      import json, sys, os
      ctx = json.load(sys.stdin)
      d = os.path.join(ctx["sources_dir"], "_prices")
      os.makedirs(d, exist_ok=True)
      open(os.path.join(d, "MANUAL.txt"), "w").write("P 2026-01-15 MANUAL 1.00 USD\n")
      print(json.dumps({"files_written": ["_prices/MANUAL.txt"], "records_fetched": 1, "warnings": []}))
      """
    When I run the daily plugins
    Then the daily run summary should have 1 outcome
    And the daily outcome for "daily-writer" should be success
    And the file "_prices/DAILY.txt" should exist in sources
    And the file "_prices/MANUAL.txt" should not exist in sources

  Scenario: Daily run continues after a failing plugin
    Given a plugins directory with a plugin "a-failing" with manifest:
      """
      [plugin]
      name = "A Failing"
      version = "0.1.0"
      script = "fail.py"
      daily = true
      """
    And the plugin "a-failing" has a script "fail.py" with content:
      """
      import sys
      print("boom", file=sys.stderr)
      sys.exit(1)
      """
    And a plugins directory with a plugin "b-writer" with manifest:
      """
      [plugin]
      name = "B Writer"
      version = "0.1.0"
      script = "write.py"
      daily = true
      """
    And the plugin "b-writer" has a script "write.py" with content:
      """
      import json, sys, os
      ctx = json.load(sys.stdin)
      d = os.path.join(ctx["sources_dir"], "_prices")
      os.makedirs(d, exist_ok=True)
      open(os.path.join(d, "B.txt"), "w").write("P 2026-01-15 B 1.00 USD\n")
      print(json.dumps({"files_written": ["_prices/B.txt"], "records_fetched": 1, "warnings": []}))
      """
    When I run the daily plugins
    Then the daily run summary should have 2 outcomes
    And the daily outcome for "a-failing" should be failure
    And the daily outcome for "b-writer" should be success
    And the file "_prices/B.txt" should exist in sources

  Scenario: Daily run reports a partial-success plugin distinctly from a failure
    Given a plugins directory with a plugin "partial-writer" with manifest:
      """
      [plugin]
      name = "Partial Writer"
      version = "0.1.0"
      script = "partial.py"
      daily = true
      """
    And the plugin "partial-writer" has a script "partial.py" with content:
      """
      import json, sys, os
      ctx = json.load(sys.stdin)
      d = os.path.join(ctx["sources_dir"], "_prices")
      os.makedirs(d, exist_ok=True)
      open(os.path.join(d, "PARTIAL.txt"), "w").write("P 2026-01-15 PARTIAL 1.00 USD\n")
      print(json.dumps({"files_written": ["_prices/PARTIAL.txt"], "records_fetched": 1, "warnings": ["a ticker was delisted"]}))
      sys.exit(2)
      """
    When I run the daily plugins
    Then the daily run summary should have 1 outcome
    And the daily outcome for "partial-writer" should be partial
    And the daily outcome for "partial-writer" should not be failure
    And the file "_prices/PARTIAL.txt" should exist in sources

  # --- once-per-day guard ---

  Scenario: Daily run skips a plugin that already succeeded today
    Given a plugins directory with a plugin "already-today" with manifest:
      """
      [plugin]
      name = "Already Today"
      version = "0.1.0"
      script = "write.py"
      daily = true
      """
    And the plugin "already-today" has a script "write.py" with content:
      """
      import json, sys, os
      ctx = json.load(sys.stdin)
      d = os.path.join(ctx["sources_dir"], "_prices")
      os.makedirs(d, exist_ok=True)
      open(os.path.join(d, "TODAY.txt"), "w").write("x")
      print(json.dumps({"files_written": [], "records_fetched": 0, "warnings": []}))
      """
    And the plugin "already-today" last succeeded today
    When I run the daily plugins skipping those already run today
    Then the daily run summary should have 1 outcome
    And the daily outcome for "already-today" should be skipped
    And the file "_prices/TODAY.txt" should not exist in sources

  Scenario: Daily run re-runs a plugin that last succeeded yesterday
    Given a plugins directory with a plugin "ran-yesterday" with manifest:
      """
      [plugin]
      name = "Ran Yesterday"
      version = "0.1.0"
      script = "write.py"
      daily = true
      """
    And the plugin "ran-yesterday" has a script "write.py" with content:
      """
      import json, sys, os
      ctx = json.load(sys.stdin)
      d = os.path.join(ctx["sources_dir"], "_prices")
      os.makedirs(d, exist_ok=True)
      open(os.path.join(d, "YDAY.txt"), "w").write("x")
      print(json.dumps({"files_written": [], "records_fetched": 0, "warnings": []}))
      """
    And the plugin "ran-yesterday" last succeeded yesterday
    When I run the daily plugins skipping those already run today
    Then the daily run summary should have 1 outcome
    And the daily outcome for "ran-yesterday" should be success
    And the file "_prices/YDAY.txt" should exist in sources

  Scenario: Forced daily run re-runs a plugin even if it already succeeded today
    Given a plugins directory with a plugin "force-me" with manifest:
      """
      [plugin]
      name = "Force Me"
      version = "0.1.0"
      script = "write.py"
      daily = true
      """
    And the plugin "force-me" has a script "write.py" with content:
      """
      import json, sys, os
      ctx = json.load(sys.stdin)
      d = os.path.join(ctx["sources_dir"], "_prices")
      os.makedirs(d, exist_ok=True)
      open(os.path.join(d, "FORCED.txt"), "w").write("x")
      print(json.dumps({"files_written": [], "records_fetched": 0, "warnings": []}))
      """
    And the plugin "force-me" last succeeded today
    When I run the daily plugins
    Then the daily outcome for "force-me" should be success
    And the file "_prices/FORCED.txt" should exist in sources

  # --- empty set ---

  Scenario: Daily run with no daily plugins produces an empty summary
    Given a plugins directory with a plugin "manual-only" with manifest:
      """
      [plugin]
      name = "Manual Only"
      version = "0.1.0"
      script = "run.py"
      """
    When I run the daily plugins
    Then the daily run summary should have 0 outcomes
