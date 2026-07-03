Feature: Plugin system

  # --- Discovery ---

  Scenario: Discover plugins in plugins directory
    Given a clean sources directory
    And a plugins directory with a plugin "test-prices" with manifest:
      """
      [plugin]
      name = "Test Prices"
      version = "0.1.0"
      description = "A test price plugin"
      script = "sync.py"
      """
    When I discover plugins
    Then I should find 1 plugin
    And plugin "test-prices" should have name "Test Prices"

  Scenario: Discover multiple plugins
    Given a clean sources directory
    And a plugins directory with a plugin "plugin-a" with manifest:
      """
      [plugin]
      name = "Plugin A"
      version = "0.1.0"
      script = "run.py"
      """
    And a plugins directory with a plugin "plugin-b" with manifest:
      """
      [plugin]
      name = "Plugin B"
      version = "0.2.0"
      script = "run.py"
      """
    When I discover plugins
    Then I should find 2 plugins

  Scenario: Skip directories without plugin.toml
    Given a clean sources directory
    And a plugins directory with a plugin "valid" with manifest:
      """
      [plugin]
      name = "Valid"
      version = "0.1.0"
      script = "run.py"
      """
    And an empty directory "not-a-plugin" in the plugins directory
    When I discover plugins
    Then I should find 1 plugin

  Scenario: Parse config fields from manifest
    Given a clean sources directory
    And a plugins directory with a plugin "with-config" with manifest:
      """
      [plugin]
      name = "With Config"
      version = "0.1.0"
      script = "run.py"

      [config]
      commodities = { type = "list", default = ["ETH", "BTC"], description = "Coins" }
      lookback_days = { type = "integer", default = 365, description = "Days" }
      """
    When I discover plugins
    Then plugin "with-config" should have 2 config fields

  Scenario: Parse secret fields from manifest
    Given a clean sources directory
    And a plugins directory with a plugin "with-secrets" with manifest:
      """
      [plugin]
      name = "With Secrets"
      version = "0.1.0"
      script = "run.py"

      [secrets]
      api_key = { type = "string", required = true, description = "API key" }
      """
    When I discover plugins
    Then plugin "with-secrets" should have 1 secret field

  # --- Execution ---

  Scenario: Run a plugin that writes a file to sources
    Given a clean sources directory
    And a plugins directory with a plugin "writer" with manifest:
      """
      [plugin]
      name = "Writer"
      version = "0.1.0"
      script = "write.py"
      """
    And the plugin "writer" has a script "write.py" with content:
      """
      import json, sys, os
      ctx = json.load(sys.stdin)
      prices_dir = os.path.join(ctx["sources_dir"], "_prices")
      os.makedirs(prices_dir, exist_ok=True)
      with open(os.path.join(prices_dir, "TEST.txt"), "w") as f:
          f.write("P 2026-01-15 TEST 100.00 USD\n")
      print(json.dumps({"files_written": ["_prices/TEST.txt"], "records_fetched": 1, "warnings": []}))
      """
    When I run plugin "writer"
    Then the plugin run should succeed
    And the file "_prices/TEST.txt" should exist in sources

  Scenario: Plugin receives config and secrets on stdin
    Given a clean sources directory
    And a plugins directory with a plugin "echo" with manifest:
      """
      [plugin]
      name = "Echo"
      version = "0.1.0"
      script = "echo.py"

      [config]
      greeting = { type = "string", default = "hello", description = "A greeting" }
      """
    And the plugin "echo" has a script "echo.py" with content:
      """
      import json, sys, os
      ctx = json.load(sys.stdin)
      data_dir = ctx["data_dir"]
      os.makedirs(data_dir, exist_ok=True)
      with open(os.path.join(data_dir, "received.json"), "w") as f:
          json.dump(ctx, f)
      print(json.dumps({"files_written": [], "records_fetched": 0, "warnings": []}))
      """
    And plugin "echo" has config:
      """
      {"greeting": "world"}
      """
    And plugin "echo" has secrets:
      """
      {"api_key": "secret123"}
      """
    When I run plugin "echo"
    Then the plugin run should succeed
    And the plugin "echo" data file "received.json" should contain "world"
    And the plugin "echo" data file "received.json" should contain "secret123"

  Scenario: Plugin failure returns error
    Given a clean sources directory
    And a plugins directory with a plugin "failing" with manifest:
      """
      [plugin]
      name = "Failing"
      version = "0.1.0"
      script = "fail.py"
      """
    And the plugin "failing" has a script "fail.py" with content:
      """
      import sys
      print("something went wrong", file=sys.stderr)
      sys.exit(1)
      """
    When I run plugin "failing"
    Then the plugin run should fail
    And the plugin error should contain "something went wrong"

  # --- Config persistence ---

  Scenario: Save and load plugin config
    Given a clean sources directory
    And a plugins directory with a plugin "configurable" with manifest:
      """
      [plugin]
      name = "Configurable"
      version = "0.1.0"
      script = "run.py"

      [config]
      count = { type = "integer", default = 10, description = "Count" }
      """
    When I save config for plugin "configurable":
      """
      {"count": 42}
      """
    Then loading config for plugin "configurable" should return "count" as 42

  Scenario: Save and load plugin secrets
    Given a clean sources directory
    And a plugins directory with a plugin "secret-keeper" with manifest:
      """
      [plugin]
      name = "Secret Keeper"
      version = "0.1.0"
      script = "run.py"

      [secrets]
      token = { type = "string", required = true, description = "Token" }
      """
    When I save secrets for plugin "secret-keeper":
      """
      {"token": "abc123"}
      """
    Then loading secrets for plugin "secret-keeper" should return "token" as "abc123"
