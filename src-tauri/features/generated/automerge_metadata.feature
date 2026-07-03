Feature: Automerge metadata layer for multi-machine sync

  Scenario: Initialize metadata from sources with transactions
    Given a clean sources directory with a CSV "bank/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "bank/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    And a "manual.transactions" file with payee "ManualEntry"
    When I run the pipeline for month "202501"
    And I initialize metadata from sources
    Then the metadata should contain transaction refs
    And the metadata file manifest should include "manual.transactions"
    And the metadata should track this device

  Scenario: Metadata records file manifest with content hashes
    Given a clean sources directory with a CSV "bank/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "bank/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    When I initialize metadata from sources
    Then the file manifest should include a CSV entry
    And the file manifest should include a transform entry
    And each file entry should have a non-empty content hash

  Scenario: Save and reload metadata preserves state
    Given a clean sources directory with a CSV "bank/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "bank/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    When I initialize metadata from sources
    And I save metadata to disk
    And I reload metadata from disk
    Then the reloaded metadata should match the original

  Scenario: Merge metadata from two devices
    Given device A creates metadata with a sync event "event_a"
    And device B loads device A metadata and adds sync event "event_b"
    When device A merges metadata from device B
    Then device A sync log should contain event "event_a"
    And device A sync log should contain event "event_b"

  Scenario: Sync log records events
    Given a clean sources directory with a CSV "bank/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "bank/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    When I initialize metadata from sources
    Then the sync log should contain a "metadata_built" event
    And the sync log event should include this device ID

  Scenario: Rules are tracked in metadata
    Given a clean sources directory with rules at "bank/savings/_rules.json":
      """
      {"rules":[{"id":"rule-abc","pattern":"Coffee*","payee":"Cafe"}]}
      """
    When I initialize metadata from sources
    Then the metadata should contain rule "rule-abc"
