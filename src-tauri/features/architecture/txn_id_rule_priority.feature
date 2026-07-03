Feature: Txn-id-anchored rules use bare patterns and sit at the top

  A txn-id rule is a rule whose pattern is the bare form `txn:<hash>`
  (no `*`, no `|`, no whitespace) and whose `match_field` is `meta`.
  Such rules are unique pointers: the hash is content-addressed, so the
  rule matches exactly one transaction by design.

  A `leg:<id>` rule is the same shape but anchors a single leg of one
  on-chain transaction. When a swap/multi-hop expands into several legs
  that share one `txn:<hash>`, a `leg:` rule lets the user allocate one
  leg independently. Within the meta tier a `leg:` rule outranks a
  `txn:` rule, so a per-leg override always beats the whole-transaction
  rule for its own leg.

  Two invariants govern these rules:

  1. Match semantics. The rule matches iff the transaction's meta
     string, split on `,` and trimmed per-segment, contains a segment
     byte-equal to the pattern (case-insensitive). The legacy
     `wildcard_match` substring path is reserved for genuinely
     glob-shaped patterns (e.g. `*token_transfer*`).

  2. Top-anchor on write. Every writer that produces a txn-id rule
     places it at index 0 of the folder's `_rules.json`. This
     guarantees txn-id rules never lose priority to broader meta-field
     rules in `find_match_prioritized`.

  Override: delete the rule. Future cleanup of legacy `*txn:<hash>*`
  patterns will be a one-shot migration command.

  Background:
    Given a clean sources directory

  Scenario: Hiding a transaction writes a bare txn:HASH rule at index 0
    When I hide txn id "txn:abc123" in folder "richard/wallet"
    Then the rule at index 0 of "richard/wallet" has pattern "txn:abc123"
    And the rule at index 0 of "richard/wallet" has match_field "meta"
    And the rule at index 0 of "richard/wallet" routes to "ignore:hidden"

  Scenario: Hide rule promotes above pre-existing rules
    Given a meta rule "broad" with pattern "*token_transfer*" routes to "expenses:tokens" in "richard/wallet"
    When I hide txn id "txn:specific" in folder "richard/wallet"
    Then the rule at index 0 of "richard/wallet" has pattern "txn:specific"
    And the rule at index 1 of "richard/wallet" has pattern "*token_transfer*"

  Scenario: Hide rule wins over a broader meta rule via segment-exact match
    Given a meta rule "broad" with pattern "*token_transfer*" routes to "expenses:tokens" in "richard/wallet"
    When I hide txn id "txn:specific" in folder "richard/wallet"
    Then a meta string "token_transfer, txn:specific" in "richard/wallet" routes to "ignore:hidden"

  Scenario: Re-hiding the same txn id is idempotent
    When I hide txn id "txn:abc123" in folder "richard/wallet"
    And I hide txn id "txn:abc123" in folder "richard/wallet"
    Then "richard/wallet" contains exactly 1 rule

  Scenario: A per-leg rule outranks a shared-txn rule for its own leg
    Given a meta rule "shared" with pattern "txn:H" routes to "equity:trading:sell" in "richard/wallet"
    And a meta rule "perleg" with pattern "leg:l-bbb" routes to "equity:trading:buy" in "richard/wallet"
    Then a meta string "txn:H, leg:l-bbb" in "richard/wallet" routes to "equity:trading:buy"
    And a meta string "txn:H, leg:l-aaa" in "richard/wallet" routes to "equity:trading:sell"
