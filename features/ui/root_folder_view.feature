@wip
Feature: Root folder view aggregates all child folders

  The sidebar's account-set label (e.g. "Richard") represents the current
  folder. Clicking it puts the app in root folder mode: the header shows
  the folder name and the net AUD total summed across every leaf account
  beneath it, and the transactions pane shows the union of transactions
  from every child folder. From there the user can drill into a child
  folder (cash / crypto / equity), and the existing back arrow returns
  to the root folder view via navigation history.

  Scenario: Clicking the account-set label enters root folder mode
    Given the app is running
    When I parse the transactions file "sample-data/example.transactions"
    And I click the account-set label "Richard"
    Then the heading should show "Richard"
    And the heading total should equal the sum of all child folder AUD totals
    And the transactions pane should list transactions from every child folder

  Scenario: Drilling from root into a child folder narrows the view
    Given the app is running
    When I parse the transactions file "sample-data/example.transactions"
    And I click the account-set label "Richard"
    And I click the sidebar folder "cash"
    Then the heading should show "cash"
    And the heading total should equal the cash folder AUD total

  Scenario: Back arrow from a child folder returns to root folder view
    Given the app is running
    When I parse the transactions file "sample-data/example.transactions"
    And I click the account-set label "Richard"
    And I click the sidebar folder "cash"
    And I click the back navigation arrow
    Then the heading should show "Richard"
    And the heading total should equal the sum of all child folder AUD totals
