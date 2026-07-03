Feature: Import prices file

  # --- P-directive format ---

  Scenario: Import valid P-directive file stores per-commodity file
    Given a clean sources directory
    And a prices import file with content:
      """
      P 2026-01-15 ETH 3200.00 USD
      P 2026-01-16 ETH 3250.50 USD
      """
    When I import the prices file
    Then the prices import should succeed with 2 directives
    And the prices import should include commodity "ETH"
    And the file "_prices/ETH.txt" should exist in sources

  Scenario: Import P-directive with multiple commodities splits into separate files
    Given a clean sources directory
    And a prices import file with content:
      """
      P 2026-01-15 ETH 3200.00 USD
      P 2026-01-15 BTC 42000.00 USD
      P 2026-01-16 ETH 3250.50 USD
      """
    When I import the prices file
    Then the prices import should succeed with 3 directives
    And the prices import should include commodity "ETH"
    And the prices import should include commodity "BTC"
    And the file "_prices/ETH.txt" should exist in sources
    And the file "_prices/BTC.txt" should exist in sources

  Scenario: Import invalid content returns error
    Given a clean sources directory
    And a prices import file with content:
      """
      this is not valid
      neither is this
      """
    When I import the prices file
    Then the prices import should fail with "no valid price directives"

  Scenario: Re-import overwrites existing file for same commodity
    Given a clean sources directory
    And a prices file at "_prices/ETH.txt" in sources with content:
      """
      P 2026-01-10 ETH 3000.00 USD
      """
    And a prices import file with content:
      """
      P 2026-01-15 ETH 3200.00 USD
      P 2026-01-16 ETH 3250.50 USD
      """
    When I import the prices file
    Then the prices import should succeed with 2 directives
    And the file "_prices/ETH.txt" should exist in sources

  # --- CSV format ---

  Scenario: Import CSV with multiple commodities splits per commodity
    Given a clean sources directory
    And a prices CSV import file with content:
      """
      Date,Commodity,Price,Currency
      2026-01-15,ETH,3200.00,USD
      2026-01-15,BTC,42000.00,USD
      2026-01-16,ETH,3250.50,USD
      """
    When I import the prices file
    Then the prices import should succeed with 3 directives
    And the prices import should include commodity "ETH"
    And the prices import should include commodity "BTC"
    And the file "_prices/ETH.txt" should exist in sources
    And the file "_prices/BTC.txt" should exist in sources

  Scenario: Import CSV with single commodity stores correctly
    Given a clean sources directory
    And a prices CSV import file with content:
      """
      Date,Commodity,Price,Currency
      2026-02-01,SOL,120.50,USD
      2026-02-02,SOL,122.00,USD
      """
    When I import the prices file
    Then the prices import should succeed with 2 directives
    And the prices import should include commodity "SOL"
    And the file "_prices/SOL.txt" should exist in sources

  # --- Lookup ---

  Scenario: Exact date match lookup
    Given a clean sources directory
    And a prices file at "_prices/ETH.txt" in sources with content:
      """
      P 2026-01-15 ETH 3200.00 USD
      P 2026-01-16 ETH 3250.50 USD
      """
    When I look up the price for "ETH" at "2026-01-16"
    Then the lookup result should be "3250.50" "USD"

  Scenario: Nearest earlier date lookup
    Given a clean sources directory
    And a prices file at "_prices/ETH.txt" in sources with content:
      """
      P 2026-01-15 ETH 3200.00 USD
      P 2026-01-20 ETH 3300.00 USD
      """
    When I look up the price for "ETH" at "2026-01-18"
    Then the lookup result should be "3200.00" "USD"

  Scenario: No prices file for commodity returns empty
    Given a clean sources directory
    When I look up the price for "DOGE" at "2026-01-15"
    Then the lookup result should be empty

  Scenario: Target before all available prices returns earliest as fallback
    Given a clean sources directory
    And a prices file at "_prices/ETH.txt" in sources with content:
      """
      P 2026-03-01 ETH 3500.00 USD
      P 2026-03-15 ETH 3600.00 USD
      """
    When I look up the price for "ETH" at "2026-01-01"
    Then the lookup result should be "3500.00" "USD"
