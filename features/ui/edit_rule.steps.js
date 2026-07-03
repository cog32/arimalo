const assert = require("node:assert/strict");
const { When, Then } = require("@cucumber/cucumber");
const { By, until } = require("selenium-webdriver");

// Driver setup is owned by parse_display.steps.js BeforeAll/AfterAll.
// Steps consume it via cucumber-js World (this.driver), matching the
// convention used by hide_transaction.steps.js / group_trade_link.steps.js.

When(
  "I expand the transaction row for payee {string}",
  async function (payee) {
    const rows = await this.driver.findElements(By.css('[data-testid="txn-row"]'));
    for (const row of rows) {
      const cell = await row.findElement(By.css('[data-testid="txn-payee"]'));
      if ((await cell.getText()).includes(payee)) {
        // Click an inert area of the row (date cell is safe — no cell-clickable handler).
        const dateCell = await row.findElement(By.css("td:first-child"));
        await dateCell.click();
        await this.driver.wait(
          until.elementLocated(By.css(".txDetail__ruleBtn")),
          5_000,
        );
        return;
      }
    }
    assert.fail(`No transaction row found for payee "${payee}"`);
  },
);

When(
  'I click {string} in the row detail',
  async function (label) {
    assert.equal(label, "Edit Rule", `Only "Edit Rule" is supported, got "${label}"`);
    const btn = await this.driver.findElement(By.css(".txDetail__ruleBtn"));
    await btn.click();
    // Rule editor mounts when state.sidebarView === "rule-editor".
    await this.driver.wait(
      until.elementLocated(By.css(".ruleEditorBar")),
      5_000,
    );
  },
);

Then(
  "the rule editor pattern should contain {string}",
  async function (expected) {
    const input = await this.driver.findElement(By.css("#ruleSearchInput"));
    const value = await input.getAttribute("value");
    assert.ok(
      value.includes(expected),
      `Expected rule pattern to contain "${expected}" but got "${value}"`,
    );
  },
);

// Exact pill-text match. A loose `text.includes(expected)` match silently
// accepted both `payee:Kraken` and `payee:*Kraken*` as identical, which let
// a wildcard-stripping bug ship: the editor displayed `payee:Orca` for a
// stored `*Orca*` substring rule, hiding why it matched "Francium lyfOrca".
Then(
  "the rule editor should show the pill {string}",
  async function (expected) {
    const pills = await this.driver.findElements(
      By.css("#ruleSearch .smartSearch__pillLabel"),
    );
    const seen = [];
    for (const pill of pills) {
      const text = (await pill.getText()).trim();
      seen.push(text);
      if (text === expected) return;
    }
    assert.fail(
      `Expected pill "${expected}". Pills seen: ${JSON.stringify(seen)}`,
    );
  },
);

// Click the Nth visible txn row by position. Clicking the date cell —
// an inert region of the row — avoids the `.cell-clickable` and
// `.commodity-clickable` handlers that would otherwise swallow the click.
When(
  "I click the second transaction row",
  async function () {
    const rows = await this.driver.findElements(By.css('[data-testid="txn-row"]'));
    assert.ok(rows.length >= 2, `expected at least 2 txn rows, got ${rows.length}`);
    const dateCell = await rows[1].findElement(By.css("td:first-child"));
    await dateCell.click();
    // Wait for either: a detail row to appear, or the render to settle. The
    // detail render is async (toggleTxnExpand fetches account config first),
    // so a fixed sleep is unreliable.
    await this.driver.wait(async () => {
      const details = await this.driver.findElements(By.css(".txRow__detail"));
      return details.length > 0;
    }, 5_000, "no detail row appeared after clicking row 2");
  },
);

Then(
  "exactly one transaction detail row should be visible",
  async function () {
    const details = await this.driver.findElements(By.css(".txRow__detail"));
    assert.equal(
      details.length,
      1,
      `expected exactly 1 detail row, got ${details.length} — ` +
      `clicking one row expanded its twin via shared data-expand-key`,
    );
  },
);

Then(
  "the detail row should follow the second transaction row",
  async function () {
    const result = await this.driver.executeScript(`
      const rows = document.querySelectorAll('[data-testid="txn-row"]');
      if (rows.length < 2) return { ok: false, reason: 'fewer than 2 rows', count: rows.length };
      const target = rows[1];
      const next = target.nextElementSibling;
      if (!next || !next.classList.contains('txRow__detail')) {
        return {
          ok: false,
          reason: 'next sibling is not a detail row',
          nextTag: next ? next.tagName : null,
          nextClass: next ? next.className : null,
        };
      }
      const expandKey = target.getAttribute('data-expand-key');
      const detailFor = next.getAttribute('data-txn-detail-for');
      if (expandKey !== detailFor) {
        return { ok: false, reason: 'detail key mismatch', expandKey, detailFor };
      }
      return { ok: true };
    `);
    assert.ok(
      result.ok,
      `Detail row should immediately follow the clicked row: ${JSON.stringify(result)}`,
    );
  },
);

Then(
  "the detail row should contain an {string} button",
  async function (label) {
    const buttons = await this.driver.findElements(By.css(".txRow__detail .txDetail__ruleBtn"));
    assert.ok(buttons.length >= 1, `no "${label}" button found in detail row`);
    const text = (await buttons[0].getText()).trim();
    assert.equal(text, label);
  },
);
