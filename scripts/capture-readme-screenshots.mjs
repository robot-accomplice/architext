import { createServer } from "node:http";
import path from "node:path";
import { chromium } from "playwright";
import { createViewerRequestHandler } from "../src/adapters/cli/architext-cli.mjs";
import { createPlanPrecomputeFarm } from "../src/adapters/http/plan-precompute.mjs";

const target = process.cwd();
const targetDataDir = path.join(target, "docs", "architext", "data");

// Each capture: navigate by mode hash, wait for the tab + a mode-specific ready
// selector, require the routing overlay to be gone, then shoot. `prepare` runs
// page interactions for modes that need state (e.g. Blast Radius search).
const captures = [
  { hash: "#flows", tab: "Flows", ready: "g.flow-edge", file: "docs/assets/screenshots/architext-flows.png" },
  { hash: "#sequence", tab: "Sequence", ready: ".sequence-message", file: "docs/assets/screenshots/architext-sequence.png" },
  { hash: "#c4", tab: "C4", ready: ".c4-lines", file: "docs/assets/screenshots/architext-c4.png" },
  { hash: "#datarisks", tab: "Data/Risks", ready: ".flow-lines", file: "docs/assets/screenshots/architext-data-risks.png" },
  { hash: "#repotree", tab: "Repo Tree", ready: ".repo-tree-row", file: "docs/assets/screenshots/architext-repo-tree.png" },
  {
    hash: "#blast",
    tab: "Blast Radius",
    ready: ".blast-head h2",
    file: "docs/assets/screenshots/architext-blast-radius.png",
    prepare: async (page) => {
      await page.locator(".blast-search").fill("routing");
      await page.locator(".blast-result").first().click();
    }
  },
  { hash: "#releasetruth", tab: "Release Truth", ready: "main", file: "docs/assets/screenshots/architext-release-truth.png" },
  { hash: "#rules", tab: "Rules", ready: "main", file: "docs/assets/screenshots/architext-rules.png" }
];

async function withServer(callback) {
  const planFarm = createPlanPrecomputeFarm({ target, dataDirFn: () => targetDataDir, log: () => {} });
  const server = createServer(createViewerRequestHandler({
    target,
    targetDataDir,
    watchHub: { attach() {} },
    planFarm
  }));
  const sockets = new Set();

  server.on("connection", (socket) => {
    sockets.add(socket);
    socket.on("close", () => sockets.delete(socket));
  });

  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });

  // Warm the plan farm so flow captures render instantly instead of racing the
  // in-browser planner (and so the screenshots exercise the shipped fast path).
  await planFarm.refresh();
  for (let i = 0; i < 600 && planFarm.stats().pending > 0; i += 1) {
    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  try {
    await callback(`http://127.0.0.1:${server.address().port}`);
  } finally {
    planFarm.dispose();
    server.closeAllConnections?.();
    for (const socket of sockets) socket.destroy();
    await new Promise((resolve) => server.close(resolve));
  }
}

async function main() {
  const browser = await chromium.launch();
  const page = await browser.newPage({
    viewport: { width: 1920, height: 1200 },
    deviceScaleFactor: 1
  });
  page.setDefaultTimeout(120_000);

  try {
    await withServer(async (origin) => {
      for (const capture of captures) {
        await page.goto(`${origin}/${capture.hash}`, { waitUntil: "domcontentloaded" });
        await page.getByRole("tab", { name: capture.tab, exact: true }).waitFor();
        if (capture.prepare) await capture.prepare(page);
        if (capture.ready) await page.locator(capture.ready).first().waitFor();
        await page.waitForFunction(() => !document.querySelector(".routing-loading-overlay"));
        await page.waitForTimeout(500);
        await page.screenshot({ path: capture.file, fullPage: false });
        console.log(capture.file);
      }
    });
  } finally {
    await browser.close();
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
