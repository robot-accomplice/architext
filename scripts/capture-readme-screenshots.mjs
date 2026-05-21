import { createServer } from "node:http";
import path from "node:path";
import { chromium } from "playwright";
import { createViewerRequestHandler } from "../src/adapters/cli/architext-cli.mjs";

const target = process.cwd();
const targetDataDir = path.join(target, "docs", "architext", "data");

const captures = [
  { hash: "#flows", tab: "Flows", file: "docs/assets/screenshots/architext-flows.png" },
  { hash: "#sequence", tab: "Sequence", file: "docs/assets/screenshots/architext-sequence.png" },
  { hash: "#c4", tab: "C4", file: "docs/assets/screenshots/architext-c4.png" },
  { hash: "#datarisks", tab: "Data/Risks", file: "docs/assets/screenshots/architext-data-risks.png" },
  { hash: "#releasetruth", tab: "Release Truth", file: "docs/assets/screenshots/architext-release-truth.png" },
  { hash: "#rules", tab: "Rules", file: "docs/assets/screenshots/architext-rules.png" }
];

async function withServer(callback) {
  const server = createServer(createViewerRequestHandler({
    target,
    targetDataDir,
    watchHub: { attach() {} }
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

  try {
    await callback(`http://127.0.0.1:${server.address().port}`);
  } finally {
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
  page.setDefaultTimeout(10_000);

  try {
    await withServer(async (origin) => {
      for (const capture of captures) {
        await page.goto(`${origin}/${capture.hash}`, { waitUntil: "domcontentloaded" });
        await page.getByRole("tab", { name: capture.tab, exact: true }).waitFor();
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
