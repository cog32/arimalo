@wip
Feature: Tax Savings (loss-harvesting) report

  The Tax Savings report surfaces tax-loss-harvesting opportunities: current
  holdings that are underwater — their mark-to-market value sits below the FIFO
  cost basis — so selling them now would realise a capital loss. It estimates
  the dollar tax saved by offsetting those losses against the financial year's
  realised capital gains, at the configured marginal rate. It appears under the
  Tax sidebar menu, alongside Capital Gains Tax and Income Tax.

  Scenario: Tax Savings button is reachable from the Tax section of the report menu
    Given the app is showing the Reports view
    Then the report menu lists "Tax Savings" under the "Tax" heading
    When I click the "Tax Savings" button in the report menu
    Then the report content should display the Tax Savings summary band
    And the report content should list the underwater holdings

  Scenario: Only underwater holdings are listed, worst loss first
    Given a vault holding BTC at an unrealised gain and SOL at an unrealised loss
    When I open the Tax Savings report
    Then SOL appears in the holdings table with a positive realisable loss
    And BTC does not appear in the holdings table
    And rows are ordered by unrealised loss descending

  Scenario: Unpriced holdings are excluded
    Given a vault containing a held commodity with no resolvable base-currency price
    When I open the Tax Savings report
    Then that commodity is not listed as a harvestable loss

  Scenario: Each holding row shows cost basis, value, loss and percent below cost
    Given the app is showing a Tax Savings report with one underwater holding
    Then the row shows the commodity, quantity, FIFO cost basis, current value, unrealised loss, and % below cost
    And the row has no per-holding dollar-tax column

  Scenario: Summary band reconciles the harvestable loss against this year's gains
    Given a vault with 10,000 of realised short-term capital gains this FY
    And 15,000 of harvestable unrealised losses
    When I open the Tax Savings report
    Then the summary band shows realisable losses of 15,000
    And shows 10,000 offsetting gains now and 5,000 carried forward
    And shows an estimated tax saved equal to 10,000 times the marginal rate

  Scenario: Offsetting a long-term (discounted) gain halves the taxable reduction
    Given a vault with 10,000 of realised long-term (discount-eligible) gains this FY
    And 15,000 of harvestable unrealised losses
    When I open the Tax Savings report
    Then the estimated tax saved reflects only the discounted half of the offset gain

  Scenario: Marginal tax rate is configurable in Tax Settings
    Given the Tax Settings modal is open
    Then there is a "Marginal tax rate %" field defaulting to 47
    When I change it to 39 and save
    Then the Tax Savings report's estimated tax saved uses a 39% rate

  Scenario: Balances no longer lives under the Tax heading
    Given the app is showing the Reports view
    Then the "Balances" button appears under the "Portfolio" heading
    And the "Balances" button does not appear under the "Tax" heading
