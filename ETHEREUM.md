# Ghost "ethereum" Account Set Bug — FIXED

## The Problem

A top-level `sources/ethereum/` folder kept getting created, making "ethereum" appear as an empty account set in the sidebar dropdown. The real ethereum data lives under `sources/richard/ethereum/`.

## Root Cause

4 places in `src/main.ts` computed `accountFolder` by naively stripping `assets:` and joining with `/`, missing the account set prefix:

```typescript
// account = "assets:ethereum:0x6d25..."
// produced: "ethereum/0x6d25..."     ← WRONG (creates stray top-level folder)
// should be: "richard/ethereum"       ← CORRECT
const accountFolder = selectedAccount.split(":").slice(1).join("/");
```

The backend does `fs::create_dir_all(sources_dir.join(account_folder))`, so the stray folder silently appeared every time a rule was saved, a CSV imported, or a transaction rendered with inline editing.

## The Fix

1. **Extracted `resolveAccountFolder()` to `src/render.ts`** as a pure, testable function that:
   - Looks up `accountFoldersMap` for an exact match
   - Walks up the account hierarchy (e.g. `assets:ethereum:0xabc` → `assets:ethereum`)
   - Falls back to prepending `selectedAccountSet` (e.g. `richard/ethereum/0xabc`)

2. **All 4 call sites** now use `resolveAccountFolder()` instead of inline path computation:
   - Transaction row `data-account-folder` (inline rule editing)
   - CSV import button
   - Rules CSV import button
   - Manual transaction / trade link folder resolution

3. **Tests added:**
   - 6 unit tests in `src/render.test.ts` covering exact match, hierarchy walk, fallback with account set prefix, and the specific stray-folder case
   - 1 BDD scenario asserting that after manual transaction append, the only top-level source folders are the expected account sets
