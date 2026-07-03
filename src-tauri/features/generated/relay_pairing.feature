Feature: Relay server device pairing

  Scenario: Initiate pairing creates a group with a 6-digit code
    Given a running relay server
    When device A initiates pairing
    Then a group ID and 6-digit pairing code are returned
    And the pairing code expires after 10 minutes

  Scenario: Join pairing with valid code returns the group ID
    Given a running relay server
    And device A has initiated pairing
    When device B joins with the pairing code
    Then device B receives the same group ID as device A

  Scenario: Join pairing with invalid code fails
    Given a running relay server
    When device B joins with an invalid pairing code
    Then the join should fail with not found

  Scenario: Pairing code is single-use
    Given a running relay server
    And device A has initiated pairing
    And device B has joined with the pairing code
    When device C tries to join with the same pairing code
    Then the join should fail with not found

  Scenario: Expired pairing code is rejected
    Given a running relay server
    And device A has initiated pairing with a 0-second TTL
    When device B joins with the expired pairing code
    Then the join should fail with not found
