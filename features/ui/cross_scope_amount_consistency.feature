@wip
Feature: Transaction amount is consistent across scopes

  A single transaction must show the same gross magnitude at every account
  scope that contains it. When both legs of a transaction fall inside the
  selected scope (e.g. an internal transfer with contra `assets:crypto:transfer`
  viewed at scope `crypto:`), the amount column must NOT collapse to zero —
  doing so hides exactly the rows the user needs to see in order to detect
  and fix half-recorded transfers (e.g. an incoming wallet leg with no matching
  exchange withdrawal).

  Scenario: Receive viewed at the wallet scope shows the gross amount
    Given a SOL receive of 7999.99 with contra "assets:crypto:transfer"
    When I view the transaction list for "assets:crypto:wallet:solana"
    Then the amount cell shows "7999.99 SOL"

  Scenario: Same receive viewed at the broader crypto scope shows the same gross amount
    Given a SOL receive of 7999.99 with contra "assets:crypto:transfer"
    When I view the transaction list for "assets:crypto"
    Then the amount cell shows "7999.99 SOL"
    And the row is not hidden or netted to zero

  Scenario: Buy with equity contra is unchanged at the asset scope
    Given a SOL buy of 1.0 with contra "equity:trading:buy:trade-1"
    When I view the transaction list for "assets"
    Then the amount cell shows "1.0 SOL"
