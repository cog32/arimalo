const { When, Then } = require("@cucumber/cucumber");
const { By, until } = require("selenium-webdriver");

// Session setup is owned by parse_display.steps.js BeforeAll/AfterAll. These
// steps consume the driver via the cucumber-js World (this.driver), matching
// update_prices_startup.steps.js / hide_transaction.steps.js.
//
// They assert presence only and never Save: the e2e app runs against the real
// vault config, so persisting prefixes would mutate config.json and regenerate
// reports.

When("I open the global settings dialog", async function () {
  const gear = await this.driver.wait(
    until.elementLocated(By.css("#globalSettingsBtn")),
    10_000,
  );
  await gear.click();
});

Then("I should see the included-accounts prefix input", async function () {
  await this.driver.wait(
    until.elementLocated(By.css('[data-testid="primary-prefix-input"]')),
    10_000,
  );
});

Then("I should see the included-accounts list", async function () {
  await this.driver.wait(
    until.elementLocated(By.css('[data-testid="primary-prefix-list"]')),
    10_000,
  );
});
