Feature: Per-account configuration
  Each account folder can have a _config.json with settings like
  explorer_url for linking transactions to blockchain explorers.
  Config uses override semantics: the nearest _config.json walking
  up from the account folder to sources root wins.

  Scenario: Config is loaded from the account folder
    Given a sources directory with account folder "richard/crypto/wallet/solana"
    And a _config.json at "richard/crypto/wallet/solana" with explorer_url "https://solscan.io/tx/{txn_id}"
    When I resolve the account config for "richard/crypto/wallet/solana"
    Then the explorer_url should be "https://solscan.io/tx/{txn_id}"

  Scenario: Config inherits from parent folder
    Given a sources directory with account folder "richard/crypto/wallet/ethereum/0xabc"
    And a _config.json at "richard/crypto/wallet/ethereum" with explorer_url "https://etherscan.io/tx/{txn_id}"
    When I resolve the account config for "richard/crypto/wallet/ethereum/0xabc"
    Then the explorer_url should be "https://etherscan.io/tx/{txn_id}"

  Scenario: Nearest config wins over parent
    Given a sources directory with account folder "richard/crypto/wallet/solana"
    And a _config.json at "richard/crypto/wallet" with explorer_url "https://parent-explorer.io/tx/{txn_id}"
    And a _config.json at "richard/crypto/wallet/solana" with explorer_url "https://solscan.io/tx/{txn_id}"
    When I resolve the account config for "richard/crypto/wallet/solana"
    Then the explorer_url should be "https://solscan.io/tx/{txn_id}"

  Scenario: No config returns empty
    Given a sources directory with account folder "richard/cash/bank"
    When I resolve the account config for "richard/cash/bank"
    Then the explorer_url should be empty
