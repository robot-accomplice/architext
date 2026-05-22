import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { createServer } from "node:http";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { browserOpenCommand } from "../src/adapters/cli/serve-lifecycle.mjs";
import { parseArgs } from "../src/adapters/cli/command-line.mjs";

const repoRoot = path.resolve(import.meta.dirname, "..");
const cli = path.join(repoRoot, "tools", "architext-adopt.mjs");
const viewerDist = path.join(repoRoot, "docs", "architext", "dist");
const viewerIndex = path.join(viewerDist, "index.html");

function run(args, cwd = repoRoot) {
  return execFileSync(process.execPath, [cli, ...args], {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"]
  });
}

async function createServeTarget() {
  const target = await mkdtemp(path.join(tmpdir(), "architext-serve-lifecycle-"));
  const targetDataDir = path.join(target, "docs", "architext", "data");
  await mkdir(targetDataDir, { recursive: true });
  await writeFile(path.join(targetDataDir, "manifest.json"), "{\"project\":{\"name\":\"Fixture\"}}\n");
  return target;
}

async function withViewerDist(callback) {
  const existed = existsSync(viewerIndex);
  if (!existed) {
    await mkdir(viewerDist, { recursive: true });
    await writeFile(viewerIndex, "<!doctype html><title>Architext test fixture</title>\n");
  }
  try {
    await callback();
  } finally {
    if (!existed) await rm(viewerDist, { recursive: true, force: true });
  }
}

async function freePort() {
  const server = createServer();
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const { port } = server.address();
  await new Promise((resolve) => server.close(resolve));
  return port;
}

async function waitForHttpOk(url, timeoutMs = 5000) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    try {
      const response = await fetch(url);
      if (response.ok) return response;
    } catch {
      // Keep polling until the foreground child finishes binding.
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`Timed out waiting for ${url}`);
}

test("serve options parse lifecycle controls without changing foreground defaults", () => {
  assert.deepEqual(
    parseArgs(["serve", "/tmp/repo", "--background", "--open", "--host", "127.0.0.1", "--port", "4517"]),
    {
      command: "serve",
      target: "/tmp/repo",
      topic: "",
      yes: false,
      quiet: false,
      prompt: false,
      foreground: false,
      background: true,
      open: true,
      noOpen: false,
      host: "127.0.0.1",
      port: 4517,
      serveStatus: false,
      serveStop: false,
      json: false,
      dryRun: false,
      force: false,
      overwriteData: false,
      appendAgents: false,
      noAgents: false,
      rootScripts: false,
      noRootScripts: false,
      updateGitignore: false,
      noGitignore: false,
      mode: "initial-buildout",
      out: "",
      skipValidate: false,
      nodeModules: false,
      branch: "",
      branchName: ""
    }
  );

  const defaults = parseArgs(["serve"]);
  assert.equal(defaults.foreground, false);
  assert.equal(defaults.background, false);
  assert.equal(defaults.open, false);
  assert.equal(defaults.host, "127.0.0.1");
  assert.equal(defaults.port, 4317);
});

test("serve options fail loudly for conflicting lifecycle controls", () => {
  assert.throws(() => parseArgs(["serve", "--foreground", "--background"]), /cannot be used together/);
  assert.throws(() => parseArgs(["serve", "--open", "--no-open"]), /cannot be used together/);
  assert.throws(() => parseArgs(["serve", "--status", "--stop"]), /cannot be used together/);
  assert.throws(() => parseArgs(["serve", "--status", "--background"]), /cannot be combined with serve startup options/);
  assert.throws(() => parseArgs(["serve", "--host"]), /--host requires a value/);
  assert.throws(() => parseArgs(["serve", "--port", "0"]), /--port must be an integer/);
  assert.throws(() => parseArgs(["serve", "--port", "abc"]), /--port must be an integer/);
  assert.throws(() => parseArgs(["sync", "--open"]), /--open is only valid for architext serve/);
});

test("browser opener uses platform-native launch commands", () => {
  assert.deepEqual(browserOpenCommand("darwin", "http://127.0.0.1:4317/"), {
    command: "open",
    args: ["http://127.0.0.1:4317/"]
  });
  assert.deepEqual(browserOpenCommand("linux", "http://127.0.0.1:4317/"), {
    command: "xdg-open",
    args: ["http://127.0.0.1:4317/"]
  });
  assert.deepEqual(browserOpenCommand("win32", "http://127.0.0.1:4317/"), {
    command: "cmd",
    args: ["/c", "start", "", "http://127.0.0.1:4317/"]
  });
  assert.equal(browserOpenCommand("aix", "http://127.0.0.1:4317/"), null);
});

test("serve background records status and can be stopped", async () => {
  await withViewerDist(async () => {
    const target = await createServeTarget();
    const port = await freePort();
    try {
      const output = run(["serve", target, "--background", "--host", "127.0.0.1", "--port", String(port), "--no-open"]);
      assert.match(output, /in the background/);
      assert.match(output, new RegExp(`http://127\\.0\\.0\\.1:${port}/`));

      const response = await waitForHttpOk(`http://127.0.0.1:${port}/`);
      assert.equal(response.status, 200);

      const duplicate = run(["serve", target, "--background", "--port", String(port), "--no-open"]);
      assert.match(duplicate, /already serving/);
      assert.match(duplicate, new RegExp(`http://127\\.0\\.0\\.1:${port}/`));

      const status = run(["serve", target, "--status"]);
      assert.match(status, /Architext is serving/);
      assert.match(status, /PID:/);
      assert.match(status, new RegExp(`http://127\\.0\\.0\\.1:${port}/`));
      assert.match(status, /Logs:/);

      const stopped = run(["serve", target, "--stop"]);
      assert.match(stopped, /Stopped Architext background server/);

      const afterStop = run(["serve", target, "--status"]);
      assert.match(afterStop, /No recorded Architext background server|Removed stale Architext background server record/);
    } finally {
      try {
        run(["serve", target, "--stop"]);
      } catch {
        // Best-effort cleanup for failures before the stop assertion.
      }
      await rm(target, { recursive: true, force: true });
    }
  });
});

test("serve status and stop are safe when no background server is recorded", async () => {
  const target = await createServeTarget();
  try {
    assert.match(run(["serve", target, "--status"]), /No recorded Architext background server/);
    assert.match(run(["serve", target, "--stop"]), /No recorded Architext background server/);
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("serve foreground remains an explicit blocking server path", async () => {
  await withViewerDist(async () => {
    const target = await createServeTarget();
    const port = await freePort();
    const child = spawn(process.execPath, [cli, "serve", target, "--foreground", "--port", String(port), "--no-open"], {
      cwd: repoRoot,
      stdio: ["ignore", "pipe", "pipe"]
    });
    let output = "";
    child.stdout.on("data", (chunk) => {
      output += chunk.toString();
    });
    child.stderr.on("data", (chunk) => {
      output += chunk.toString();
    });

    try {
      const response = await waitForHttpOk(`http://127.0.0.1:${port}/`);
      assert.equal(response.status, 200);
      assert.equal(child.exitCode, null);
      assert.match(output, /Serving Architext/);
      assert.match(output, new RegExp(`http://127\\.0\\.0\\.1:${port}/`));
    } finally {
      child.kill("SIGTERM");
      await new Promise((resolve) => child.once("exit", resolve));
      await rm(target, { recursive: true, force: true });
    }
  });
});
