Feature: arimalo-dedupe CLI removes overlapping bank-export duplicates

  The CLI scans a sources folder and, per-subfolder, identifies records that
  appear in multiple export files. OFX files are deduplicated by FITID; CSV
  files are deduplicated by normalized row content (trim + collapsed whitespace).
  When the same key appears in more than one file, the file covering the widest
  date span keeps the record, and narrower files are rewritten with that record
  stripped. Dry-run is the default; --apply rewrites files in place and moves
  any file left empty into a timestamped .dedupe-archive folder.

  Background:
    Given a temporary sources directory

  Scenario: OFX triple-overlap dry-run reports duplicates without changing files
    Given an OFX file "wide.ofx" in folder "cba/smartaccess" with DTSTART "20240101" DTEND "20251231" and FITIDs "A,B,C,D"
    And an OFX file "mid.ofx" in folder "cba/smartaccess" with DTSTART "20240401" DTEND "20250617" and FITIDs "B,C"
    And an OFX file "narrow.ofx" in folder "cba/smartaccess" with DTSTART "20250101" DTEND "20250228" and FITIDs "C"
    When I run arimalo-dedupe without --apply
    Then the output reports that "wide.ofx" is the canonical file for folder "cba/smartaccess"
    And the output reports 2 duplicate STMTTRN blocks would be stripped from "mid.ofx"
    And the output reports 1 duplicate STMTTRN block would be stripped from "narrow.ofx"
    And the file "cba/smartaccess/mid.ofx" still contains FITID "B"
    And the file "cba/smartaccess/mid.ofx" still contains FITID "C"
    And the file "cba/smartaccess/narrow.ofx" still contains FITID "C"

  Scenario: OFX triple-overlap --apply rewrites narrower files and leaves the canonical file untouched
    Given an OFX file "wide.ofx" in folder "cba/smartaccess" with DTSTART "20240101" DTEND "20251231" and FITIDs "A,B,C,D"
    And an OFX file "mid.ofx" in folder "cba/smartaccess" with DTSTART "20240401" DTEND "20250617" and FITIDs "B,C"
    And an OFX file "narrow.ofx" in folder "cba/smartaccess" with DTSTART "20250101" DTEND "20250228" and FITIDs "C"
    When I run arimalo-dedupe with --apply
    Then the file "cba/smartaccess/wide.ofx" still contains FITIDs "A,B,C,D"
    And the file "cba/smartaccess/mid.ofx" no longer contains FITID "B"
    And the file "cba/smartaccess/mid.ofx" no longer contains FITID "C"
    And the file "cba/smartaccess/narrow.ofx" has been archived

  Scenario: CSV dedup keys on whitespace-normalized rows
    Given a CSV file "wide.csv" in folder "cba/cdia" with header "Date,Amount,Memo" and rows:
      | 2024-10-01,100.00,Coffee  Shop |
      | 2024-10-02,50.00,Bookshop      |
      | 2024-11-15,200.00,Groceries    |
    And a CSV file "narrow.csv" in folder "cba/cdia" with header "Date,Amount,Memo" and rows:
      | 2024-10-01, 100.00 , Coffee Shop |
      | 2024-10-15,75.00,Cinema          |
    When I run arimalo-dedupe with --apply
    Then the file "cba/cdia/wide.csv" still contains 3 data rows
    And the file "cba/cdia/narrow.csv" contains 1 data row
    And the remaining row in "cba/cdia/narrow.csv" matches "Cinema"

  Scenario: Equal date spans break tie by file size
    Given an OFX file "small.ofx" in folder "ubank/savings" with DTSTART "20240101" DTEND "20240630" and FITIDs "X,Y"
    And an OFX file "big.ofx" in folder "ubank/savings" with DTSTART "20240101" DTEND "20240630" and FITIDs "X,Y,Z,W,Q"
    When I run arimalo-dedupe with --apply
    Then the file "ubank/savings/big.ofx" still contains FITIDs "X,Y,Z,W,Q"
    And the file "ubank/savings/small.ofx" has been archived

  Scenario: --apply writes dedupe-report.json describing every change
    Given an OFX file "wide.ofx" in folder "cba/smartaccess" with DTSTART "20240101" DTEND "20251231" and FITIDs "A,B"
    And an OFX file "narrow.ofx" in folder "cba/smartaccess" with DTSTART "20250101" DTEND "20250228" and FITIDs "B"
    When I run arimalo-dedupe with --apply
    Then a dedupe-report.json exists in the archive folder
    And the report lists "cba/smartaccess/narrow.ofx" as archived
    And the report records 1 FITID dropped for kind "ofx"
