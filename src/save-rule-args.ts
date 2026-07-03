// Pure function for building the save_rule Tauri command args.
//
// Lives in its own file so save-rule-contract.test.ts can import it without
// pulling main.ts's DOM-dependent bootstrap.

import type { SavedRule } from "./rules";

/** Build the exact args object passed to the `save_rule` Tauri command.
 *  Every field the Rust signature in src-tauri/src/main.rs expects must
 *  appear here in camelCase. */
export function buildSaveRuleArgs(
  saved: SavedRule,
  accountFolder: string,
  accountSet: string,
  nowYyyymm: string,
) {
  return {
    nowYyyymm, accountFolder,
    pattern: saved.pattern, payee: null, commodity: null,
    matchField: saved.match_field, amountCondition: saved.amount_condition,
    feeCondition: saved.fee_condition, payeeCondition: saved.payee_condition,
    narrationCondition: saved.narration_condition,
    commodityCondition: saved.commodity_condition,
    metaCondition: saved.meta_condition,
    amountAccount: saved.amount_account, feeAccount: saved.fee_account,
    comment: saved.comment, accountSet,
  };
}
