Feature: Update prices on startup setting

  The Plugins view exposes a checkbox that controls whether the app runs the
  daily price-backfill plugins automatically on launch.

  Scenario: The Plugins view shows the startup checkbox and the update-now button
    Given the app is running
    When I switch to the plugins view
    Then I should see the update prices on startup checkbox
    And I should see the update prices now button
