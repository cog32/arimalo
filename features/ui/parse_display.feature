Feature: Display parsed transactions in a sidebar + table

  Scenario: Parsing a file shows accounts and transaction rows
    Given the app is running
    When I parse the transactions file "sample-data/example.transactions"
    Then the sidebar should include the account "assets:cash:usd"
    When I select the account "assets:cash:usd"
    Then I should see a transaction row for payee "Kraken"
    And I should see a notes value of "Sell BTC"
    And that transaction row should show a deposit of "160.00"

  Scenario: Account declarations create accounts with opening balances
    Given the app is running
    When I parse the transactions file "sample-data/account_opening.transactions"
    Then the sidebar should include the account "assets:cash:bank:cba:smartaccess"
    And the account "assets:cash:bank:cba:smartaccess" should show an amount of "100.00"
