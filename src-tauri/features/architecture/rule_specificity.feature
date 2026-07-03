Feature: Rule matching prioritizes specificity over folder depth

  When a transaction is matched, all applicable rules from the
  source folder up to `sources/` are combined into one list and
  evaluated in specificity order, not file/folder order.

  Specificity tiers (lower = more specific = higher precedence):

    0. has a meta condition (exact id-style match)
    1. has both payee and narration constraints
    2. has a payee constraint only
    3. has a narration constraint only
    4. other field-specific (commodity / amount / fee only)
    5. general fallback (no payee/narration/meta constraint)

  Within a tier, original order is preserved (stable sort), so
  per-folder rules continue to beat root rules and earlier file
  entries beat later ones.

  Design choice: specificity dominates folder depth. A root-level
  meta (tier 0) or payee (tier 2) rule will beat a leaf-level
  narration (tier 3) rule. We chose this because exact / payee
  matches encode stronger user intent than fuzzy narration matches,
  regardless of where they live. The alternative — folder depth
  dominates, leaf 1..5 then parent 1..5 — was rejected; it would
  let a leaf narration rule shadow a root payee rule.

  Override: if you need leaf precedence over a root rule of higher
  specificity, copy the rule into the leaf folder with at least
  matching specificity.

  Scenario: Payee rule beats earlier narration rule in same file
    Given a narration rule "narr" with pattern "coffee" routes to "expenses:food"
    And a payee-conditioned rule "pay" with payee "Starbucks" routes to "expenses:coffee"
    When a transaction with payee "Starbucks" and narration "coffee" is matched
    Then it routes to "expenses:coffee"

  Scenario: Root payee rule beats leaf narration rule
    Given a narration rule with pattern "coffee" routes to "expenses:food" in "richard/wallet"
    And a payee-conditioned rule with payee "Starbucks" routes to "expenses:coffee" in root
    When a transaction with payee "Starbucks" and narration "coffee" is matched in "richard/wallet"
    Then it routes to "expenses:coffee"

  Scenario: Leaf payee rule beats root payee rule (intra-tier folder order)
    Given a payee rule with payee "Starbucks" routes to "expenses:root" in root
    And a payee rule with payee "Starbucks" routes to "expenses:leaf" in "richard/wallet"
    When a transaction with payee "Starbucks" is matched in "richard/wallet"
    Then it routes to "expenses:leaf"

  Scenario: Meta rule beats payee rule across folders
    Given a payee-conditioned rule with payee "Foo" routes to "expenses:foo" in "richard/wallet"
    And a meta rule with pattern "txn:csv-*" routes to "equity:trading:sell" in root
    When a transaction with payee "Foo" and meta "txn:csv-001" is matched in "richard/wallet"
    Then it routes to "equity:trading:sell"
