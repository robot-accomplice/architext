import assert from "node:assert/strict";
import { createServer, request } from "node:http";
import { cp, mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { createViewerRequestHandler } from "../src/adapters/cli/architext-cli.mjs";

const repoRoot = path.resolve(import.meta.dirname, "..");
const viewerTemplate = path.join(repoRoot, "viewer");
const templateData = path.join(repoRoot, "docs", "architext", "data");
const copiedTemplateEntries = [
  "AGENTS_APPENDIX.md",
  "LLM_ARCHITEXT.md",
  "README.md",
  "index.html",
  "package-lock.json",
  "package.json",
  "public",
  "schema",
  "src",
  "tools",
  "tsconfig.json",
  "vite.config.ts"
];

async function writeLegacyCopiedInstall(target) {
  const legacyDir = path.join(target, "docs", "architext");
  await mkdir(legacyDir, { recursive: true });
  for (const entry of copiedTemplateEntries) {
    await cp(path.join(viewerTemplate, entry), path.join(legacyDir, entry), { recursive: true });
  }
  await cp(templateData, path.join(legacyDir, "data"), { recursive: true });
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

async function mutationToken(origin) {
  const response = await fetch(`${origin}/api/session`);
  assert.equal(response.status, 200);
  return (await response.json()).mutationToken;
}

async function authorizedPost(origin, pathname, body = {}) {
  return fetch(`${origin}${pathname}`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-architext-mutation-token": await mutationToken(origin)
    },
    body: JSON.stringify(body)
  });
}

async function rawGetWithHost(origin, pathname, host) {
  const url = new URL(pathname, origin);
  return new Promise((resolve, reject) => {
    const req = request({
      hostname: url.hostname,
      port: url.port,
      path: url.pathname,
      headers: { host }
    }, (res) => {
      let body = "";
      res.setEncoding("utf8");
      res.on("data", (chunk) => { body += chunk; });
      res.on("end", () => resolve({ status: res.statusCode, body: JSON.parse(body) }));
    });
    req.on("error", reject);
    req.end();
  });
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

test("serve handler serves the resolved diagram config from the project config file", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-config-"));
  try {
    const architextDir = path.join(target, "docs", "architext");
    await mkdir(architextDir, { recursive: true });
    // Project config wins over any user-global config, so these assertions are
    // deterministic regardless of the developer's ~/.architext/config.json.
    await writeFile(
      path.join(architextDir, "config.json"),
      JSON.stringify({ layout: { laneWidth: 321 }, legibility: { gapArrowheads: 0.75 }, bogus: 1 })
    );

    await withServer(createViewerRequestHandler({ target, targetDataDir: path.join(architextDir, "data"), watchHub: { attach() {} } }), async (origin) => {
      const response = await fetch(`${origin}/api/config`);
      assert.equal(response.status, 200);
      const body = await response.json();
      assert.equal(body.diagram.layout.laneWidth, 321);
      assert.equal(body.diagram.legibility.gapArrowheads, 0.75);
      assert.ok(Array.isArray(body.warnings));
      assert.ok(body.warnings.some((w) => /unknown section "bogus"/.test(w)));
      assert.ok(body.fields?.layout?.laneWidth, "GET payload carries the field spec for the UI");
    });
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("serve handler writes diagram config via an authorized POST", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-config-write-"));
  try {
    const architextDir = path.join(target, "docs", "architext");
    await mkdir(architextDir, { recursive: true });

    await withServer(createViewerRequestHandler({ target, targetDataDir: path.join(architextDir, "data"), watchHub: { attach() {} }, mutationToken: "secret" }), async (origin) => {
      const response = await fetch(`${origin}/api/config`, {
        method: "POST",
        headers: { "content-type": "application/json", "x-architext-mutation-token": "secret" },
        body: JSON.stringify({ scope: "project", diagram: { layout: { laneWidth: 333 } } })
      });
      assert.equal(response.status, 200);
      const body = await response.json();
      assert.equal(body.ok, true);
      assert.equal(body.diagram.layout.laneWidth, 333); // project layer wins on re-resolve
      const onDisk = JSON.parse(await readFile(path.join(architextDir, "config.json"), "utf8"));
      assert.deepEqual(onDisk, { layout: { laneWidth: 333 } });
    });
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("serve handler rejects an unauthorized diagram config write", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-config-auth-"));
  try {
    await withServer(createViewerRequestHandler({ target, watchHub: { attach() {} }, mutationToken: "secret" }), async (origin) => {
      const response = await fetch(`${origin}/api/config`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ scope: "project", diagram: { layout: { laneWidth: 333 } } })
      });
      assert.equal(response.status, 403);
    });
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("serve handler treats malformed data paths as not found", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-serve-"));
  try {
    await withServer(createViewerRequestHandler({ target, watchHub: { attach() {} } }), async (origin) => {
      const response = await fetch(`${origin}/data/%E0%A4%A`);
      const body = await response.text();

      assert.equal(response.status, 404);
      assert.equal(body, "Not found");
      assert.doesNotMatch(body, /URI malformed|stat is not defined|ReferenceError/);
    });
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("serve handler rejects data path traversal attempts without reading outside target data", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-serve-"));
  try {
    const targetDataDir = path.join(target, "docs", "architext", "data");
    await mkdir(targetDataDir, { recursive: true });
    await writeFile(path.join(target, "package.json"), "{\"leaked\":true}\n");

    await withServer(createViewerRequestHandler({ target, targetDataDir, watchHub: { attach() {} } }), async (origin) => {
      for (const pathname of ["/data/..%2f..%2f..%2fpackage.json", "/data/%2e%2e%2f%2e%2e%2f%2e%2e%2fpackage.json"]) {
        const response = await fetch(`${origin}${pathname}`);
        const body = await response.text();

        assert.equal(response.status, 404);
        assert.equal(body, "Not found");
        assert.doesNotMatch(body, /leaked/);
      }
    });
  } finally {
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

test("serve handler returns ok false envelopes for release and rules API failures", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-serve-"));
  try {
    await withServer(createViewerRequestHandler({ target, watchHub: { attach() {} }, mutationToken: "secret" }), async (origin) => {
      for (const pathname of ["/api/release-plans", "/api/rules"]) {
        const response = await authorizedPost(origin, pathname, {});
        const body = await response.json();

        assert.equal(response.status, 200);
        assert.equal(body.ok, false);
        assert.equal(body.reload, false);
        assert.match(body.error, /Architext|ENOENT|requires|rules/i);
      }
    });
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("serve handler rejects mutating API requests without the server token", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-serve-"));
  try {
    await withServer(createViewerRequestHandler({ target, watchHub: { attach() {} }, mutationToken: "secret" }), async (origin) => {
      const response = await fetch(`${origin}/api/sync-repair`, { method: "POST", body: "{}" });
      const body = await response.json();

      assert.equal(response.status, 403);
      assert.match(body.error, /not authorized/);
    });
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("serve handler rejects cross-origin mutating API requests", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-serve-"));
  try {
    await withServer(createViewerRequestHandler({ target, watchHub: { attach() {} }, mutationToken: "secret" }), async (origin) => {
      const response = await fetch(`${origin}/api/sync-repair`, {
        method: "POST",
        headers: {
          origin: "http://evil.example",
          "x-architext-mutation-token": "secret"
        },
        body: "{}"
      });
      const body = await response.json();

      assert.equal(response.status, 403);
      assert.match(body.error, /loopback origin/);
    });
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("serve handler rejects non-loopback host requests", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-serve-"));
  try {
    await withServer(createViewerRequestHandler({ target, watchHub: { attach() {} }, mutationToken: "secret" }), async (origin) => {
      const response = await rawGetWithHost(origin, "/api/status", "attacker.example");

      assert.equal(response.status, 403);
      assert.match(response.body.error, /loopback origin/);
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
    await writeLegacyCopiedInstall(target);
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
      const response = await authorizedPost(origin, "/api/sync-repair");
      const body = await response.json();

      assert.equal(response.status, 200);
      assert.equal(body.ok, true);
      assert.equal(body.reload, true);
      assert.match(body.output, /Operation: install/);
      assert.match(await readFile(path.join(target, "docs", "architext", "data", "manifest.json"), "utf8"), /"schemaVersion": "1.5.0"/);
    });
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("plan endpoint answers misses with 200 so browser UAT sees no failed responses", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-plan-miss-"));
  try {
    const targetDataDir = path.join(target, "docs", "architext", "data");
    await mkdir(targetDataDir, { recursive: true });
    const knownHash = "a".repeat(64);
    const planFarm = { lookup: (hash) => (hash === knownHash ? "{\"laneIndexByNode\":[]}" : undefined) };

    await withServer(createViewerRequestHandler({ target, targetDataDir, watchHub: { attach() {} }, planFarm }), async (origin) => {
      // A cache miss is a designed outcome (farm warming, data just changed),
      // not an HTTP error: the release-gate UAT fails on any non-2xx response,
      // which is how 404-on-miss broke the 1.6.3 publish. 404 stays reserved
      // for "this server has no plan endpoint at all" (older versions).
      const miss = await fetch(`${origin}/api/plan/${"b".repeat(64)}`);
      assert.equal(miss.status, 200);
      const missBody = await miss.json();
      assert.equal(missBody.miss, true);
      assert.equal(missBody.plan, undefined, "miss body must not carry a plan");

      const malformed = await fetch(`${origin}/api/plan/not-a-sha`);
      assert.equal(malformed.status, 200);
      assert.equal((await malformed.json()).miss, true);

      const hit = await fetch(`${origin}/api/plan/${knownHash}`);
      assert.equal(hit.status, 200);
      assert.deepEqual((await hit.json()).plan, { laneIndexByNode: [] });
    });

    // Without a farm at all (static/embedded contexts) the endpoint still
    // answers 200-miss rather than erroring.
    await withServer(createViewerRequestHandler({ target, targetDataDir, watchHub: { attach() {} } }), async (origin) => {
      const response = await fetch(`${origin}/api/plan/${knownHash}`);
      assert.equal(response.status, 200);
      assert.equal((await response.json()).miss, true);
    });
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});
