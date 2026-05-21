import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { cp, mkdtemp, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createViewerRequestHandler } from "../src/adapters/cli/architext-cli.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const sourceDataDir = path.join(repoRoot, "docs", "architext", "data");
const viewerIndex = path.join(repoRoot, "docs", "architext", "dist", "index.html");

const workflows = [];

function workflow(name, run) {
  workflows.push({ name, run });
}

async function withServer(handler, callback) {
  const server = createServer(handler);
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });

  try {
    const { port } = server.address();
    await callback(`http://127.0.0.1:${port}`);
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

async function clickRole(page, role, name) {
  const locator = page.getByRole(role, { name, exact: true });
  assert.equal(await locator.count(), 1, `Expected exactly one ${role} named "${name}"`);
  await locator.click();
}

async function fillLabel(page, label, value) {
  const locator = page.getByLabel(label, { exact: true });
  assert.equal(await locator.count(), 1, `Expected exactly one field labeled "${label}"`);
  await locator.fill(value);
}

async function fillPlaceholder(page, placeholder, value) {
  const locator = page.getByPlaceholder(placeholder, { exact: true });
  assert.equal(await locator.count(), 1, `Expected exactly one field with placeholder "${placeholder}"`);
  await locator.fill(value);
}

async function selectLabel(page, label, value) {
  const locator = page.getByLabel(label, { exact: true });
  assert.equal(await locator.count(), 1, `Expected exactly one select labeled "${label}"`);
  await locator.selectOption(value);
}

async function expectDiscardConfirmation(page, action) {
  const dialogPromise = page.waitForEvent("dialog");
  const actionPromise = action();
  const dialog = await dialogPromise;
  assert.match(dialog.message(), /unsaved changes/i);
  await dialog.dismiss();
  await actionPromise;
}

async function assertNoBrowserErrors(pageErrors, consoleErrors) {
  assert.deepEqual(pageErrors, [], `Unexpected page errors: ${pageErrors.join("\n")}`);
  assert.deepEqual(consoleErrors, [], `Unexpected console errors: ${consoleErrors.join("\n")}`);
}

workflow("top-level navigation and diagram controls", async ({ page, origin }) => {
  await page.goto(`${origin}/#flows`, { waitUntil: "domcontentloaded" });
  await page.getByRole("tab", { name: "Flows", exact: true }).waitFor();
  await page.locator(".diagram-controls").waitFor();

  for (const mode of ["Sequence", "C4", "Deployment", "Data/Risks", "Release Truth", "Rules", "Flows"]) {
    console.log(`UAT: navigating to ${mode}`);
    await clickRole(page, "tab", mode);
  }

  await selectLabel(page, "Line Style", "spline");
  await selectLabel(page, "Line Style", "straight");
  await selectLabel(page, "Line Style", "orthogonal");
  await clickRole(page, "button", "Zoom out");
  await clickRole(page, "button", "Zoom in");
  await clickRole(page, "button", "Fit");
  await clickRole(page, "button", "Reset");
  await clickRole(page, "button", "Focus");
  await clickRole(page, "button", "Exit focus");
  await clickRole(page, "button", "PDF");
  await page.getByText("Use the browser print dialog to save this view as PDF.", { exact: true }).waitFor();

  await clickRole(page, "button", "Hide steps");
  await clickRole(page, "button", "Show steps");
  await clickRole(page, "button", "Collapse left navigation");
  await clickRole(page, "button", "Expand left navigation");
  await clickRole(page, "button", "Collapse right details");
  await clickRole(page, "button", "Expand right details");
});

workflow("rules editor add, guard, save, move, delete, and category navigation", async ({ page, origin }) => {
  await page.goto(`${origin}/#rules`, { waitUntil: "domcontentloaded" });
  await page.getByRole("heading", { name: "Project Rules", exact: true }).waitFor();

  await clickRole(page, "button", "Add Rule");
  await page.getByRole("heading", { name: "Add Rule", exact: true }).waitFor();
  await assertDisabled(page, "Move up");
  await assertDisabled(page, "Move down");
  await assertDisabled(page, "Delete");

  await fillLabel(page, "Title", "Unsaved UAT rule");
  await expectDiscardConfirmation(page, () => clickRole(page, "tab", "Flows"));
  await page.getByRole("heading", { name: "Project Rules", exact: true }).waitFor();
  await clickRole(page, "button", "Cancel");

  const addCategoryAndRule = page.getByRole("button", { name: /Add Category and Rule/ });
  assert.equal(await addCategoryAndRule.count(), 1, "Expected one Add Category and Rule button in the category browse pane");
  await addCategoryAndRule.click();
  await page.getByRole("heading", { name: "Add Rule", exact: true }).waitFor();
  const id = `uat-rule-${Date.now()}`;
  await fillLabel(page, "ID", id);
  await fillLabel(page, "Title", "UAT rules editor workflows");
  await selectLabel(page, "Criticality", "high");
  await fillLabel(page, "Category", "UAT Harness");
  await selectLabel(page, "Source", "maintainer");
  await fillLabel(page, "Summary", "UAT verifies Rules editor controls through visible UI workflows.");

  const summary = page.locator(".rule-editor-summary");
  const actions = page.locator(".rule-editor-actions-footer");
  await summary.waitFor();
  await actions.waitFor();
  const summaryBox = await summary.boundingBox();
  const actionsBox = await actions.boundingBox();
  assert(summaryBox, "Rules editor summary must render");
  assert(actionsBox, "Rules editor actions footer must render");
  assert(actionsBox.y > summaryBox.y + summaryBox.height, "Rules editor actions must sit below the full edit form");
  await expectPrimarySaveButton(page);

  await clickRole(page, "button", "Save rule");
  await page.getByText("UAT rules editor workflows", { exact: true }).waitFor();
  await page.getByRole("button", { name: "Move up", exact: true }).waitFor({ state: "visible" });
  await clickRole(page, "button", "Move up");
  await page.getByText("UAT rules editor workflows", { exact: true }).waitFor();
  await clickRole(page, "button", "Move down");
  await page.getByText("UAT rules editor workflows", { exact: true }).waitFor();
  await clickRole(page, "button", "Delete");
  await page.getByText("UAT rules editor workflows", { exact: true }).waitFor({ state: "detached" });
});

workflow("release truth projection and release planning controls", async ({ page, origin }) => {
  await page.goto(`${origin}/#releasetruth`, { waitUntil: "domcontentloaded" });
  await page.getByRole("heading", { name: /Architext 1\.4\.1/ }).first().waitFor();

  await clickRole(page, "button", "Kanban");
  await page.getByRole("heading", { name: "Kanban", exact: true }).waitFor();
  await clickRole(page, "button", "Path");
  await page.getByRole("heading", { name: "Release Path", exact: true }).waitFor();
  await clickRole(page, "button", "Edit plan");
  await page.getByRole("heading", { name: /Edit Architext 1\.4\.1/ }).waitFor();

  await clickRole(page, "button", "Add new item");
  await fillPlaceholder(page, "Title", "UAT ad hoc release item");
  await fillPlaceholder(page, "Summary (optional)", "Created by the browser UAT harness through the release planning UI.");
  await page.locator(".release-planning-inline-fields select").nth(0).selectOption("test");
  await page.locator(".release-planning-inline-fields select").nth(1).selectOption("high");
  await page.locator(".release-planning-inline-fields select").nth(2).selectOption("stretch");
  await fillPlaceholder(page, "Section", "Verification Harness");
  await clickRole(page, "button", "Add and select");
  await page.getByText("UAT ad hoc release item", { exact: true }).waitFor();

  await clickRole(page, "button", "Preview changes");
  await page.getByText("Created by the browser UAT harness through the release planning UI.", { exact: true }).waitFor();
  await clickRole(page, "button", "Save draft");
  await page.waitForFunction(() => document.body.innerText.includes("STATUS: DRAFT"));
});

async function assertDisabled(page, name) {
  const button = page.getByRole("button", { name, exact: true });
  assert.equal(await button.count(), 1, `Expected button "${name}"`);
  assert.equal(await button.isDisabled(), true, `Expected "${name}" to be disabled`);
}

async function expectPrimarySaveButton(page) {
  const locator = page.getByRole("button", { name: "Save rule", exact: true });
  const style = await locator.evaluate((button) => {
    const computed = getComputedStyle(button);
    return {
      color: computed.color,
      backgroundColor: computed.backgroundColor
    };
  });
  assert.notEqual(style.backgroundColor, "rgba(0, 0, 0, 0)", "Save button should have a visible highlighted background");
  assert.notEqual(style.color, style.backgroundColor, "Save button text must contrast with its background");
  assert.match(await locator.getAttribute("class"), /primary-action/, "Save button should use the primary action treatment");
}

async function main() {
  if (!existsSync(viewerIndex)) {
    throw new Error("Package-owned viewer is not built. Run `npm run build` before `npm run test:uat`.");
  }

  const target = await mkdtemp(path.join(tmpdir(), "architext-uat-"));
  const targetDataDir = path.join(target, "docs", "architext", "data");
  await cp(sourceDataDir, targetDataDir, { recursive: true });

  const browser = await chromium.launch();
  const pageErrors = [];
  const consoleErrors = [];

  try {
    await withServer(
      createViewerRequestHandler({
        target,
        targetDataDir,
        watchHub: { attach() {} }
      }),
      async (origin) => {
        const page = await browser.newPage();
        try {
          page.setDefaultTimeout(10_000);
          await page.addInitScript(() => {
            window.print = () => {
              window.dispatchEvent(new Event("architext-print-requested"));
            };
          });
          page.on("pageerror", (error) => pageErrors.push(error.message));
          page.on("console", (message) => {
            if (message.type() === "error") consoleErrors.push(message.text());
          });

          for (const { name, run } of workflows) {
            console.log(`UAT: ${name}`);
            await run({ page, origin });
          }

          await assertNoBrowserErrors(pageErrors, consoleErrors);
        } finally {
          await page.close();
        }
      }
    );
  } finally {
    await browser.close();
    await rm(target, { recursive: true, force: true });
  }
}

main().catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
