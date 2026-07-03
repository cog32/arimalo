#![deny(warnings)]

use std::path::PathBuf;

use arimalo_covid::ledger_parser::parse_transactions;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(file_path) = args.next() else {
        eprintln!("Usage: ledger-parse <file.transactions>");
        std::process::exit(2);
    };

    let path = PathBuf::from(file_path);
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to read file: {e}");
            std::process::exit(2);
        }
    };

    let result = parse_transactions(&contents);
    if result.ok {
        println!("OK");
        std::process::exit(0);
    }

    eprintln!("Parse failed with diagnostics:");
    for diagnostic in result.diagnostics {
        eprintln!(
            "line {}, column {}: {}",
            diagnostic.line, diagnostic.column, diagnostic.message
        );
    }

    std::process::exit(1);
}
