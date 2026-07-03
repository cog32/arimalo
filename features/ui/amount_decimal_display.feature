@wip
Feature: Smart decimal display for amounts in the account view

  By default, transaction amounts in the account view render with 2 significant
  digits — most values get 2 decimal places, sub-cent values extend precision
  enough to keep two significant digits visible. No per-commodity config is
  required for this. An explicit decimals override in displayConfig still wins
  when set (e.g. "AUD": { "decimals": 2 } keeps cents stable).

  Background:
    Given the app is running

  Scenario: ETH amounts default to 2dp without any displayConfig override
    When I view a transaction row whose ETH posting amount is 9.241119
    Then the amount cell should render "9.24"

  Scenario: Sub-cent amounts keep two significant digits
    When I view a transaction row whose ETH posting amount is 0.0034
    Then the amount cell should render "0.0034"

  Scenario: Trailing zeros do not pad an ETH amount
    When I view a transaction row whose ETH posting amount is 0.0625
    Then the amount cell should render "0.06"

  Scenario: Explicit per-commodity override overrides the smart default
    Given displayConfig has "ETH": { "decimals": 6 }
    When I view a transaction row whose ETH posting amount is 9.241119
    Then the amount cell should render "9.241119"

  Scenario: Fiat currencies stay at the configured 2dp
    Given displayConfig has "AUD": { "decimals": 2 }
    When I view a transaction row whose AUD posting amount is 100
    Then the amount cell should render "100.00"
