Feature: Pipeline regeneration and file-change detection

  Scenario: Regenerate rebuilds ledger from existing sources
    Given a clean sources directory with a CSV and transform
    When I run the pipeline
    And I regenerate the pipeline
    Then the regenerated ledger should match the original

  Scenario: Regenerate is incremental via build cache
    Given a clean sources directory with a CSV and transform
    When I run the pipeline
    And I regenerate the pipeline
    Then the regenerate should report 0 CSVs transformed and all cached

  Scenario: Adding a new CSV triggers a rebuild with new transactions
    Given a clean sources directory with a CSV "bank/savings/2025-01.csv":
      | Date       | Description | Amount |
      | 2025-01-15 | Coffee Shop | -4.50  |
    And a transform at "bank/savings/_transform.rhai" that maps Date/Description/Amount to AUD
    When I run the pipeline for month "202501"
    And a new CSV "bank/savings/2025-02.csv" is added:
      | Date       | Description | Amount |
      | 2025-02-10 | Lunch       | -12.00 |
    And I regenerate the pipeline
    Then the active ledger should contain 2 transactions
    And the active ledger should include payee "Coffee Shop"
    And the active ledger should include payee "Lunch"

  Scenario: Modifying a transform triggers full re-transform on regenerate
    Given a pipeline has been run once with a transform
    When the transform is modified
    And I regenerate the pipeline
    Then the CSV should be re-transformed (not cached)

  # Generated files are derived artifacts. If something outside the pipeline
  # (git checkout, manual edit, another tool) modifies a per-folder
  # ledger.transactions, the next regenerate must detect the divergence and
  # restore the file from the build cache. Without this, a single corrupting
  # write would persist through every subsequent regenerate, with no recovery
  # path short of wiping the cache directory.
  Scenario: Regenerate recovers from external corruption of a per-folder generated ledger
    Given a clean sources directory with a CSV and transform
    When I run the pipeline
    And the per-folder ledger for "bank" is externally clobbered
    And I regenerate the pipeline
    Then the per-folder ledger for "bank" should include payee "CSV Entry"

  # When a folder has BOTH a CSV and a manual.transactions, an incremental
  # regenerate must NOT silently drop the CSV-derived transactions. A previous
  # bug truncated the on-disk ledger to "manual entries only" because:
  #   1. Layer-2 marked the folder unchanged (CSV bytes unchanged).
  #   2. The on_disk_intact path didn't add CSV txns to new_folder_txns.
  #   3. process_csv_files skipped the folder (unchanged).
  #   4. The manual loop pushed manual txns into new_folder_txns[folder].
  #   5. changed_folders included the folder (it was in new_folder_txns).
  #   6. write_pipeline_output wrote new_folder_txns[folder] (manual only).
  #   7. cache.output_hashes was updated to match the truncated file, locking
  #      the corruption in place across subsequent regens.
  # The fix needs both detection (refuse to write a too-short ledger) AND
  # recovery (rebuild from cache.entries when an unchanged folder has manual).
  Scenario: Manual.transactions on an unchanged folder does not truncate the ledger
    Given a clean sources directory with a CSV and transform
    When I run the pipeline
    And a "manual.transactions" file is added to the "bank" folder with one transaction
    And I regenerate the pipeline
    # The bug only triggers on the SECOND regen — the first regen sees the
    # folder as changed (manual.transactions was just added → fingerprint
    # differs → folder NOT in unchanged_folders → process_csv_files runs and
    # populates new_folder_txns correctly). The second regen sees nothing
    # changed → folder IS in unchanged_folders → process_csv_files skips it,
    # but the manual loop still re-adds manual.transactions to
    # new_folder_txns. Without the merge fix, the write would produce
    # "manual only" because CSV txns were never added to new_folder_txns
    # for unchanged folders.
    And I regenerate the pipeline
    Then the per-folder ledger for "bank" should include payee "CSV Entry"
    And the per-folder ledger for "bank" should include payee "Manual Entry"

  # The previous scenario (truncation guard) only catches the bug when the
  # global early-exit fires — sources fingerprint matches, all output hashes
  # match, pipeline returns immediately and the truncated state survives.
  # When some other folder changes, the early-exit doesn't fire and the
  # full pipeline runs over the unchanged-intact folder. In that path,
  # parse_manual unconditionally pushed manual.transactions into
  # new_folder_txns even though the on-disk ledger already had them
  # inlined. The previous mitigation tried to recover by topping up
  # new_folder_txns from cache.entries, but cache.entries is a
  # transform-output cache, not a source of ledger content; relying on
  # it caused doubling whenever the cache count drifted from the on-disk
  # count (e.g. via hide rules that filter at write time).
  #
  # Invariant: an unchanged-intact folder's on-disk ledger never grows
  # or shrinks across regens. cache.entries is used ONLY for incremental
  # CSV-transform reuse, never as a source of ledger rows.
  Scenario: Unchanged folder's manual.transactions is not double-counted when another folder changes
    Given a clean sources directory with a CSV and transform
    When I run the pipeline
    And a "manual.transactions" file is added to the "bank" folder with one transaction
    And I regenerate the pipeline
    # Add a second folder so subsequent regens can mutate it without
    # touching "bank". Mutating "bank2" defeats the global early-exit and
    # forces the full pipeline path over the unchanged-intact "bank".
    And a CSV "bank2/2025-01.csv" is added with one row
    And I regenerate the pipeline
    And a "manual.transactions" file is added to the "bank2" folder with one transaction
    And I regenerate the pipeline
    Then the per-folder ledger for "bank" should include payee "CSV Entry"
    And the per-folder ledger for "bank" should include payee "Manual Entry"
    And the per-folder ledger for "bank" should have 2 transactions
