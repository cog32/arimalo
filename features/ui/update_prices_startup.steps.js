const { When, Then } = require("@cucumber/cucumber");
const { By, until } = require("selenium-webdriver");

// Session setup is owned by parse_display.steps.js BeforeAll/AfterAll. These
// steps consume the driver via the cucumber-js World (this.driver), matching
// hide_transaction.steps.js / group_trade_link.steps.js.
//
// They assert presence only and never toggle the checkbox: the e2e app runs
// against the real vault config, so flipping it would persist to config.json
// and could trigger live price-API runs.

When("I switch to the plugins view", async function () {
  const btn = await this.driver.wait(
    until.elementLocated(By.css('.sidebarNav__item[data-view="plugins"]')),
    10_000,
  );
  await btn.click();
});

Then("I should see the update prices on startup checkbox", async function () {
  await this.driver.wait(
    until.elementLocated(By.css('[data-testid="update-prices-on-startup"]')),
    10_000,
  );
});

Then("I should see the update prices now button", async function () {
  await this.driver.wait(
    until.elementLocated(By.css('[data-testid="update-prices-now"]')),
    10_000,
  );
});
