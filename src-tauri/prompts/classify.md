You are analyzing uncategorised financial transactions for the account "{acct}" in a personal finance app.

These transactions are currently "expenses:unknown" and need categorisation rules:

{txn_lines}

Existing rules for this account:
{rules_json}{categorised_section}

The app uses a rules system where wildcard patterns match transaction narration/payee text.

Available account categories (from TAX_ACCOUNTS.md):
- assets:transfer — inter-wallet/exchange transfers
- expenses:crypto:gas — blockchain gas/network fees
- income:trading:fees — exchange/DEX trading fees
- expenses:loss:spam — spam/scam/dust tokens
- expenses:loss:unknown — lost/stuck/failed transactions
- income:crypto:staking — staking rewards
- income:crypto:airdrop — airdrops
- income:crypto:defi — DeFi yield, LP rewards
- equity:trading:buy — acquisition leg of a swap/trade
- equity:trading:sell — disposal leg of a swap/trade
- ignore:spam — spam tokens to ignore entirely
- ignore:noop — zero-value or no-op transactions

For crypto wallets, look at the narration patterns. Common types:
- Token swaps (e.g. "token_swap:*") → equity:trading:buy/sell based on direction
- Transfers between wallets → assets:transfer or assets:crypto:transfer
- Staking deposits/rewards → assets:staking or income:crypto:staking
- LP deposits/withdrawals → assets:lending or expenses:crypto:defi
- Bridge transactions → assets:crypto:bridge:*
- Spam/scam tokens → ignore:spam
- Failed/reverted transactions → ignore:failed
- Gas fees → expenses:crypto:gas
- Approvals → ignore:approval

If this is a crypto wallet, particularly look at transactions within a minute of each other as they are often a group of transactions for a single action.

Please suggest categorisation rules. Return ONLY a JSON array, no other text. Each rule object must have:
- "pattern": wildcard pattern to match (e.g. "*token_swap*", "*transfer*")
- "amount_account": the category account
- "payee": optional human-readable name
- "match_field": optional, set to "narration" or "meta" if the pattern should only match that field
- "explanation": brief reason

Return the JSON array directly with no markdown formatting or code fences.
