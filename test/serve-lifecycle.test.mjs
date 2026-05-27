import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { createServer } from "node:http";
import { chmodSync, existsSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { browserOpenCommand, isLoopbackServeUrl, readServeState } from "../src/adapters/cli/serve-lifecycle.mjs";
import { parseArgs } from "../src/adapters/cli/command-line.mjs";

const serveRuntimeDir = path.join(tmpdir(), "architext-serve");

function serveStatePathForTarget(target) {
  const key = createHash("sha256").update(path.resolve(target)).digest("hex").slice(0, 24);
  return path.join(serveRuntimeDir, `${key}.json`);
}

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

async function occupyPort(port = 0) {
  const server = createServer((_request, response) => {
    response.end("occupied");
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", resolve);
  });
  return server;
}

function closeServer(server) {
  return new Promise((resolve) => server.close(resolve));
}

function servedUrl(output) {
  const match = output.match(/http:\/\/127\.0\.0\.1:(\d+)\//);
  assert.ok(match, `Expected serve output to contain a local URL:\n${output}`);
  return { url: match[0], port: Number(match[1]) };
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

async function waitForText(readText, pattern, timeoutMs = 5000) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const text = readText();
    if (pattern.test(text)) return text;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  return readText();
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
      serveList: false,
      serveRestart: false,
      serveInstance: "",
      checkUpdates: false,
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

  const globalList = parseArgs(["--list"]);
  assert.equal(globalList.command, "serve");
  assert.equal(globalList.serveList, true);

  const restartByInstance = parseArgs(["serve", "--restart", "--instance", "abc123"]);
  assert.equal(restartByInstance.serveRestart, true);
  assert.equal(restartByInstance.serveInstance, "abc123");

  assert.equal(parseArgs(["serve", "--refresh"]).serveRestart, true);
  assert.equal(parseArgs(["serve", "--update"]).serveRestart, true);

  const checkUpdates = parseArgs(["--check-updates"]);
  assert.equal(checkUpdates.command, "version");
  assert.equal(checkUpdates.checkUpdates, true);
});

test("serve options fail loudly for conflicting lifecycle controls", () => {
  assert.throws(() => parseArgs(["serve", "--foreground", "--background"]), /cannot be used together/);
  assert.throws(() => parseArgs(["serve", "--open", "--no-open"]), /cannot be used together/);
  assert.throws(() => parseArgs(["serve", "--status", "--stop"]), /cannot be used together/);
  assert.throws(() => parseArgs(["serve", "--list", "--stop"]), /cannot be used together/);
  assert.throws(() => parseArgs(["serve", "--restart", "--background"]), /cannot be combined with serve startup options/);
  assert.throws(() => parseArgs(["serve", "--status", "--background"]), /cannot be combined with serve startup options/);
  assert.throws(() => parseArgs(["serve", "--instance"]), /--instance requires a value/);
  assert.throws(() => parseArgs(["serve", "--instance", "abc"]), /--instance requires --status, --stop, --list, or --restart/);
  assert.throws(() => parseArgs(["serve", "--host"]), /--host requires a value/);
  assert.throws(() => parseArgs(["serve", "--host", "0.0.0.0"]), /--host must be a loopback address/);
  assert.throws(() => parseArgs(["serve", "--host", "192.168.1.10"]), /--host must be a loopback address/);
  assert.throws(() => parseArgs(["serve", "--port", "abc"]), /--port must be an integer/);
  assert.equal(parseArgs(["serve", "--port", "0"]).port, 0);
  assert.throws(() => parseArgs(["sync", "--open"]), /--open is only valid for architext serve/);

  assert.equal(parseArgs(["serve", "--host", "localhost"]).host, "localhost");
  assert.equal(parseArgs(["serve", "--host", "::1"]).host, "::1");
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

test("serve lifecycle only probes loopback HTTP instance URLs", () => {
  assert.equal(isLoopbackServeUrl("http://127.0.0.1:4317/"), true);
  assert.equal(isLoopbackServeUrl("http://localhost:4317/"), true);
  assert.equal(isLoopbackServeUrl("http://[::1]:4317/"), true);
  assert.equal(isLoopbackServeUrl("http://192.168.1.10:4317/"), false);
  assert.equal(isLoopbackServeUrl("https://127.0.0.1:4317/"), false);
  assert.equal(isLoopbackServeUrl("not a url"), false);
});

test("serve background records status and can be stopped", async () => {
  await withViewerDist(async () => {
    const target = await createServeTarget();
    try {
      const output = run(["serve", target, "--background", "--host", "127.0.0.1", "--port", "0", "--no-open"]);
      const served = servedUrl(output);
      assert.match(output, /in the background/);
      assert.notEqual(served.port, 0);

      const response = await waitForHttpOk(served.url);
      assert.equal(response.status, 200);

      const duplicate = run(["serve", target, "--background", "--port", "0", "--no-open"]);
      assert.match(duplicate, /already serving/);
      assert.match(duplicate, new RegExp(`http://127\\.0\\.0\\.1:${served.port}/`));

      const status = run(["serve", target, "--status"]);
      assert.match(status, /Architext is serving/);
      assert.match(status, /PID:/);
      assert.match(status, new RegExp(`http://127\\.0\\.0\\.1:${served.port}/`));
      assert.match(status, /Logs:/);

      const stopped = run(["serve", target, "--stop"]);
      assert.match(stopped, /Stopped Architext serve instance/);

      const afterStop = run(["serve", target, "--status"]);
      assert.match(afterStop, /No recorded Architext serve instance/);
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

test("serve background advances to the next available port when the preferred port is occupied", async () => {
  await withViewerDist(async () => {
    const target = await createServeTarget();
    const blocker = await occupyPort();
    const occupiedPort = blocker.address().port;
    try {
      const output = run(["serve", target, "--background", "--host", "127.0.0.1", "--port", String(occupiedPort), "--no-open"]);
      const served = servedUrl(output);
      assert.notEqual(served.port, occupiedPort);

      const response = await waitForHttpOk(served.url);
      assert.equal(response.status, 200);

      const status = run(["serve", target, "--status"]);
      assert.match(status, new RegExp(`http://127\\.0\\.0\\.1:${served.port}/`));
    } finally {
      try {
        run(["serve", target, "--stop"]);
      } catch {
        // Best-effort cleanup for failures before the stop assertion.
      }
      await closeServer(blocker);
      await rm(target, { recursive: true, force: true });
    }
  });
});

test("serve list shows all instances and stop can target one instance", async () => {
  await withViewerDist(async () => {
    const first = await createServeTarget();
    const second = await createServeTarget();
    try {
      const firstServed = servedUrl(run(["serve", first, "--background", "--host", "127.0.0.1", "--port", "0", "--no-open"]));
      const secondServed = servedUrl(run(["serve", second, "--background", "--host", "127.0.0.1", "--port", "0", "--no-open"]));

      const listed = JSON.parse(run(["--list", "--json"]));
      const firstInstance = listed.instances.find((instance) => instance.target === path.resolve(first));
      const secondInstance = listed.instances.find((instance) => instance.target === path.resolve(second));
      assert.ok(firstInstance?.id);
      assert.ok(secondInstance?.id);
      assert.equal(firstInstance.url, firstServed.url);
      assert.equal(secondInstance.url, secondServed.url);

      const textList = run(["serve", "--list"]);
      assert.match(textList, new RegExp(firstInstance.id));
      assert.match(textList, new RegExp(secondInstance.id));

      const stopped = run(["serve", "--stop", "--instance", firstInstance.id]);
      assert.match(stopped, new RegExp(`Stopped Architext serve instance .*${firstInstance.id}`));

      const afterStop = JSON.parse(run(["serve", "--list", "--json"]));
      assert.equal(afterStop.instances.some((instance) => instance.id === firstInstance.id), false);
      assert.equal(afterStop.instances.some((instance) => instance.id === secondInstance.id), true);
      await waitForHttpOk(secondServed.url);
    } finally {
      for (const target of [first, second]) {
        try {
          run(["serve", target, "--stop"]);
        } catch {
          // Best-effort cleanup for failures before the stop assertion.
        }
        await rm(target, { recursive: true, force: true });
      }
    }
  });
});

test("serve refresh syncs and restarts a targeted instance", async () => {
  await withViewerDist(async () => {
    const target = await mkdtemp(path.join(tmpdir(), "architext-serve-refresh-"));
    try {
      run(["sync", target, "--yes", "--branch", "none"]);
      const manifestPath = path.join(target, "docs", "architext", "data", "manifest.json");
      const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
      await writeFile(manifestPath, `${JSON.stringify({ ...manifest, schemaVersion: "0.1.0" }, null, 2)}\n`);

      run(["serve", target, "--background", "--host", "127.0.0.1", "--port", "0", "--no-open"]);
      const before = JSON.parse(run(["serve", "--list", "--json"])).instances.find((instance) => instance.target === path.resolve(target));
      assert.ok(before?.id);

      const refreshed = run(["serve", "--refresh", "--instance", before.id]);
      assert.match(refreshed, /Syncing Architext target before restart/);
      assert.match(refreshed, new RegExp(`Restarted Architext background server .*${before.id}`));

      const after = JSON.parse(run(["serve", "--list", "--json"])).instances.find((instance) => instance.id === before.id);
      assert.equal(after.url, before.url);
      assert.notEqual(after.pid, before.pid);
      assert.equal(JSON.parse(await readFile(manifestPath, "utf8")).schemaVersion, "1.5.0");
      await waitForHttpOk(before.url);
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

test("serve status and stop are safe when no serve instance is recorded", async () => {
  const target = await createServeTarget();
  try {
    assert.match(run(["serve", target, "--status"]), /No recorded Architext serve instance/);
    assert.match(run(["serve", target, "--stop"]), /No recorded Architext serve instance/);
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("serve foreground remains an explicit blocking server path", async () => {
  await withViewerDist(async () => {
    const target = await createServeTarget();
    const child = spawn(process.execPath, [cli, "serve", target, "--foreground", "--port", "0", "--no-open"], {
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
      output = await waitForText(() => output, /Open http:\/\/127\.0\.0\.1:\d+\//);
      const served = servedUrl(output);
      const response = await waitForHttpOk(served.url);
      assert.equal(response.status, 200);
      assert.equal(child.exitCode, null);
      assert.match(output, /Serving Architext/);
      assert.notEqual(served.port, 0);
    } finally {
      child.kill("SIGTERM");
      await new Promise((resolve) => child.once("exit", resolve));
      await rm(target, { recursive: true, force: true });
    }
  });
});

test("serve foreground advances to the next available port when the preferred port is occupied", async () => {
  await withViewerDist(async () => {
    const target = await createServeTarget();
    const blocker = await occupyPort();
    const occupiedPort = blocker.address().port;
    const child = spawn(process.execPath, [cli, "serve", target, "--foreground", "--port", String(occupiedPort), "--no-open"], {
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
      output = await waitForText(() => output, /Open http:\/\/127\.0\.0\.1:\d+\//);
      const served = servedUrl(output);
      assert.notEqual(served.port, occupiedPort);

      const response = await waitForHttpOk(served.url);
      assert.equal(response.status, 200);
      assert.equal(child.exitCode, null);
    } finally {
      child.kill("SIGTERM");
      await new Promise((resolve) => child.once("exit", resolve));
      await closeServer(blocker);
      await rm(target, { recursive: true, force: true });
    }
  });
});

test("serve list discovers live foreground instances", async () => {
  await withViewerDist(async () => {
    const target = await createServeTarget();
    const child = spawn(process.execPath, [cli, "serve", target, "--foreground", "--port", "0", "--no-open"], {
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
      output = await waitForText(() => output, /Open http:\/\/127\.0\.0\.1:\d+\//);
      const served = servedUrl(output);
      await waitForHttpOk(served.url);

      const listed = JSON.parse(run(["--list", "--json"]));
      const instance = listed.instances.find((candidate) => candidate.target === path.resolve(target));
      assert.ok(instance?.id);
      assert.equal(instance.mode, "foreground");
      assert.equal(instance.url, served.url);
      assert.equal(instance.pid, child.pid);

      const textList = run(["serve", "--list"]);
      assert.match(textList, new RegExp(instance.id));
      assert.match(textList, /foreground/);
    } finally {
      child.kill("SIGTERM");
      await new Promise((resolve) => child.once("exit", resolve));
      await rm(target, { recursive: true, force: true });
    }
  });
});

test("serve refresh refuses foreground instances", async () => {
  await withViewerDist(async () => {
    const target = await createServeTarget();
    const child = spawn(process.execPath, [cli, "serve", target, "--foreground", "--port", "0", "--no-open"], {
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
      output = await waitForText(() => output, /Open http:\/\/127\.0\.0\.1:\d+\//);
      const served = servedUrl(output);
      await waitForHttpOk(served.url);
      const listed = JSON.parse(run(["--list", "--json"]));
      const instance = listed.instances.find((candidate) => candidate.target === path.resolve(target));
      assert.ok(instance?.id);

      assert.throws(
        () => run(["serve", "--refresh", "--instance", instance.id]),
        /Foreground serve instances cannot be restarted/
      );
      await waitForHttpOk(served.url);
    } finally {
      child.kill("SIGTERM");
      await new Promise((resolve) => child.once("exit", resolve));
      await rm(target, { recursive: true, force: true });
    }
  });
});

test("reading serve state preserves the file on a transient (non-ENOENT, non-parse) read error", async () => {
  // A transient read failure (e.g. EACCES from a permissions hiccup, EMFILE under
  // load) must NOT delete state for a live background server. Deletion is only
  // correct when the state is genuinely gone (ENOENT) or corrupt (JSON parse).
  const target = await mkdtemp(path.join(tmpdir(), "architext-serve-transient-"));
  const statePath = serveStatePathForTarget(target);
  await mkdir(serveRuntimeDir, { recursive: true });
  await writeFile(statePath, JSON.stringify({ pid: 4242, url: "http://127.0.0.1:4317/" }));
  // Make the file unreadable to force an EACCES (a transient-style read error)
  // without making it missing or corrupt.
  chmodSync(statePath, 0o000);
  try {
    const state = await readServeState(target);
    // The read failed, so no state is surfaced...
    assert.equal(state, null);
    // ...but the file must survive because the live server still owns it.
    assert.equal(existsSync(statePath), true, "transient read error must not delete serve state");
  } finally {
    // The file may already be gone if the guard regresses; restore perms best-effort.
    if (existsSync(statePath)) chmodSync(statePath, 0o644);
    await rm(statePath, { force: true });
    await rm(target, { recursive: true, force: true });
  }
});
