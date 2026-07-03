# Code Improvements

Identified during CHANGE_REVIEW of `feat/tax-reports` branch.

## High Priority

- [x] 1. **Extract `resolve_account_set_dir()` helper in main.rs** — Extracted `resolve_set_dir()`. 26 instances replaced. -130 lines, +34 lines. All tests pass.

- [x] 2. **Extract `build_pipeline_response()` helper in main.rs** — Extracted helper. 15 call sites replaced. -93 lines, +21 lines. All tests pass.

- [x] 3. **Replace hand-rolled `current_yyyymm()` with chrono** — Added chrono as direct dependency, replaced 13 lines of magic-number date math with 2 lines. All tests pass.

- [x] 4. **Break `run_pipeline()` into smaller functions** — Extracted 4 functions via PipelineState struct: process_csv_files(), process_ofx_files(), rebuild_owner_accounts(), write_pipeline_output(). All tests pass.

- [x] 5. **Unify `scan_csv_recursive()` and `scan_ofx_recursive()`** — Unified into generic `scan_source_files()` with extension and skip_underscore_prefix params. -37 lines, +17 lines. All tests pass.

- [x] 6. **Extract common `update_account_property()` in processing_pipeline.rs** — Both now delegate to generic `update_account_property(kind, value)`. -73 lines, +25 lines. All tests pass.

- [x] 7. **Refactor `resolve_acquisition_cost()` / `resolve_sale_proceeds()` in reports.rs** — Extracted `resolve_annotation_value()` and `find_equity_swap_sibling()` helpers. -90 lines, +72 lines. All tests pass.

- [x] 8. **Replace `std::ptr::eq` transaction comparison in reports.rs** — Replaced with datetime + `postings.as_ptr()` comparison in `find_equity_swap_sibling()`. Done as part of #7.

- [x] 9. **Break `render()` into component functions in main.ts** — Extracted renderVaultPicker(), renderSidebar(), renderTransactionRows(), renderModals(). ~600 lines moved to component functions. All tests pass.

- [x] 10. **Fix `fyYearOptions()` missing `reportYears` param in render.ts** — Added `state.reportYears` as second argument to both CGT and income report year selectors. All tests pass.

## Medium Priority

- [x] 11. **Replace `.lock().unwrap()` with parking_lot::Mutex in main.rs** — Swapped std::sync::Mutex for parking_lot::Mutex. 21 `.unwrap()` calls removed. No poisoning risk. All tests pass.

- [x] 12. **Extract hardcoded path strings to constants in processing_pipeline.rs** — Defined 9 constants, replaced 27 string literals. All tests pass.

- [x] 13. **Define `FALLBACK_ACCOUNT` constants** — Added `FALLBACK_ASSET_ACCOUNT` and `FALLBACK_EXPENSE_ACCOUNT` in lib.rs, replaced across csv_transform, processing_pipeline, ofx_parser. All tests pass.

- [x] 14. **Add path traversal validation** — Added `validate_folder_name()` in processing_pipeline.rs, called from all Tauri commands that accept `account_folder`. Rejects `..`, leading `/` or `\`. All tests pass.

- [x] 15. **Centralize floating-point epsilon in reports.rs** — Defined `const EPSILON: f64 = 1e-10;`, replaced all scattered `1e-10` and `1e-8` comparisons. All tests pass.

- [x] 16. **Fix FY label to use TaxConfig** — Added `fyLabel()` helper in render.ts that computes month names from `taxConfig.financial_year_end_month`. Replaced hardcoded "Jul-Jun" in both CGT and income report headers. Added 4 tests. All tests pass.

- [x] 17. **Unify date parsing between csv_transform and ofx_parser** — Added shared `parse_date_to_iso()` in lib.rs accepting both YYYY-MM-DD and YYYYMMDD formats with validation. Replaced blind `[0..10]` slicing in csv_transform and `parse_ofx_date()` in ofx_parser. All tests pass.

- [x] 18. **Fix OFX byte-level slicing on uppercased content** — Added ASCII sanitization at top of `parse_ofx()` using `Cow<str>`. Non-ASCII chars replaced with '?' to prevent byte-position mismatch between uppercased search and original content indexing. All tests pass.

- [x] 19. **Fix `fy_year.parse().unwrap_or(2025)` in report_templates.rs** — Changed `fy_label()` in report_templates.rs to return `Result` with proper error. Changed `fy_date_range()` in reports.rs to use `expect()` with descriptive message. All tests pass.

- [x] 20. **Split `collectIssues()` in main.ts** — Extracted 6 functions: collectParseErrors(), collectUncategorised(), collectPipelineWarnings(), collectAccountGaps(), collectUnverifiedBalances(), collectTradeSuggestions(). collectIssues() now just orchestrates. All tests pass.

- [x] 21. **Extract shared inline-edit function in main.ts** — Extracted `startCellEdit()` helper handling input creation, keyboard events, and blur save. Both payee/category and commodity editing now use it. -60 lines. All tests pass.

- [x] 22. **Extract CSV content builder in bdd.rs** — Added `table_to_csv()` helper, replaced 4 duplicated header+row join blocks. All tests pass.

- [x] 23. **Parameterize Device A/B setup in bdd.rs** — Merged 3 pairs (sources CSV, transform, local metadata) into parameterized `(A|B)` regex steps. Added `set_device_sources()` and `device_sources()` helpers on LedgerWorld. -35 lines. All tests pass.

## Low Priority

- [x] 24. **Add warning on silent JSON parse failure in rules.rs** — Added `eprintln!` warnings for both file read and JSON parse failures in `RulesFile::load()`. All tests pass.

- [x] 25. **Remove unused `_now_yyyymm` parameter in main.rs** — Removed from both Tauri command and frontend caller. All tests pass.

- [x] 26. **Extract `"000000"` magic string in main.rs** — Defined `YYYYMM_UNUSED` constant. All tests pass.

- [x] 27. **Make currency priority configurable in main.ts** — `pickTreeTotal()` now accepts optional `preferredCurrencies` array, defaults to `["USD", "AUD"]`. All tests pass.

- [x] 28. **Extract decimal precision constants in render.ts** — Added `CURRENCY_DECIMALS` (2) and `QUANTITY_DECIMALS` (4), replaced all 14 `.toFixed()` calls. All tests pass.

- [x] 29. **Remove identity wrapper functions in main.ts** — Removed unused `renderCgtReport`/`renderIncomeReport` wrappers and their imports (dead code). All tests pass.

- [x] 30. **Canonicalize paths in root_config.rs** — `set_root()` now canonicalizes the path before storing, preventing duplicates from symlinks or trailing slashes. Falls back to original path if canonicalization fails. All tests pass.
