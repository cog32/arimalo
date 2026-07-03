// Simple transform: maps Date, Description, Amount columns to ledger fields
//
// Combining fields:
//   narration: row["Reference"] + " - " + row["Details"],
//   payee: row["First Name"] + " " + row["Last Name"],
//
// Conditional contra account based on sign:
//   contra: if clean(row["Amount"]).starts_with("-") { "income:sales" } else { "expenses:purchases" }
//
// Conditional narration (e.g. tag sells):
//   narration: if clean(row["Amount"]).starts_with("-") { "sell " + row["Type"] } else { row["Type"] }
//
// Note: account is auto-derived from the folder path — do not set it here.
#{
  date: row["Date"],
  payee: row["Description"],
  narration: "imported",
  amount: row["Amount"],
  commodity: "AUD",
  status: "*",
  txn_id: ""
}
