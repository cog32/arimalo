@appium
Feature: macOS E2E (Appium mac2)

  Scenario: App starts and shows imported data
    Given the macOS app is running with imported file "sample-data/example.transactions"
    Then the UI should show text containing "Parsed"
    And the UI should show text containing "Kraken"

