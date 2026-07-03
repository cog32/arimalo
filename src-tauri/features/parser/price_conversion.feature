Feature: Multi-step price conversion

  Scenario: Direct conversion ETH to USD
    Given a clean sources directory
    And a prices file at "_prices/ETH.txt" in sources with content:
      """
      P 2026-01-15 ETH 3200.00 USD
      """
    When I convert 10 "ETH" to base currency "USD" at "2026-01-15"
    Then the converted value should be "32000.00"

  Scenario: Multi-step conversion ETH to AUD via USD
    Given a clean sources directory
    And a prices file at "_prices/ETH.txt" in sources with content:
      """
      P 2026-01-15 ETH 3200.00 USD
      """
    And a prices file at "_prices/USD.txt" in sources with content:
      """
      P 2026-01-15 USD 1.55 AUD
      """
    When I convert 10 "ETH" to base currency "AUD" at "2026-01-15"
    Then the converted value should be "49600.00"

  Scenario: Identity conversion AUD to AUD
    Given a clean sources directory
    When I convert 100 "AUD" to base currency "AUD" at "2026-01-15"
    Then the converted value should be "100.00"

  Scenario: No price data returns empty
    Given a clean sources directory
    When I convert 10 "ETH" to base currency "AUD" at "2026-01-15"
    Then the converted value should be empty

  Scenario: Batch conversion with mixed results
    Given a clean sources directory
    And a prices file at "_prices/ETH.txt" in sources with content:
      """
      P 2026-01-15 ETH 3200.00 USD
      """
    And a prices file at "_prices/USD.txt" in sources with content:
      """
      P 2026-01-15 USD 1.55 AUD
      """
    When I batch convert the following to base currency "AUD" at "2026-01-15":
      | amount | commodity |
      | 10     | ETH       |
      | 50     | AUD       |
      | 5      | DOGE      |
    Then the batch results should be:
      | value    |
      | 49600.00 |
      | 50.00    |
      | empty    |

  Scenario: Negative amounts preserve sign
    Given a clean sources directory
    And a prices file at "_prices/ETH.txt" in sources with content:
      """
      P 2026-01-15 ETH 3200.00 USD
      """
    When I convert -5 "ETH" to base currency "USD" at "2026-01-15"
    Then the converted value should be "-16000.00"
