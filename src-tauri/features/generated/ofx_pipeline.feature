Feature: OFX file import pipeline

  Scenario: OFX file parsed into transactions
    Given a clean sources directory with an OFX file "bank/savings/statement.ofx":
      """
      OFXHEADER:100
      DATA:OFXSGML

      <OFX>
      <BANKMSGSRSV1>
      <STMTTRNRS>
      <STMTRS>
      <CURDEF>AUD
      <BANKTRANLIST>
      <STMTTRN>
      <TRNTYPE>DEBIT
      <DTPOSTED>20250115
      <TRNAMT>-4.50
      <FITID>TXN001
      <MEMO>Coffee Shop
      </STMTTRN>
      <STMTTRN>
      <TRNTYPE>CREDIT
      <DTPOSTED>20250116
      <TRNAMT>3500.00
      <FITID>TXN002
      <MEMO>Salary
      </STMTTRN>
      </BANKTRANLIST>
      </STMTRS>
      </STMTTRNRS>
      </BANKMSGSRSV1>
      </OFX>
      """
    And an accounts file declaring "assets:bank:savings AUD"
    When I run the pipeline for month "202501"
    Then the active ledger should contain 2 transactions
    And the active ledger should include narration "Coffee Shop"
    And the active ledger should include narration "Salary"

  Scenario: OFX transactions use FITID-based IDs
    Given a clean sources directory with an OFX file "bank/savings/statement.ofx":
      """
      OFXHEADER:100
      DATA:OFXSGML

      <OFX>
      <BANKMSGSRSV1>
      <STMTTRNRS>
      <STMTRS>
      <CURDEF>AUD
      <BANKTRANLIST>
      <STMTTRN>
      <TRNTYPE>DEBIT
      <DTPOSTED>20250115
      <TRNAMT>-10.00
      <FITID>UNIQUE123
      <MEMO>Test Transaction
      </STMTTRN>
      </BANKTRANLIST>
      </STMTRS>
      </STMTTRNRS>
      </BANKMSGSRSV1>
      </OFX>
      """
    And an accounts file declaring "assets:bank:savings AUD"
    When I run the pipeline for month "202501"
    Then the active ledger should contain a transaction with ID starting with "ofx-"
    And running the pipeline again should produce the same transaction IDs

  Scenario: OFX uses account from folder lookup
    Given a clean sources directory with an OFX file "mybank/checking/statement.ofx":
      """
      OFXHEADER:100
      DATA:OFXSGML

      <OFX>
      <BANKMSGSRSV1>
      <STMTTRNRS>
      <STMTRS>
      <CURDEF>USD
      <BANKTRANLIST>
      <STMTTRN>
      <TRNTYPE>DEBIT
      <DTPOSTED>20250115
      <TRNAMT>-25.00
      <FITID>CHK001
      <MEMO>Groceries
      </STMTTRN>
      </BANKTRANLIST>
      </STMTRS>
      </STMTTRNRS>
      </BANKMSGSRSV1>
      </OFX>
      """
    And an accounts file declaring "assets:mybank:checking USD"
    When I run the pipeline for month "202501"
    Then the transaction should use account "assets:mybank:checking"

  Scenario: OFX and CSV coexist in same directory
    Given a clean sources directory with a CSV "bank/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-10 | CSV Item    | -20.00 |
    And a transform at "bank/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    And an OFX file "bank/savings/statement.ofx":
      """
      OFXHEADER:100
      DATA:OFXSGML

      <OFX>
      <BANKMSGSRSV1>
      <STMTTRNRS>
      <STMTRS>
      <CURDEF>AUD
      <BANKTRANLIST>
      <STMTTRN>
      <TRNTYPE>DEBIT
      <DTPOSTED>20250115
      <TRNAMT>-15.00
      <FITID>OFX001
      <MEMO>OFX Item
      </STMTTRN>
      </BANKTRANLIST>
      </STMTRS>
      </STMTTRNRS>
      </BANKMSGSRSV1>
      </OFX>
      """
    And an accounts file declaring "assets:bank:savings AUD"
    When I run the pipeline for month "202501"
    Then the active ledger should contain 2 transactions
    And the active ledger should include payee "CSV Item"
    And the active ledger should include narration "OFX Item"

  Scenario: OFX files processed via import workflow
    Given a clean sources directory
    And an accounts file declaring "assets:bank:savings AUD"
    And an OFX file in imports at "bank/savings/imports/statement.ofx":
      """
      OFXHEADER:100
      DATA:OFXSGML

      <OFX>
      <BANKMSGSRSV1>
      <STMTTRNRS>
      <STMTRS>
      <CURDEF>AUD
      <BANKTRANLIST>
      <STMTTRN>
      <TRNTYPE>DEBIT
      <DTPOSTED>20250120
      <TRNAMT>-50.00
      <FITID>IMP001
      <MEMO>Imported Item
      </STMTTRN>
      </BANKTRANLIST>
      </STMTRS>
      </STMTTRNRS>
      </BANKMSGSRSV1>
      </OFX>
      """
    When I process imports for account "bank/savings"
    Then the OFX file should be moved to "bank/savings/statement.ofx"
    And the active ledger should contain 1 transactions
    And the active ledger should include narration "Imported Item"
