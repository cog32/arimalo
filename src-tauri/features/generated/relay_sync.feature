Feature: Relay server metadata and blob sync

  Scenario: Upload and download metadata through relay
    Given a running relay server with a paired group
    When device A uploads metadata to the relay
    And device B downloads metadata from the relay
    Then device B should receive device A metadata

  Scenario: Server-side metadata merge
    Given a running relay server with a paired group
    And device A has uploaded metadata with event "event_a"
    When device B uploads metadata with event "event_b"
    And device A downloads metadata from the relay
    Then device A metadata should contain both events

  Scenario: Upload and download blobs through relay
    Given a running relay server with a paired group
    When device A uploads a blob with content "hello relay"
    Then the relay blob list should include that hash
    And device B can download the blob and get "hello relay"

  Scenario: List remote blobs returns all uploaded hashes
    Given a running relay server with a paired group
    When device A uploads 3 blobs to the relay
    Then the relay blob list should contain 3 hashes
