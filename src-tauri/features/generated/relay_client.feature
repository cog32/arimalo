Feature: Relay client HTTP sync

  Scenario: Client pairs with relay and syncs metadata
    Given a relay server running in background
    And device A has local metadata and CAS
    When device A pairs via the relay client
    And device A syncs with the relay
    Then the relay should have device A metadata

  Scenario: Two clients sync through relay
    Given a relay server running in background
    And device A has local metadata with a file "file_a.csv"
    And device B has local metadata with a file "file_b.csv"
    And both devices have paired with the relay
    When device A syncs with the relay
    And device B syncs with the relay
    And device A syncs with the relay again
    Then device A should have the blob from device B
    And device A metadata should reference both files

  Scenario: Client handles relay server being unavailable
    Given no relay server is running
    When device A tries to sync with the relay
    Then the sync should fail with a connection error
