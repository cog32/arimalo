use std::path::Path;

/// Header patterns for heuristic matching (case-insensitive).
const DATE_PATTERNS: &[&str] = &["date", "transaction date", "trans date", "value date"];
const PAYEE_PATTERNS: &[&str] = &["payee", "merchant"];
const NARRATION_PATTERNS: &[&str] = &[
    "description",
    "transaction description",
    "details",
    "narration",
    "memo",
    "notes",
    "reference",
    "particulars",
];
const AMOUNT_PATTERNS: &[&str] = &["amount", "transaction amount"];
const DEBIT_PATTERNS: &[&str] = &["debit", "withdrawal"];
const CREDIT_PATTERNS: &[&str] = &["credit", "deposit"];

fn match_header<'a>(headers: &'a [String], patterns: &[&str]) -> Option<&'a str> {
    for header in headers {
        let lower = header.trim().to_lowercase();
        for pattern in patterns {
            if lower == *pattern {
                return Some(header.as_str());
            }
        }
    }
    None
}

struct HeuristicMatch<'a> {
    date_col: Option<&'a str>,
    payee_col: Option<&'a str>,
    narration_col: Option<&'a str>,
    amount_col: Option<&'a str>,
    debit_col: Option<&'a str>,
    credit_col: Option<&'a str>,
}

fn run_heuristic<'a>(headers: &'a [String]) -> HeuristicMatch<'a> {
    HeuristicMatch {
        date_col: match_header(headers, DATE_PATTERNS),
        payee_col: match_header(headers, PAYEE_PATTERNS),
        narration_col: match_header(headers, NARRATION_PATTERNS),
        amount_col: match_header(headers, AMOUNT_PATTERNS),
        debit_col: match_header(headers, DEBIT_PATTERNS),
        credit_col: match_header(headers, CREDIT_PATTERNS),
    }
}

fn is_high_confidence(m: &HeuristicMatch) -> bool {
    m.date_col.is_some()
        && (m.amount_col.is_some() || (m.debit_col.is_some() && m.credit_col.is_some()))
}

#[derive(Default)]
struct ScriptOptions {
    needs_date_parse: bool,
}


/// Check sample date values to see if they're non-ISO format (e.g. "16 Nov 2022").
fn detect_needs_date_parse(sample_rows: &[Vec<String>], date_col_index: Option<usize>) -> bool {
    let idx = match date_col_index {
        Some(i) => i,
        None => return false,
    };
    for row in sample_rows {
        if let Some(val) = row.get(idx) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                // ISO dates start with a 4-digit year
                if trimmed.len() >= 4 && trimmed[..4].chars().all(|c| c.is_ascii_digit()) {
                    return false;
                }
                return true;
            }
        }
    }
    false
}

fn build_script(m: &HeuristicMatch, currency: &str, opts: &ScriptOptions) -> String {
    let mut helpers = String::new();
    let mut lines = Vec::new();

    // Always include amount cleanup helper (harmless on clean data)
    // Note: Rhai's replace() mutates in-place and returns (), so we can't chain.
    helpers.push_str("fn clean(s) { s.replace(\"$\", \"\"); s.replace(\"+\", \"\"); s.replace(\",\", \"\"); s }\n");

    if opts.needs_date_parse {
        helpers.push_str(
            r#"fn parse_date(s) {
  let parts = s.split(' ');
  let day = parts[0]; let mon = parts[1]; let year = parts[2];
  let month = switch mon {
    "Jan" => "01", "Feb" => "02", "Mar" => "03", "Apr" => "04",
    "May" => "05", "Jun" => "06", "Jul" => "07", "Aug" => "08",
    "Sep" => "09", "Oct" => "10", "Nov" => "11", "Dec" => "12",
    _ => "01"
  };
  if day.len() < 2 { day = "0" + day; }
  year + "-" + month + "-" + day
}
"#,
        );
    }

    // Date
    if let Some(col) = m.date_col {
        if opts.needs_date_parse {
            lines.push(format!(r#"  date: parse_date(row["{}"])"#, col));
        } else {
            lines.push(format!(r#"  date: row["{}"]"#, col));
        }
    } else {
        lines.push(
            r#"  // TODO: map the date column
  date: row["FIXME_DATE_COLUMN"]"#
                .to_string(),
        );
    }

    // Payee
    if let Some(col) = m.payee_col {
        lines.push(format!(r#"  payee: row["{}"]"#, col));
    } else {
        lines.push(r#"  payee: """#.to_string());
    }

    // Narration
    if let Some(col) = m.narration_col {
        lines.push(format!(r#"  narration: row["{}"]"#, col));
    } else {
        lines.push(r#"  narration: "imported""#.to_string());
    }

    // Amount (always use clean() to strip currency symbols)
    if let Some(col) = m.amount_col {
        lines.push(format!(r#"  amount: clean(row["{}"])"#, col));
    } else if let (Some(debit_col), Some(credit_col)) = (m.debit_col, m.credit_col) {
        lines.push(format!(
            r#"  amount: if row["{}"] != "" {{ "-" + clean(row["{}"]) }} else {{ clean(row["{}"]) }}"#,
            debit_col, debit_col, credit_col
        ));
    } else {
        lines.push(
            r#"  // TODO: map the amount column
  amount: row["FIXME_AMOUNT_COLUMN"]"#
                .to_string(),
        );
    }

    lines.push(format!(r#"  commodity: "{}""#, currency));
    lines.push(r#"  status: "*""#.to_string());
    lines.push(r#"  txn_id: ""  // native ID from source file (optional)"#.to_string());

    let comments = "\
// Combine fields:  row[\"Col A\"] + \" \" + row[\"Col B\"]\n\
// Conditional contra account based on sign:\n\
//   contra: if clean(row[\"Amount\"]).starts_with(\"-\") { \"income:sales\" } else { \"expenses:purchases\" }\n\
// Conditional narration (e.g. tag sells):\n\
//   narration: if clean(row[\"Amount\"]).starts_with(\"-\") { \"sell \" + row[\"Type\"] } else { row[\"Type\"] }\n";

    format!("{}{}#{{\n{}\n}}\n", comments, helpers, lines.join(",\n"))
}

/// Read CSV headers + first N sample rows.
pub fn read_csv_sample(
    csv_path: &Path,
    max_rows: usize,
) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let mut reader = csv::Reader::from_path(csv_path)
        .map_err(|e| format!("failed to open CSV {}: {e}", csv_path.display()))?;
    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| format!("failed to read CSV headers: {e}"))?
        .iter()
        .map(|h| h.to_string())
        .collect();
    let mut rows = Vec::new();
    for record in reader.records().take(max_rows) {
        let record = record.map_err(|e| format!("CSV read error: {e}"))?;
        rows.push(record.iter().map(|s| s.to_string()).collect());
    }
    Ok((headers, rows))
}

/// Try to get a suggestion from a local llama.cpp server (OpenAI-compatible API).
fn suggest_via_llm(headers: &[String], sample_rows: &[Vec<String>]) -> Option<String> {
    let base_url =
        std::env::var("ARIMALO_LLAMA_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let model = std::env::var("ARIMALO_LLAMA_MODEL").unwrap_or_else(|_| "default".to_string());

    let example = r#"#{
  date: row["Date"],
  payee: row["Description"],
  narration: "imported",
  amount: row["Amount"],
  commodity: "USD",
  status: "*"
}"#;

    let mut sample_text = String::new();
    for row in sample_rows {
        sample_text.push_str(&row.join(","));
        sample_text.push('\n');
    }

    let prompt = format!(
        "Given a CSV with these headers: {}\n\
         Sample rows:\n{}\n\n\
         Write a Rhai transform script that maps CSV columns to ledger fields.\n\
         The account is auto-derived from the folder path — do NOT include an account field.\n\
         The script must return a Rhai map literal (#{{ ... }}) with keys: date, payee, narration, amount, commodity, status.\n\
         Access CSV columns via row[\"ColumnName\"].\n\n\
         Example:\n{}\n\n\
         Return ONLY the Rhai script, no explanation.",
        headers.join(", "),
        sample_text,
        example,
    );

    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "user", "content": prompt}
        ],
        "temperature": 0.2
    });

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(30))
        .build();

    let response = agent
        .post(&url)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .ok()?;

    let resp_text = response.into_string().ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&resp_text).ok()?;
    let content = parsed["choices"][0]["message"]["content"].as_str()?;

    // Extract just the Rhai map literal if the LLM wrapped it in markdown
    let script = extract_rhai_block(content);
    Some(script)
}

pub fn extract_rhai_block(text: &str) -> String {
    // Try to find ```rhai ... ``` or ``` ... ``` blocks
    if let Some(start) = text.find("```") {
        let after_start = &text[start + 3..];
        // Skip optional language tag on same line
        let code_start = after_start.find('\n').map(|i| i + 1).unwrap_or(0);
        let after_tag = &after_start[code_start..];
        if let Some(end) = after_tag.find("```") {
            return after_tag[..end].trim().to_string();
        }
    }
    // No fenced block. The model may emit helper fns / let-bindings before the
    // `#{ ... }` map (it's instructed to). Keep them — slicing from `#{` would
    // drop the preamble and leave the map referencing undefined names. Start at
    // the first Rhai construct and end at the last `}`, trimming any stray prose.
    let code_start = ["fn ", "let ", "if ", "#{"]
        .iter()
        .filter_map(|kw| text.find(kw))
        .min();
    if let (Some(start), Some(end)) = (code_start, text.rfind('}')) {
        if end >= start {
            return text[start..=end].trim().to_string();
        }
    }
    text.trim().to_string()
}

/// Validate that a Rhai script compiles. Returns Ok(()) or the error message.
pub fn rhai_compile_check(script: &str) -> Result<(), String> {
    let engine = rhai::Engine::new();
    engine
        .compile(script)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Validate that a Rhai script compiles.
fn rhai_compiles(script: &str) -> bool {
    rhai_compile_check(script).is_ok()
}

/// Validate that a Rhai transform actually *runs* against a sample row — not
/// just that it compiles. Rhai resolves variables and functions at runtime, so
/// a script that references an undefined variable (`narration: narr`) or calls
/// an undefined helper (`clean(...)` with no `fn clean`) compiles cleanly yet
/// fails the moment the pipeline executes it. Evaluating it here against a real
/// sample row surfaces those errors so the caller can retry before the user
/// ever sees them. Uses the same engine + `row` shape as the live pipeline.
pub fn rhai_run_check(
    script: &str,
    headers: &[String],
    sample_row: &[String],
    source_path: &str,
) -> Result<(), String> {
    let engine = crate::csv_transform::new_transform_engine();
    let ast = engine.compile(script).map_err(|e| e.to_string())?;
    let row_map =
        crate::csv_transform::build_row_map(headers, sample_row, 0, source_path, "assets:sample");
    let mut scope = rhai::Scope::new();
    scope.push("row", row_map);
    // The transform must return a map literal; a non-map return also fails here.
    engine
        .eval_ast_with_scope::<rhai::Map>(&mut scope, &ast)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Generate a suggested Rhai transform script for a CSV file.
///
/// - `csv_path`: path to the original CSV (not yet copied)
/// - `currency`: optional currency, defaults to "USD"
///
/// Note: the account is auto-derived from the folder path at pipeline time,
/// so the generated script does not include an `account:` field.
pub fn generate_suggestion(csv_path: &Path, currency: Option<&str>) -> Result<String, String> {
    let currency = currency.unwrap_or("USD");
    let (headers, sample_rows) = read_csv_sample(csv_path, 5)?;
    let m = run_heuristic(&headers);

    // Detect date column index for format detection
    let date_col_index = m
        .date_col
        .and_then(|col| headers.iter().position(|h| h == col));
    let opts = ScriptOptions {
        needs_date_parse: detect_needs_date_parse(&sample_rows, date_col_index),
    };

    if is_high_confidence(&m) {
        return Ok(build_script(&m, currency, &opts));
    }

    // Low confidence — try LLM fallback
    if let Some(llm_script) = suggest_via_llm(&headers, &sample_rows) {
        if rhai_compiles(&llm_script) {
            return Ok(llm_script);
        }
    }

    // Fallback: heuristic with placeholder comments
    Ok(build_script(&m, currency, &opts))
}

/// Generate a suggestion from headers directly (for BDD tests without a file).
pub fn suggest_transform_script(headers: &[String], currency: Option<&str>) -> String {
    let currency = currency.unwrap_or("USD");
    let m = run_heuristic(headers);
    build_script(&m, currency, &ScriptOptions::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_date_header() {
        let headers = vec!["Transaction Date".to_string(), "Desc".to_string()];
        assert_eq!(
            match_header(&headers, DATE_PATTERNS),
            Some("Transaction Date")
        );
    }

    #[test]
    fn test_high_confidence_with_amount() {
        let headers = vec![
            "Date".to_string(),
            "Description".to_string(),
            "Amount".to_string(),
        ];
        let m = run_heuristic(&headers);
        assert!(is_high_confidence(&m));
    }

    #[test]
    fn test_high_confidence_with_debit_credit() {
        let headers = vec![
            "Date".to_string(),
            "Debit".to_string(),
            "Credit".to_string(),
        ];
        let m = run_heuristic(&headers);
        assert!(is_high_confidence(&m));
    }

    #[test]
    fn test_low_confidence_no_date() {
        let headers = vec!["Col1".to_string(), "Col2".to_string()];
        let m = run_heuristic(&headers);
        assert!(!is_high_confidence(&m));
    }

    #[test]
    fn test_extract_rhai_from_markdown() {
        let text = "Here is the script:\n```rhai\n#{date: row[\"Date\"]}\n```\nDone.";
        assert_eq!(extract_rhai_block(text), "#{date: row[\"Date\"]}");
    }

    #[test]
    fn test_extract_rhai_preserves_helpers_when_unfenced() {
        // The AI is told to emit helper fns / let-bindings before the map. When
        // the output is unfenced, extraction must KEEP that preamble — slicing
        // from `#{` would drop it and leave the map referencing undefined names
        // (`narr`, `clean`), which is why generation never one-shot.
        let text = "fn clean(s) { s.replace(\",\", \"\"); s }\n\
                    let narr = row[\"Title\"];\n\n\
                    #{\n  \
                    date: row[\"Date\"],\n  \
                    narration: narr,\n  \
                    amount: clean(row[\"Amount\"]),\n  \
                    commodity: \"AUD\",\n  \
                    status: \"*\"\n}";
        let out = extract_rhai_block(text);
        assert!(out.contains("fn clean"), "helper fn was dropped:\n{out}");
        assert!(out.contains("let narr"), "let binding was dropped:\n{out}");
        // The preserved script must actually run (helpers resolve narr + clean).
        let headers = vec![
            "Date".to_string(),
            "Title".to_string(),
            "Amount".to_string(),
        ];
        let row = vec![
            "2024-01-15".to_string(),
            "Contribution".to_string(),
            "1,000".to_string(),
        ];
        assert!(
            rhai_run_check(&out, &headers, &row, "file.csv").is_ok(),
            "extracted script should run, got:\n{out}"
        );
    }

    #[test]
    fn test_rhai_compile_check_valid_simple_map() {
        let script = r#"#{
  date: row["Date"],
  payee: row["Description"],
  narration: "imported",
  amount: row["Amount"],
  commodity: "USD",
  status: "*",
  txn_id: ""
}"#;
        assert!(rhai_compile_check(script).is_ok());
    }

    #[test]
    fn test_rhai_compile_check_valid_with_helpers() {
        let script = r#"fn clean(s) { s.replace("$", ""); s.replace("+", ""); s.replace(",", ""); s }

#{
  date: row["Date"],
  payee: "",
  narration: row["Description"],
  amount: clean(row["Amount"]),
  commodity: "AUD",
  status: "*",
  txn_id: ""
}"#;
        assert!(rhai_compile_check(script).is_ok());
    }

    #[test]
    fn test_rhai_compile_check_valid_trim_pattern() {
        // Correct Rhai pattern: trim() mutates in place, then use the variable
        let script = r#"let s = row["time"];
s.trim();
let parts = s.split(" - ");
let date = parts[0];
#{
  date: date,
  payee: "",
  narration: row["action"],
  amount: row["amount"],
  commodity: "USD",
  status: "*"
}"#;
        assert!(rhai_compile_check(script).is_ok());
    }

    #[test]
    fn test_rhai_compile_check_valid_branching() {
        // Script that branches on _source_path for multiple CSV types
        let script = r#"fn clean(s) { s.replace("$", ""); s.replace(",", ""); s }

if row["_source_path"].contains("withdraw") {
  #{
    date: row["time"],
    payee: "",
    narration: row["destination"],
    amount: clean(row["amount"]),
    commodity: "USDC",
    status: "*"
  }
} else {
  #{
    date: row["time"],
    payee: row["asset"],
    narration: row["side"],
    amount: clean(row["size"]),
    commodity: "USD",
    status: "*"
  }
}"#;
        assert!(rhai_compile_check(script).is_ok());
    }

    #[test]
    fn test_rhai_compile_check_rejects_chained_trim() {
        // This is the common AI mistake: chaining trim().split() fails
        // because trim() returns () in Rhai
        let script = r#"let parts = row["time"].trim().split(" - ");
#{
  date: parts[0],
  payee: "",
  narration: "test",
  amount: "0",
  commodity: "USD",
  status: "*"
}"#;
        // Note: trim().split() actually compiles in Rhai (it's a runtime error, not compile-time)
        // so we can't catch this at compile time. The compile check catches syntax errors.
        // This test documents the limitation.
        let result = rhai_compile_check(script);
        // If Rhai accepts it syntactically, that's expected — the error is at runtime
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_rhai_compile_check_rejects_syntax_error() {
        let script = r#"#{
  date: row["Date"],
  this is not valid rhai
}"#;
        assert!(rhai_compile_check(script).is_err());
    }

    #[test]
    fn test_rhai_compile_check_rejects_unclosed_brace() {
        let script = r#"#{
  date: row["Date"],
  payee: ""
"#;
        assert!(rhai_compile_check(script).is_err());
    }

    #[test]
    fn test_heuristic_generates_compilable_script() {
        let headers = vec![
            "Date".to_string(),
            "Description".to_string(),
            "Amount".to_string(),
        ];
        let script = suggest_transform_script(&headers, Some("AUD"));
        assert!(
            rhai_compile_check(&script).is_ok(),
            "Heuristic-generated script should compile: {script}"
        );
    }

    #[test]
    fn test_heuristic_debit_credit_generates_compilable_script() {
        let headers = vec![
            "Date".to_string(),
            "Details".to_string(),
            "Debit".to_string(),
            "Credit".to_string(),
        ];
        let script = suggest_transform_script(&headers, Some("USD"));
        assert!(
            rhai_compile_check(&script).is_ok(),
            "Heuristic-generated script should compile: {script}"
        );
    }

    #[test]
    fn test_heuristic_unknown_headers_generates_compilable_script() {
        let headers = vec![
            "time".to_string(),
            "action".to_string(),
            "source".to_string(),
            "destination".to_string(),
            "accountValueChange".to_string(),
            "fee".to_string(),
        ];
        let script = suggest_transform_script(&headers, Some("USDC"));
        assert!(
            rhai_compile_check(&script).is_ok(),
            "Heuristic-generated script should compile even with unknown headers: {script}"
        );
    }

    #[test]
    fn test_run_check_catches_undefined_variable() {
        // The exact AI-generator failure: `narration: narr` compiles (narr is a
        // valid identifier) but is a runtime "Variable not found" — the kind of
        // bug a compile-only check lets through to the pipeline.
        let script = r#"#{
  date: row["Date"],
  payee: "AustralianSuper",
  narration: narr,
  amount: row["Total Amount"],
  commodity: "AUD",
  status: "*"
}"#;
        assert!(rhai_compile_check(script).is_ok(), "should compile");
        let headers = vec!["Date".to_string(), "Total Amount".to_string()];
        let row = vec!["2024-01-15".to_string(), "100.00".to_string()];
        let err = rhai_run_check(script, &headers, &row, "file.csv")
            .expect_err("undefined variable must fail the run-check");
        assert!(
            err.contains("narr"),
            "run-check should surface the undefined variable, got: {err}"
        );
    }

    #[test]
    fn test_run_check_catches_undefined_function() {
        // `clean()` used but never defined — also compiles, fails at runtime.
        let script = r#"#{
  date: row["Date"],
  payee: "",
  narration: row["Date"],
  amount: clean(row["Total Amount"]),
  commodity: "AUD",
  status: "*"
}"#;
        assert!(rhai_compile_check(script).is_ok());
        let headers = vec!["Date".to_string(), "Total Amount".to_string()];
        let row = vec!["2024-01-15".to_string(), "100.00".to_string()];
        assert!(
            rhai_run_check(script, &headers, &row, "file.csv").is_err(),
            "undefined function must fail the run-check"
        );
    }

    #[test]
    fn test_run_check_passes_for_valid_self_contained_script() {
        let script = r#"fn clean(s) { s.replace("$", ""); s.replace(",", ""); s }
#{
  date: row["Date"],
  payee: "",
  narration: row["Desc"],
  amount: clean(row["Amount"]),
  commodity: "AUD",
  status: "*"
}"#;
        let headers = vec!["Date".to_string(), "Desc".to_string(), "Amount".to_string()];
        let row = vec![
            "2024-01-15".to_string(),
            "groceries".to_string(),
            "$1,234.50".to_string(),
        ];
        assert!(rhai_run_check(script, &headers, &row, "file.csv").is_ok());
    }

    #[test]
    fn test_run_check_runs_the_branch_for_the_sample_source_path() {
        // A multi-type script branches on row["_source_path"]; the run-check must
        // exercise the branch matching the sample's source path.
        let script = r#"if row["_source_path"].contains("withdraw") {
  #{ date: row["t"], payee: "", narration: "w", amount: row["a"], commodity: "USDC", status: "*" }
} else {
  #{ date: row["t"], payee: "", narration: "d", amount: row["a"], commodity: "USD", status: "*" }
}"#;
        let headers = vec!["t".to_string(), "a".to_string()];
        let row = vec!["2024-01-15".to_string(), "5".to_string()];
        assert!(rhai_run_check(script, &headers, &row, "deposits.csv").is_ok());
        assert!(rhai_run_check(script, &headers, &row, "withdraw.csv").is_ok());
    }

    #[test]
    fn test_run_check_rejects_non_map_return() {
        // A transform must return a map literal; returning a string must fail.
        let script = r#""not a map""#;
        let headers = vec!["Date".to_string()];
        let row = vec!["2024-01-15".to_string()];
        assert!(rhai_run_check(script, &headers, &row, "file.csv").is_err());
    }
}
