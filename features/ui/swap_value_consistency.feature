@wip
Feature: Swap transaction value consistency

  A swap transaction's AUD value should be the same whether the
  swap is linked (showing both legs) or unlinked (showing a single leg).
  When a price annotation exists in the base currency (set by the
  pipeline's swap linking), it takes priority over the PriceGraph
  market price.

  Scenario: Unlinked swap leg shows price-annotation value, not PriceGraph value
    Given the app is running with base currency "AUD"
    And I select an account with a PENGU swap that has a price annotation of "0.010594 AUD"
    And the PriceGraph returns 151.52 AUD for 19029.00 PENGU
    Then the value cell should show "201.61 AUD"

  Scenario: PriceGraph value used when annotation is not in base currency
    Given the app is running with base currency "AUD"
    And I select an account with a PENGU swap that has a price annotation of "0.0067 USD"
    And the PriceGraph returns 151.52 AUD for 19029.00 PENGU
    Then the value cell should show "151.52 AUD"
