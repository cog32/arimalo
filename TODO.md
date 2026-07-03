# TODO — reports as JSON-driven artifacts

The pipeline now writes per-FY JSON snapshots (`reports/cgt-{fy}.json`,
`income-{fy}.json`, `balances-{fy}.json`) on every rebuild, and the FY-form
Tauri commands read the JSON instead of re-running the generator. The
following follow-ups complete the picture so every renderer is a thin
template over the same JSON.

## Renderers to add

- **CSV exporter**: per-report-type CSV templates that consume the JSON. Add
  Tauri commands `export_cgt_csv`, `export_income_csv`, `export_balances_csv`
  taking `(account_set, financial_year)` and writing to a user-chosen path.
  Useful for tax-software import (e.g. the AU tax-agent format in the
  reference screenshot — Short-Term / Long-Term / Losses sections).
- **PDF exporter**: render the CGT/Income/Balances JSON via an HTML→PDF
  pipeline. Reuse the same HTML template the in-app view will use (see
  next item).
- **HTML view as a JSON-driven template**: the in-app `renderCgtReport` /
  `renderIncomeReport` / `renderBalancesReport` already consume the JSON,
  but the layout currently lives only in TypeScript. Consider extracting a
  shared HTML template so the PDF/HTML exports can reuse it. The pending
  CGT three-section UI (Short-Term gains / Long-Term gains / Losses)
  should land in this template.

## Cleanups

- **Port markdown templates to read JSON**: today
  `report_templates::regenerate_reports_for_set` runs the generator once
  and feeds the struct to both the markdown Tera template and the JSON
  writer. Once the JSON is the canonical artifact, the markdown step can
  be a separate pass that reads `cgt-{fy}.json` from disk and renders. This
  decouples format from compute and lets a markdown re-render happen
  without re-parsing transactions.
- **Range queries**: `generate_cgt_report_range_cmd` etc. still re-parse
  the ledger on every call. They can't be precomputed (date windows are
  user-supplied), but they could share the auto-link + load preamble with
  the FY commands via a small helper to remove the duplication in
  `main.rs:2505,2538,2568,2596,2624,2660`.
- **Drop unused per-event `discounted_gain` field**: now that the in-app
  CGT view splits into Short-Term / Long-Term / Losses sections, the
  per-event "halved gain" is no longer surfaced. Once the markdown
  template stops referencing it, the field can be removed from `CgtEvent`
  in `reports.rs:34` (and the BDD step that asserts it).

## Cleanup of orphan report files

`regenerate_reports_for_set` writes the current FY set but doesn't delete
files for FYs that are no longer relevant. After data changes, stale
`cgt-{old_fy}.md`, `income-{old_fy}.md` (and now `.json`) files remain
in `reports/`. Add a sweep at the end of `regenerate_reports_for_set` that
deletes report files whose FY isn't in the current `relevant_financial_years`
set. Be careful to only touch files matching the known prefixes
(`cgt-`, `income-`, `balances-`) so unrelated artifacts in `reports/` are
left alone.

## Discovery

- **What FYs to precompute**: `relevant_financial_years` discovers FYs from
  transaction dates. If a user wants a future or pre-history FY, it won't
  be in the cache and the FY command will fall through to the live
  generator. That's the intended behaviour; no action needed, just noted.
