//! ANALYSIS HARNESS (not a CI test): runs the REAL loss-harvest ("Tax Savings")
//! report against the live vault for FY2026, then contrasts each flagged coin's
//! cost basis with the transfer-aware ("None" universe) snapshot to expose which
//! harvestable "losses" are artifacts of the allowlist excluding `assets:transfer`.
//!
//! Inert unless `ARIMALO_ANALYZE_VAULT=1` so it never runs in normal suites.
//! Vault path defaults to ~/workspace/accountsv2 (override with ARIMALO_VAULT).

use arimalo_covid::generated_store::{
    filter_hidden_accounts, load_active_ledger, load_hidden_accounts,
};
use arimalo_covid::ledger_parser::PriceGraph;
use arimalo_covid::processing_pipeline::{auto_link_equity_swaps, PipelineMetadata};
use arimalo_covid::report_templates::load_tax_config;
use arimalo_covid::reports::{generate_loss_harvest_report, holdings_as_of, AccountAllowlist};
use std::collections::BTreeSet;
use std::path::Path;

#[test]
fn analyze_real_vault_loss_harvest_fy2026() {
    if std::env::var("ARIMALO_ANALYZE_VAULT").is_err() {
        eprintln!("skip: set ARIMALO_ANALYZE_VAULT=1 to run vault analysis");
        return;
    }
    let home = std::env::var("HOME").unwrap();
    let vault = std::env::var("ARIMALO_VAULT").unwrap_or(format!("{home}/workspace/accountsv2"));
    let set_dir = Path::new(&vault).join("generated/richard");
    let sources_dir = Path::new(&vault).join("sources");
    let generated_dir = Path::new(&vault).join("generated");
    let base = "AUD";
    let fy_end = "2026-06-30";

    let mut parse = load_active_ledger(&set_dir).expect("load active ledger");
    let hidden = load_hidden_accounts(&set_dir);
    if !hidden.is_empty() {
        filter_hidden_accounts(&mut parse, &hidden);
    }
    let pg = PriceGraph::load(&sources_dir);
    let tax = load_tax_config(&set_dir);

    // Mirror the live FY commands: annotate auto-linked swap prices first.
    let mut tagged: Vec<(Option<String>, Option<String>, _)> =
        parse.transactions.drain(..).map(|t| (None, None, t)).collect();
    auto_link_equity_swaps(&mut tagged, Some(&pg), Some(base));
    parse.transactions = tagged.into_iter().map(|(_, _, t)| t).collect();

    let meta = PipelineMetadata::load(&generated_dir).expect("pipeline metadata");
    let allow = AccountAllowlist::new(meta.account_folders.into_keys().collect(), &[]);

    // ---- AS THE APP SHOWS IT (allowlist excludes assets:transfer) ----
    let report =
        generate_loss_harvest_report(&parse.transactions, &pg, &tax, "2026", base, None, Some(&allow));
    println!("\n================ FY2026 TAX-SAVINGS / LOSS-HARVEST (as displayed) ================");
    println!("total_realisable_loss = {:>14.2} {base}", report.total_realisable_loss);
    println!("estimated_tax_saved   = {:>14.2} {base}", report.estimated_tax_saved);
    println!("total_parcel_loss     = {:>14.2} {base}", report.total_parcel_loss);

    println!("\n-- net-underwater positions (the headline 'losses to harvest') --");
    println!("{:<12}{:>14}{:>16}{:>16}{:>16}{:>8}", "coin", "qty", "cost_basis", "value", "loss", "%");
    for p in &report.positions {
        println!(
            "{:<12}{:>14.4}{:>16.2}{:>16.2}{:>16.2}{:>7.0}%",
            p.commodity, p.quantity, p.cost_basis, p.value, p.unrealised_loss, p.pct_below_cost * 100.0
        );
    }

    println!("\n-- underwater PARCELS (parcel scan: harvest candidates, incl. inside net-positive holdings) — top 25 --");
    println!("{:<12}{:<13}{:>16}{:>12}{:>15}{:>15}", "coin", "acq_date", "cost/unit", "qty", "cost_basis", "loss");
    for p in report.underwater_parcels.iter().take(25) {
        println!(
            "{:<12}{:<13}{:>16.2}{:>12.4}{:>15.2}{:>15.2}",
            p.commodity, p.acquisition_date, p.cost_per_unit, p.quantity, p.cost_basis, p.unrealised_loss
        );
    }

    // ---- TRANSFER-AWARE CONTRAST (None universe nets assets:transfer round-trips) ----
    let aware = holdings_as_of(&parse.transactions, &pg, fy_end, base, None, None);
    let flagged: BTreeSet<String> = report
        .positions
        .iter()
        .map(|p| p.commodity.clone())
        .chain(report.underwater_parcels.iter().map(|p| p.commodity.clone()))
        .collect();
    println!("\n-- SAME COINS, transfer-aware cost basis (transfers netted) — is the 'loss' real? --");
    println!("{:<12}{:>14}{:>16}{:>16}{:>16}  verdict", "coin", "qty", "cost_basis", "value", "unrealised");
    for c in &flagged {
        match aware.holdings.iter().find(|h| &h.commodity == c) {
            Some(h) => {
                let verdict = if h.unrealised >= 0.0 {
                    "*** ACTUALLY A GAIN — fake loss ***"
                } else {
                    "still a loss"
                };
                println!(
                    "{:<12}{:>14.4}{:>16.2}{:>16.2}{:>16.2}  {verdict}",
                    c, h.quantity, h.cost_basis, h.value, h.unrealised
                );
            }
            None => println!("{:<12}{:>14}  (nets out / not held once transfers recognized)", c, "-"),
        }
    }
    println!("================================================================================\n");
}
