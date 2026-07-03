Feature: Content-addressed storage for file deduplication and sync

  Scenario: Store a file by content hash
    Given a clean CAS directory
    When I store a file with content "hello world"
    Then the CAS should contain exactly 1 blob
    And retrieving by the content hash should return "hello world"

  Scenario: Duplicate content is deduplicated
    Given a clean CAS directory
    When I store a file with content "same content"
    And I store another file with content "same content"
    Then the CAS should contain exactly 1 blob

  Scenario: Different content produces different hashes
    Given a clean CAS directory
    When I store a file with content "content A"
    And I store another file with content "content B"
    Then the CAS should contain exactly 2 blobs

  Scenario: Integrity check passes for valid blob
    Given a clean CAS directory
    When I store a file with content "verify me"
    Then the integrity check should pass for that blob

  Scenario: Integrity check detects corruption
    Given a clean CAS directory
    When I store a file with content "original"
    And the blob file is corrupted
    Then the integrity check should fail for that blob

  Scenario: Store CSV from sources into CAS and update manifest
    Given a clean sources directory with a CSV "bank/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    When I ingest sources into CAS
    Then the CAS should contain exactly 1 blob
    And the metadata file manifest should reference the CSV by hash

  Scenario: Missing blob is detected during manifest verification
    Given a clean CAS directory
    When I store a file with content "will be deleted"
    And the blob file is deleted
    Then the missing blob should be detected during verification
