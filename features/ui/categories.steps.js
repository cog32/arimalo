const assert = require("node:assert/strict");
const { When, Then } = require("@cucumber/cucumber");
const { By, until } = require("selenium-webdriver");

// The shared selenium driver is owned by parse_display.steps.js (BeforeAll/
// AfterAll). Steps consume it via the cucumber-js World (this.driver), matching
// the convention in hide_transaction.steps.js and group_trade_link.steps.js.
//
// The "the app is running", "I parse the transactions file ..." and "the sidebar
// should include the account ..." steps are reused from parse_display.steps.js.

When("I switch to the Categories view", async function () {
  const tab = await this.driver.findElement(
    By.css('.sidebarNav__item[data-view="categories"]'),
  );
  await tab.click();
  // The Categories sidebar renders a multi-root drill-down; wait for it.
  await this.driver.wait(
    until.elementLocated(By.css('[data-testid="drilldown-item"]')),
    10_000,
  );
});

Then(
  "the Categories sidebar should show the root {string}",
  async function (root) {
    const selector = `[data-testid="drilldown-item"][data-group="${root}"]`;
    await this.driver.wait(until.elementLocated(By.css(selector)), 10_000);
  },
);

When("I drill into the category {string}", async function (group) {
  const selector = `[data-testid="drilldown-item"][data-group="${group}"]`;
  const item = await this.driver.wait(
    until.elementLocated(By.css(selector)),
    10_000,
  );
  await item.click();
});

When("I select the category account {string}", async function (account) {
  const selector = `[data-testid="account-item"][data-account="${account}"]`;
  const item = await this.driver.wait(
    until.elementLocated(By.css(selector)),
    10_000,
  );
  await item.click();
});

Then("I should see at least one transaction row", async function () {
  await this.driver.wait(
    until.elementLocated(By.css('[data-testid="txn-row"]')),
    10_000,
  );
  const rows = await this.driver.findElements(
    By.css('[data-testid="txn-row"]'),
  );
  assert.ok(rows.length >= 1, "Expected at least one transaction row");
});
