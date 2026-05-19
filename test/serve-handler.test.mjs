import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { createViewerRequestHandler } from "../src/adapters/cli/architext-cli.mjs";

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

test("serve handler reads target data files through the package-owned server", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-serve-"));
  try {
    const targetDataDir = path.join(target, "docs", "architext", "data");
    await mkdir(targetDataDir, { recursive: true });
    await writeFile(path.join(targetDataDir, "manifest.json"), "{\"project\":{\"name\":\"Fixture\"}}\n");

    await withServer(createViewerRequestHandler({ target, targetDataDir, watchHub: { attach() {} } }), async (origin) => {
      const response = await fetch(`${origin}/data/manifest.json`);
      assert.equal(response.status, 200);
      assert.deepEqual(await response.json(), { project: { name: "Fixture" } });
    });
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("serve handler returns a controlled failure instead of leaking implementation errors", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-serve-"));
  const originalConsoleError = console.error;
  console.error = () => {};
  try {
    await withServer(createViewerRequestHandler({ target, watchHub: { attach() {} } }), async (origin) => {
      const response = await fetch(`${origin}/data/%E0%A4%A`);
      const body = await response.text();

      assert.equal(response.status, 500);
      assert.match(body, /Architext could not serve this request/);
      assert.doesNotMatch(body, /URI malformed|stat is not defined|ReferenceError/);
    });
  } finally {
    console.error = originalConsoleError;
    await rm(target, { recursive: true, force: true });
  }
});
