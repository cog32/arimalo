Feature: Two-phase sync protocol between devices

  Scenario: Full sync between two devices with different transactions
    Given device A has sources with a CSV "bank/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And device A has a transform at "bank/_transform.rhai"
    And device B has sources with a CSV "bank/2025-02.csv":
      | Date       | Description | Amount |
      | 2025-02-10 | Lunch       | -12.00 |
    And device B has a transform at "bank/_transform.rhai"
    And both devices have initialized metadata and CAS
    When device A syncs with device B
    Then device A CAS should contain files from both devices
    And device A metadata should reference files from both devices
    And the sync log should record the sync event

  Scenario: Sync transfers only missing files
    Given device A has sources with a CSV "bank/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And device A has a transform at "bank/_transform.rhai"
    And device B has sources with a CSV "bank/2025-02.csv":
      | Date       | Description | Amount |
      | 2025-02-10 | Lunch       | -12.00 |
    And device B has a transform at "bank/_transform.rhai"
    And both devices have initialized metadata and CAS
    And device B has the same CAS blobs as device A
    When device A syncs with device B
    Then no files should be transferred

  Scenario: Sync detects file manifest differences
    Given device A has initialized metadata with 2 files
    And device B has initialized metadata with 3 files
    When comparing manifests between device A and device B
    Then 3 files should be identified as missing from device A

  Scenario: Sync state tracks last sync timestamp
    Given device A has sources with a CSV "bank/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And device A has a transform at "bank/_transform.rhai"
    And device B has sources with a CSV "bank/2025-02.csv":
      | Date       | Description | Amount |
      | 2025-02-10 | Lunch       | -12.00 |
    And device B has a transform at "bank/_transform.rhai"
    And both devices have initialized metadata and CAS
    When device A syncs with device B
    Then device A should record the sync timestamp
    And device A should know about device B
