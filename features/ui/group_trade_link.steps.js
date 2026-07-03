const assert = require("node:assert/strict");
const { Then, When } = require("@cucumber/cucumber");
const { By, until } = require("selenium-webdriver");

// Shared driver is set up by parse_display.steps.js BeforeAll/AfterAll.
// Access it via the cucumber world or module-level reference.
// Note: cucumber-js shares step definitions across feature files.

Then("I should see {int} aggregated group headers", async function (count) {
  // Wait for group headers to render
  await this.driver.wait(
    until.elementsLocated(By.css(".txGroup__header")),
    10_000,
  );
  const headers = await this.driver.findElements(By.css(".txGroup__header"));
  assert.equal(
    headers.length,
    count,
    `Expected ${count} group headers, found ${headers.length}`,
  );
});

Then(
  "the first group header should have a chain link button",
  async function () {
    const header = await this.driver.findElement(By.css(".txGroup__header"));
    const chainBtns = await header.findElements(By.css(".chain-btn"));
    assert.ok(
      chainBtns.length > 0,
      "First group header should contain a chain link button",
    );
  },
);

When("I click the group chain link button", async function () {
  const btn = await this.driver.findElement(
    By.css(".txGroup__header .chain-btn"),
  );
  await btn.click();
  // Wait for the linking to complete (busy state clears)
  await this.driver.wait(async () => {
    const busyEls = await this.driver.findElements(
      By.css('[data-testid="app-busy"]'),
    );
    return busyEls.length === 0;
  }, 15_000);
});

Then(
  "the aggregated groups should dissolve into swap rows",
  async function () {
    // After linking, group headers should be gone (transactions become swap rows)
    const headers = await this.driver.findElements(By.css(".txGroup__header"));
    assert.equal(
      headers.length,
      0,
      `Expected no group headers after linking, found ${headers.length}`,
    );
    // Should have swap rows instead
    const swapRows = await this.driver.findElements(By.css(".txRow--swap"));
    assert.ok(swapRows.length > 0, "Expected swap rows after linking");
  },
);
