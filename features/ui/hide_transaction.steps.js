const assert = require("node:assert/strict");
const { When, Then } = require("@cucumber/cucumber");
const { By } = require("selenium-webdriver");

// Driver setup is owned by parse_display.steps.js BeforeAll/AfterAll.
// Steps consume it via cucumber-js World (this.driver), matching the
// convention used by group_trade_link.steps.js.

When(
  "I click the hide button on the transaction row for payee {string}",
  async function (payee) {
    const rows = await this.driver.findElements(By.css('[data-testid="txn-row"]'));
    for (const row of rows) {
      const cell = await row.findElement(By.css('[data-testid="txn-payee"]'));
      if ((await cell.getText()) === payee) {
        const btn = await row.findElement(By.css('[data-testid="txn-delete"]'));
        await btn.click();
        return;
      }
    }
    assert.fail(`No transaction row found for payee "${payee}"`);
  },
);

Then(
  "I should not see a transaction row for payee {string}",
  async function (payee) {
    await this.driver.wait(async () => {
      const rows = await this.driver.findElements(By.css('[data-testid="txn-row"]'));
      for (const row of rows) {
        try {
          const cell = await row.findElement(By.css('[data-testid="txn-payee"]'));
          if ((await cell.getText()) === payee) return false;
        } catch {
          // row detached during re-render; retry
        }
      }
      return true;
    }, 10_000, `Expected no transaction row for payee "${payee}"`);
  },
);
