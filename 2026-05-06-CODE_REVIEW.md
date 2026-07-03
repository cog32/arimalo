# CODE_REVIEW Report — 2026-05-06

Process: `.covid/core/.processes/CODE_REVIEW.md`. Whole-project, **report-only**. Findings deduplicated across the 11 sections; each accepted finding becomes its own unit of work via FIX.md / FEATURE.md → CHANGE_REVIEW.md.

Severities: `blocker` (security / data loss / known-broken) · `high` (correctness gap, missing test on critical path) · `medium` (code quality, boundary leak) · `low` (style, nit).

Snapshot: 56 features / 324 BDD scenarios (316 ✓ / 8 skipped), 249 Rust tests ✓, Vitest coverage 76.1% lines / 66.15% branches.

## Blockers

- `src/render.test.ts:136,153,197,242,282,306,326,345,362,485,660,840` — `npx tsc --noEmit` fails with 12 TS2739 errors: `cgtReport` test fixtures missing `short_term_gains` / `long_term_gains` (added to the type but not the fixtures). Vitest passes because it transpiles without type-checking; CI's contract is silently broken. Per CLAUDE.md "FORCED VERIFICATION", `tsc --noEmit` must be clean — it currently isn't on `main`. → Update fixtures and add `tsc --noEmit` to `scripts/run_unit_tests.sh`.
- `coverage/coverage-summary.json` — Vitest coverage is **76.1% lines / 66.15% branches / 76.56% funcs / 74.13% statements**, all below the 80/70/80/80 thresholds. `scripts/run_unit_tests.sh` would fail those gates, but the script never runs Vitest because `src-tauri/Cargo.toml` exists, so it `exit 0`s after Rust nextest. **TS coverage thresholds are configured but not enforced anywhere.** → Either run Vitest from `run_unit_tests.sh` (and bring coverage up), or remove the misleading thresholds.
- `cucumber.js` + `scripts/run_feature_tests.sh` — `npm run test:bdd` resolves to `test:bdd:parser` (`cargo test --test bdd`); the **17-feature UI cucumber suite under `features/ui/` is never executed** by `run_feature_tests.sh`. `cucumber-js --dry-run` reports `43 scenarios (30 undefined, 13 skipped)`; **11 of 17 feature files have no matching `*.steps.js`** (`add_account`, `aggregation_by_narration`, `amount_decimal_display`, `balances_report`, `cgt_report_link`, `cross_scope_amount_consistency`, `manual_transaction`, `nav_state_restore`, `set_price`, `sidebar_assets_only`, `swap_value_consistency`). MEMORY.md "UI Cucumber tests" feedback explicitly required this to be closed. → Add `cucumber-js` to `run_feature_tests.sh`, implement missing step files, fail CI on undefined scenarios.
- `src-tauri/src/main.rs:309-313, 457-461, 664-669, 725-730, 1578-1588, 1611-1618, 1684-1690, 1726-1729, 2293-2306, 2335-2379` — `WatcherSuppressFlag::suppress()` / `resume()` is not RAII-guarded. Every command uses `?` between the two calls; if any intermediate `?` errors, the flag stays suppressed for the rest of the process, **silently disabling the file watcher until app restart**. → Replace `AtomicBool` with a `SuppressGuard` whose `Drop` calls `resume`, or refcount semaphore.
- `src-tauri/tauri.conf.json:20` — `"csp": null` disables Content-Security-Policy entirely. The WebView renders user-controlled strings (payee, narration, transform output); without CSP this is one mistake away from script injection. → Set `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'` (tighter if achievable).

## High

### Correctness / layering
- `src-tauri/src/main.rs:2544, 2581, 2678, 2711` — all four `generate_*_report*_cmd` Tauri commands re-run `auto_link_equity_swaps` on the already-pipelined ledger after loading from disk. The pipeline already runs it at `processing_pipeline.rs:1832` and persists the result. Per MEMORY.md "fix split trade legs in the pipeline layer, not the CGT engine", this is exactly the layering anti-pattern decided against. → Drop the duplicate calls; reports should consume the ledger as source of truth.
- `src-tauri/src/reports.rs:380-581` — `find_equity_swap_sibling` is ~200 LOC of "compensate for transform output" logic: same-datetime sibling pairing, amount ranking, three fallback matching strategies. Per MEMORY.md "ledger is source of truth", the pipeline should emit linked legs (`swap:txn:` meta refs); reports should look those up. → Move matching upstream; have `resolve_acquisition_cost` / `resolve_sale_proceeds` consult `swap_ref` only.
- `src-tauri/src/main.rs:2895-2913` — `WatcherSuppressFlag` is a single `AtomicBool`, not a counter. Two overlapping suppressors race: A `suppress` → B `suppress` → B finishes & `resume` → watcher re-armed while A's pipeline still running → A's writes trigger an unwanted second pipeline. → Convert to `AtomicUsize` counter.
- `src-tauri/src/main.rs:2978, 3095, 3204` (and 11 more sites) — **no in-flight gate around `run_pipeline`**. Two concurrent commands (e.g. user double-clicks "save rule") will both load sources, transform, and write the same `generated/` ledger files, racing on disk and cache mtimes. Only `MetadataState.store` is protected. → Add a `Mutex<()>` or `AtomicBool` guard at the top of `run_pipeline`; queue or reject overlapping runs.
- `src-tauri/src/main.rs:1733` — `parse_transactions_file` reads an arbitrary file path passed from the frontend. Registered (line 3244) but **zero callsites in `src/`** and zero scenarios. An exposed read-arbitrary-path command is a reach-through waiting to happen. → Remove or restrict path scope.
- `src-tauri/src/main.rs:594-676` — `save_rule` takes 16 positional `Option<String>`/`String` params. TS callsites pass an object so the field-naming contract is enforced only by Tauri's serde glue. → Define `SaveRuleArgs` struct mirroring `src/save-rule-args.ts`.
- `src-tauri/src/processing_pipeline.rs:874` — `&partner.unwrap().datetime` inside auto-link path. If a swap-pair regression produces a partner-less leg, this panics in production rather than warning. → Replace with `match` and a structured warning into `state.warnings`.
- `src-tauri/src/processing_pipeline.rs:1957` — `let _ = fs::write(&warnings_path, ...)`. Pipeline silently swallows IO failures writing `warnings.json`; full disk → user sees no warnings even though rebuild "succeeded". → At minimum `eprintln!` on error or push to `state.warnings`.
- `src-tauri/src/ledger_parser.rs:974-977, 996` — `unwrap()` on CSV-row indices/values inside the prices-CSV parser. Malformed prices CSV panics in production. → Return parse error / collect a diagnostic.
- `src-tauri/src/root_config.rs:55-66` — `set_root` accepts any path the dialog returns; canonicalize-fail falls back to the original (line 60), no `..` rejection, no upper-bound on depth, no ownership check. Combined with auto-creating `sources/`, `generated/`, and `.gitignore` (lines 71-78), a malicious or accidental selection (`~`, `/`, an existing project dir) silently mutates that tree. → Validate path is empty/new or already-known root; reject paths whose canonical form is shorter than `app_data_dir`'s canonical parent.
- `src-tauri/src/csv_transform.rs:157` (and `:559`, `processing_pipeline.rs:3394`, `transform_suggest.rs:276`) — `Engine::new()` used with **no Rhai resource limits**. User-supplied `_transform.rhai` runs in-process with no `set_max_operations` / `set_max_call_levels` / `set_max_string_size` / `set_max_array_size` / `set_max_expr_depths`. A pathological transform DoSes the pipeline. → Apply Rhai sandbox knobs at every Engine construction site.
- `src-tauri/src/csv_transform.rs:462, 520-524, 532-543` — `f64::parse` errors are mapped to a string but `INFINITY` / `NaN` aren't rejected; a transform emitting `"inf"` or `"NaN"` produces a transaction with infinite/nan amount and silently corrupts balances. → Validate `is_finite()` after parse.

### Tests / coverage
- `src-tauri/src/reports.rs:832, 853, 1124, 1145, 1262, 1287` — `generate_cgt_report`, `generate_income_report`, `generate_balances_report` and the `*_range` siblings have **zero in-file `#[test]`** (1,456 LOC, 19 commits in 3 months). All coverage is via BDD with important blind spots: **no scenario covers CGT with fees on a sell**, **no scenario uses a base currency other than AUD**, **no FX-conversion edge cases** (price-graph miss, mid-year rate change). → Add fixture-driven Rust tests next to FIFO inventory + `convert_to_base` calls.
- `src-tauri/features/generated/capital_gains_report.feature:1-310` — 21 scenarios; none cover trading fees on a sell, none assert `income:trading:fees` flowing through, base currency hardcoded AUD. → Add fee-on-disposal and non-AUD base coverage.
- `src/main.ts` — 5,618-LOC UI orchestrator with **121 internal functions, 95 event handlers, zero exports** — no Vitest test can import it. Most-churned file in repo (101 commits in 3 months) effectively has no unit-test layer. → Extract pure helpers to sibling modules; add `main.test.ts` for state machinery.
- `src/smart-search.ts` — coverage 39% / 45.6% / 34.3% / 41.2%. Lines 192-424 untested. Largest single coverage hole. → Add tests for parsing/matching paths.
- `src/startup.ts` — coverage 68.6% / 38.6% / 54.3% / 62.4%. Bootstrap path; failure modes here surface as blank screens or wrong account selection. Lines 280-333 (balance reconciliation) untested.
- `src/currency-totals.ts` — **0% coverage**. Either delete as dead code or write tests.
- `src/filter-bar.ts` — **0% coverage**. Same: prove it's used or remove it.
- `src-tauri/src/main.rs:760, 1176` (`ai_suggest_categorisation`, `ai_suggest_transform`) — no Cucumber, no Vitest, no in-file test; spawns child processes and parses JSON.
- `src-tauri/src/main.rs:2086, 2453, 2471` (`update_account_properties`, `get_tax_config`, `save_tax_config`) — never referenced in features/ or tests/. Tax config flows directly into CGT discount calculations.
- `src-tauri/src/main.rs:2320, 2386` — `save_trade_links_bulk`, `suggest_trade_links_cmd` — no test coverage.
- `src/main.ts:2526, 3172-3218, 3296, 3491` (vault picker, tax settings, balance toggle, show-hidden) — no Cucumber feature despite MEMORY.md "UI Cucumber tests" feedback. Tax-settings is load-bearing for CGT discount.
- `src-tauri/features/architecture/rule_specificity.feature:32, 38, 44, 50` — 4 scenarios skipped (undefined steps). Specificity invariant unverified.
- `src-tauri/features/architecture/ui_over_cli.feature:13, 20, 26` — 3 skipped scenarios. Per MEMORY.md "Extend arimalo-query, don't duplicate", this CLI/UI parity invariant is critical.
- `src-tauri/features/architecture/lightweight_metadata.feature:12` — skipped scenario; "trade-link only regenerates affected folder" architectural claim unverified.

### Stringly-typed boundaries
- `src/main.ts` (89 raw `invoke("...")` calls, 78 distinct command names across `src/`, no `commands.ts` constants module) — typo in any string is a silent runtime failure. Same on the Rust side: 24 raw `emit("...")` calls covering 6 distinct event names duplicated into `src/main.ts:5528, 5538, 5545, 5559, 5566` and `src/startup.ts:135`. → Centralise in `src/commands.ts` + `src/events.ts` (and a `pub const` per name in Rust).
- `src/main.ts:2530, 2544, 3159, 3208, 3493, 3602, 3633, 3943, 4041, 4125, 4142, 4174, 4203, 5040` — 14 `await invoke("xxx", ...)` callsites without the generic type parameter; each silently degrades return to `unknown`. Worst: `set_root_dir` (×2), `save_relay_config`, `ai_suggest_categorisation`, `ai_suggest_transform`. → Type each callsite; consider a typed wrapper module.
- `src/startup.ts:46-62` — `StartupState` declares `account_properties: Record<string, any>`, `diagnostics: any[]`, `devices?: any[]`, `syncLog?: any[]`, `tradeLinks: any[]`, `tradeSuggestions: any[]`, `relayConfig?: any`, `accountGaps?: any[]`. Types exist (`SyncEvent`, `DeviceInfo`, `TradeLink`, `TradeSuggestion`) but aren't imported. → Replace each `any[]` with the corresponding type.
- `src/main.ts:5487-5493` — `_normalStartup(state as any, { invoke, listen: listen as any, ..., onPipelineEvent: (payload: any) => ... })` erases the entire `AppState` type at the wire then re-imports as `any`. → Define `PipelineRebuiltEvent` and remove `as any`.

## Medium

### Architecture & complexity
- `src-tauri/src/main.rs` — **3,420 LOC** single Tauri command surface file. With `processing_pipeline.rs` (4,251 LOC) and `rules.rs` (2,314 LOC), these are the largest non-vendor files. CLAUDE.md "Step 0" pass overdue.
- `src-tauri/src/processing_pipeline.rs:1289-1982` — `run_pipeline` is **694 LOC** in one fn with 13+ phases. Each phase already has a name in the timing log. → Extract `load_caches`, `process_sources`, `apply_rules`, `write_outputs`.
- `src-tauri/src/processing_pipeline.rs:433-911` — `auto_link_equity_swaps`, **479 LOC**, mixes detection + sibling-pairing + hash-resolution + meta-writing.
- `src-tauri/src/main.rs:1176-1475` — `ai_suggest_transform`, **300 LOC** end-to-end (CSV scanning, prompt assembly, AI call, parsing, emit).
- `src-tauri/src/main.rs:776-1005` — `ai_suggest_categorisation`, **246 LOC** with similar shape. Move both to a new `ai_suggest.rs` module.
- `src/main.ts:4234-4842` — `attachRuleAndTableHandlers`, **609 LOC**. Worst single function in TS code. Whole `attach*Handlers` cluster (~2,200 LOC) is a structural smell — consider an event-table pattern.
- `src/main.ts:3290-3608, 3610-3906, 2998-3288, 4005-4232, 4844-5050` — five additional `attach*Handlers` functions in 200-320 LOC each.
- `src-tauri/src/processing_pipeline.rs:1984` — `process_csv_files` cognitive complexity 22/15.
- `src-tauri/src/processing_pipeline.rs:2431` — `write_pipeline_output` cognitive complexity 25/15 (highest in repo).
- `src-tauri/src/reports.rs:380, 853` — `find_equity_swap_sibling` 22/15; `generate_cgt_report_range` 22/15 + 254 LOC.
- `src-tauri/src/rules.rs:166-425` — `amount_matches`, 260 LOC.
- `src-tauri/src/main.rs:2521-2721` — five `generate_*_report*_cmd` commands repeat resolve_dir → cached-short-circuit → load_active_ledger → PriceGraph::load → hidden-filter → auto_link → reports::* in five near-identical blocks. → Extract `prepare_report_inputs(app, account_set) -> ReportInputs`.
- `src-tauri/src/bin/arimalo_*.rs` — `jscpd` flags ~600 lines duplicated across 9 CLI binaries (largest cluster: `arimalo_regenerate.rs:22-62` ↔ `arimalo_reports.rs:8-48`). The `platform_app_data_dir` / `resolve_sources_dir` / `resolve_generated_dir` / `now_yyyymm` / arg-parsing helpers belong in `root_config` or a new `cli_common` module.

### Stale / dead surface
- `src-tauri/src/main.rs:3243-3323` (invoke handler list) — 13 commands registered but never called from TS or BDD: `get_plugin_config`, `get_report_cmd`, `get_show_hidden`, `import_rules_csv`, `load_generated_ledger`, `load_pipeline_metadata`, `merge_metadata`, `parse_transactions_file`, `process_imports_cmd`, `rename_account_folder`, `save_plugin_config_cmd`, `save_plugin_secrets_cmd`, `sync_with_remote`. → Delete or wire up.
- `src-tauri/src/probe_test.rs.tmp` — leftover scratch file (892 bytes, 2026-04-28). → Delete.

### State / concurrency
- `src-tauri/src/main.rs:1802-1833` — `spawn_report_generation` fire-and-forget. Generation counter prevents `emit` re-firing but two threads still write `generated/reports/` in parallel; can corrupt markdown. → Mutex around report-write or cancellable threads.
- `src/main.ts:5495-5499` — `pipeline-rebuilt` payload mutates `accountSetMap` etc. but `state.parse`, `searchFilteredTransactions`, `transactionValues`, `tradeSuggestions` survive untouched until `loadGeneratedLedger` resolves. 200-2000ms window where sidebar shows new accounts but txn list shows old data.
- `src/main.ts:5394-5395` — `txExpandedGroups` / `txExpandedRows` not cleared by `pipeline-rebuilt`. If an expanded txn is regrouped or removed, the expansion key lingers and silently re-expands a different row.
- `src-tauri/src/main.rs:2978-2999, 2870-2875, 3097-3101, 147-156` — four rebuild paths each clone the entire `ParseResult` per account-set. On a 46K-txn vault this is multi-MB cloned per rebuild. → `Arc<ParseResult>`. Also: brief race between `invalidate()` and the populate loop.
- `src-tauri/src/main.rs:1747, 3159` and `src/main.ts:5414, 5249` — `showHidden` lives in both `state.showHidden` (TS) and `ShowHiddenState(AtomicBool)` (Rust); defaults match so benign, but Rust resets to false on restart while user might expect persistence.
- `src/main.ts:276-282` and `:3002` — `NavEntry` saved to localStorage AND `state.selectedAccountSet` persisted to a separate localStorage key. Two parallel persistence schemes.

### Type safety
- 23 `: any` / `as any` hits in production TS, concentrated in `src/startup.ts` and `src/main.ts:5487-5493`. ESLint config has `@typescript-eslint/no-explicit-any: warn` not `error`.
- `src/main.ts:891, 893, 908, 912, 3142-3143` — `reportType === "cgt" / "income" / "balances"` repeated. → `type ReportType = "cgt" | "income" | "balances"` once.
- `src/main.ts:1459, 1469, 2468, 2469, 2883, 2893, 2896, 2952, 2954, 2968, 3498` — `state.sidebarView === "accounts" / "reports" / "plugins" / "rule-editor"`. → `SidebarView` union.
- `src/account-input.ts:187, 247` and `src/main.ts:1999, 2074-2075` — ad-hoc string discriminants (`"revert" / "cancel" / "single" / "new" / "existing" / "direct" / "from-date"`). Each a discriminated-union candidate.
- `src-tauri/src/main.rs:2101-2128` — `get_display_config` returns `serde_json::Value`; TS types as `DisplayConfig`. → Return the typed struct.

### Error handling
- `src-tauri/src/relay_client.rs:50-260` (~18 sites) — every error `.map_err(|e| format!("...{}: {}", ..., e))` flattens stack/source chain. → `anyhow::Context`.
- `src-tauri/src/processing_pipeline.rs:3359, 3363` — `dest_dir.canonicalize().ok()`; if canonicalize fails, the safety check that prevents writing outside `sources_dir` is silently disabled.
- `src-tauri/src/main.rs:520-523` — chained `.ok()` swallows two errors when reading config. Malformed `config.json` → app starts as if no root were configured, no message.
- `src-tauri/src/main.rs:776-3104` — 25+ `let _ = app.emit(...)`. → Single helper that logs first failure.
- `src/main.ts:1020, 4693, 4734, 4788` — `} catch (e) { console.warn("trade suggestions failed:", e); }`. User has no idea suggestions are stale.
- `src-tauri/src/transform_suggest.rs:243-246` — three `.ok()?` chained against the LLM HTTP response. 500, non-JSON body, parse fail all collapse to "no suggestion" with no diagnostic.
- `src-tauri/src/plugins.rs:138, 142, 210` — `let _ = std::fs::create_dir_all(...)` / `let _ = std::fs::write(...)`. Failure to persist `last_run.json` silently breaks "last successful run" indicator.
- `src/main.ts:801, 975, 3210, 3473` (and ~15 more) — `console.error(...)` + return without surfacing to user.

### Debug noise
- `src/main.ts:1019, 4663, 4667, 4693, 4734, 4788` — `console.warn("[trade-suggest]...")` / `"[trade-link]..."` debug breadcrumbs ~60 days old (committed `b296f262` 2026-03-06). Tee'd to in-app debug panel, but trigger on every click. → `pushDebug("info", ...)` directly.
- `src-tauri/src/processing_pipeline.rs:1296, 1343, 1877, 1926` — pipeline timing `eprintln!`s unconditional. → Gate behind `if cfg!(debug_assertions)` or env var.
- `src-tauri/src/main.rs:985-1000, 1463-1470, 2280-2296, 2369-2373` — `[trade-link]`, AI-suggest debug `eprintln!`s. → `log::debug!` or feature-gate.

### Untested CLIs / commands
- `src-tauri/src/bin/arimalo_classify.rs, arimalo_gaps.rs, arimalo_import.rs, arimalo_issues.rs, arimalo_migrate_rules.rs, arimalo_repair_trade_links.rs, arimalo_reports.rs` — 7 CLI binaries with no integration test (only `arimalo_query_cli.rs` exists).
- `src-tauri/src/main.rs:1502-1733, 1862-1973, 3027-3140` — `parse_transactions_file`, `collect_issues_cmd`, `get_rules`, `update_rule`, `delete_rule`, `import_rules_csv`, sync/relay command wrappers, plugin commands — library logic IS BDD-tested but command wrappers aren't.
- `src-tauri/src/build_cache.rs:1-450` — 0 in-file tests, 8 commits in 3 months. No architecture feature exercises cache invalidation when SHA256 matches but mtime/size differs.
- `src-tauri/src/lib.rs:8` — `parse_date_to_iso` 0 tests; 4 distinct branches, used by OFX import.
- `src-tauri/src/relay_client.rs:1-264` — 0 in-file tests; HTTP retry/timeout/request-shaping branches unit-untested.
- `src-tauri/src/report_templates.rs:1-477` — 0 in-file tests; CSV export of CGT/income/balances has 3 in-file tests in `report_csv.rs` but no fixture-driven test for malformed numbers, embedded commas in narrations, locale-specific decimals.

### Documentation drift
- `CLAUDE.md:33-34, 43, 59` — references `playwright.config.ts`, `e2e/` directory, `npm i -D @playwright/test`. Neither file/dir exists. README still references `tauri-driver`/Appium-mac2 (`README.md:155-163`). → Reconcile to a single current toolchain.
- `.covid/core/.processes/CHECKOUT_WORKTREE.md:30, 110, 113, 116, 119` and `.covid/core/.processes/FIND_WORK.md:40` — reference `.venv/bin/python scripts/validate_branch_name.py`. Neither exists.
- `.covid/core/.processes/CODE_REVIEW.md:7` — references `scripts/agents/code_review_agent.py`; path does not exist.
- `MEMORY.md` "Active Refactoring – main.ts refactoring – branch refactor-main (5/14 done, 9 remaining)" — staleness indicator; current branch is `fix/pipeline/ignore-rule-zeros-both-legs`. Likely outdated.

### Outdated dependencies / supply chain
- `package.json` (npm outdated): `vite 5.4.21 → 8.0.10` (3 majors behind; security-relevant), `jsdom 28 → 29`, `marked 17 → 18`, `typescript 5.9 → 6.0`, `@cucumber/cucumber 11 → 12`, `@types/node 22 → 25`. → Schedule major upgrade pass starting with vite.
- `src-tauri/Cargo.toml:21` — `ureq = "2"` on outbound HTTP (relay client + LLM transform suggestion). `cargo-audit` not installed; no automated CVE check. → Install `cargo-audit` in CI; gate PR on `cargo audit`.
- `src-tauri/src/transform_suggest.rs:193` — `ARIMALO_LLAMA_URL` defaults to `http://localhost:8080`; request sent without TLS verification. If user sets to remote URL, prompts (containing sample CSV) leave device in cleartext. → Document HTTPS-only or refuse non-https schemes.
- `src-tauri/src/csv_transform.rs:147-152` — `csv::Reader::from_path` reads entire CSV; no upper-bound on row count, row width, per-cell length. Multi-GB malformed CSV in `sources/` OOMs. → `max_rows` / `max_cell_bytes` cap.

## Low

- `src-tauri/tests/bdd.rs:5467` — compiler warning `unused variable: sources` in build log. Per CLAUDE.md "FORCED VERIFICATION", warnings should be clean. → Prefix `_sources` or remove.
- `src/account-utils.test.ts:8-72`, `src/sidebar-accounts.test.ts:13`, `src/query-sort.test.ts:8-43` — happy-path-only suites with no negative assertions.
- `src/apply-pipeline.ts:92-112`, `src/render.ts:438-441/557-560`, `src/mutation-queue.ts:51,95`, `src/virtual-scroll.ts:37-61` — uncovered error/flush/page-boundary branches.
- `package.json` `"test:bdd"` → `"test:bdd:parser"` — naming misleading; UI cucumber suite reachable only via `test:bdd:ui` which no script invokes. → Rename or compose.
- No top-level `.eslintrc*` (only `.covid/.eslintrc.js` in submodule); `package.json` has no `lint` script — eslint effectively unrun. CLAUDE.md "FORCED VERIFICATION" requires `eslint . --quiet` runnable.
- Dead-code tooling absent (`cargo machete`, `cargo +nightly udeps`, `ts-unused-exports`).
- `scripts/run_e2e_tests.sh` exits 0 with "Skipping E2E (macOS): Appium mac2 driver is not installed." → Confirm CI on Linux runs E2E.
- `src/main.ts:1019` — `console.warn` for info-level breadcrumb. → Demote to `console.log` or drop.
- `src-tauri/capabilities/default.json:5` — minimal allowlist (`core:default` + three `dialog:*`). Good. → Add a comment so a future contributor doesn't add `fs:default` casually.
- No `_rules.json` carries catch-all `"*"` patterns; `save_rule` rejects them at `main.rs:616-621`. Compliant.
- `npx madge --circular src/` finds none. No TS circular imports.
- No `unimplemented!()` / `todo!()` in non-test code.
- TS catch blocks all use implicit `unknown`. No `@ts-ignore` / `@ts-expect-error` in production. Rust commands consistently use `Result<T, String>`.
- `src-tauri/features/generated/plugins.feature:77, 135` — fixture strings `"api_key": "secret123"` look like real credentials to scanners. → `"<redacted>"`.
- `src-tauri/src/transform_suggest.rs:119, 120, 149, 150` — `// TODO:` and `FIXME_*_COLUMN` are intentional placeholders in *generated* Rhai output, not aged dev TODOs.

## Tooling status

- ✓ ran: `npx vitest run --coverage`, `cargo test --lib`, `cargo test --test bdd`, `npx jscpd`, `npx madge`, `npx cucumber-js --dry-run`, clippy.
- ✗ not installed (skipped, recommend installing): `cargo-audit`, `cargo-machete`, `cargo-outdated`, `ts-unused-exports`, `npx eslint` at top level.

## Suggested ordering

1. **Blockers** — get to green CI: `tsc --noEmit`, coverage gates, UI cucumber wiring, watcher RAII guard, CSP.
2. **High correctness/layering** — auto_link reach-through, run_pipeline gating, root_config validation, Rhai sandbox.
3. **High test gaps** — CGT fees / FX scenarios, `main.ts` extractability.
4. **Medium** — file splits, stale dead surface, debug noise pruning.
5. **Low** — stylistic.

Each accepted finding lands as its own unit of work via FIX.md / FEATURE.md → CHANGE_REVIEW.md.
