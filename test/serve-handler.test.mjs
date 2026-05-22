import assert from "node:assert/strict";
import { createServer } from "node:http";
import { cp, mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { createViewerRequestHandler } from "../src/adapters/cli/architext-cli.mjs";

const repoRoot = path.resolve(import.meta.dirname, "..");
const template = path.join(repoRoot, "docs", "architext");

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

test("serve handler returns JSON for unknown API routes", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-serve-"));
  try {
    await withServer(createViewerRequestHandler({ target, watchHub: { attach() {} } }), async (origin) => {
      const response = await fetch(`${origin}/api/not-real`);
      const body = await response.json();

      assert.equal(response.status, 404);
      assert.match(response.headers.get("content-type") ?? "", /application\/json/);
      assert.deepEqual(body, { error: "Unknown Architext API route: /api/not-real" });
    });
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("serve handler exposes recovery status for invalid targets", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-serve-"));
  try {
    await withServer(createViewerRequestHandler({ target, watchHub: { attach() {} } }), async (origin) => {
      const response = await fetch(`${origin}/api/status`);
      const body = await response.json();

      assert.equal(response.status, 200);
      assert.equal(body.ok, false);
      assert.equal(body.status.installed, false);
      assert.match(body.status.validation.output, /Architext data is not installed/);
    });
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("serve handler reports malformed data as structured recovery status", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-serve-"));
  try {
    await cp(template, path.join(target, "docs", "architext"), { recursive: true });
    await writeFile(path.join(target, "docs", "architext", "data", "nodes.json"), "{ invalid json\n");
    await withServer(createViewerRequestHandler({ target, watchHub: { attach() {} } }), async (origin) => {
      const response = await fetch(`${origin}/api/status`);
      const body = await response.json();

      assert.equal(response.status, 200);
      assert.equal(body.ok, false);
      assert.equal(body.mode, "status");
      assert.match(body.error, /JSON|property name|Unexpected token/);
      assert.match(body.error, /Offending JSON/);
      assert.match(body.error, /\{ invalid json/);
    });
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("serve handler can run constrained sync repair for missing data", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-serve-"));
  try {
    await withServer(createViewerRequestHandler({ target, watchHub: { attach() {} } }), async (origin) => {
      const response = await fetch(`${origin}/api/sync-repair`, { method: "POST", body: "{}" });
      const body = await response.json();

      assert.equal(response.status, 200);
      assert.equal(body.ok, true);
      assert.equal(body.reload, true);
      assert.match(body.output, /Operation: install/);
      assert.match(await readFile(path.join(target, "docs", "architext", "data", "manifest.json"), "utf8"), /"schemaVersion": "1.4.0"/);
    });
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});
