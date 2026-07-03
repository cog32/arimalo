Feature: Cross-folder transaction propagation preserves folder ownership

  When two wallet folders have CSV rows sharing the same blockchain tx_hash
  (e.g. a self-transfer between wallets), the pipeline must write each
  folder's ledger with posting[0] pointing to that folder's own account —
  not the other folder's account.

  Scenario: Self-transfer between two wallets keeps correct posting accounts
    Given a clean sources directory
    And a CSV "richard/crypto/wallet/alpha/txns.csv" with columns "Date,Description,Amount,TxHash":
      | Date       | Description      | Amount | TxHash |
      | 2025-01-15 | Incoming from B  | 100.00 | shared-tx-001 |
    And a CSV "richard/crypto/wallet/beta/txns.csv" with columns "Date,Description,Amount,TxHash":
      | Date       | Description      | Amount  | TxHash |
      | 2025-01-15 | Outgoing to A    | -100.00 | shared-tx-001 |
    And a transform with txn_id at "richard/crypto/wallet/alpha/_transform.rhai"
    And a transform with txn_id at "richard/crypto/wallet/beta/_transform.rhai"
    When I run the pipeline
    Then the folder ledger "crypto/wallet/alpha" posting 0 should use account "assets:crypto:wallet:alpha"
    And the folder ledger "crypto/wallet/beta" posting 0 should use account "assets:crypto:wallet:beta"
