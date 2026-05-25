import { closeSync, openSync } from "node:fs";
import { mkdir, readdir, rm, stat } from "node:fs/promises";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import path from "node:path";
import { isLoopbackHost } from "./command-line.mjs";
import { readJson, writeJson } from "./runtime.mjs";

const serveRuntimeDir = path.join(tmpdir(), "architext-serve");
const serveLockStaleMs = 30000;

function serveUrl(options) {
  return `http://${options.host}:${options.port}/`;
}

function serveStateKey(target) {
  return createHash("sha256").update(path.resolve(target)).digest("hex").slice(0, 24);
}

function serveStatePath(target) {
  return path.join(serveRuntimeDir, `${serveStateKey(target)}.json`);
}

function serveLockPath(target) {
  return path.join(serveRuntimeDir, `${serveStateKey(target)}.lock`);
}

function serveStatePathById(id) {
  return path.join(serveRuntimeDir, `${id}.json`);
}

async function readServeState(target) {
  const statePath = serveStatePath(target);
  try {
    return await readJson(statePath);
  } catch {
    await rm(statePath, { force: true });
    return null;
  }
}

async function readServeStateById(id) {
  if (!/^[a-f0-9]{24}$/.test(id)) return null;
  const statePath = serveStatePathById(id);
  try {
    return await readJson(statePath);
  } catch {
    await rm(statePath, { force: true });
    return null;
  }
}

async function writeServeState(target, state) {
  await mkdir(serveRuntimeDir, { recursive: true });
  await writeJson(serveStatePath(target), state);
}

async function removeServeState(target) {
  await rm(serveStatePath(target), { force: true });
}

async function removeServeStateById(id) {
  await rm(serveStatePathById(id), { force: true });
}

async function removeServeStateIfOwned(target, expected) {
  const state = await readServeState(target);
  if (state?.pid === expected.pid && state?.mode === expected.mode) {
    await removeServeState(target);
  }
}

async function withServeStateLock(target, callback, { timeoutMs = 5000, pollMs = 50 } = {}) {
  await mkdir(serveRuntimeDir, { recursive: true });
  const lockPath = serveLockPath(target);
  const deadline = Date.now() + timeoutMs;
  while (Date.now() <= deadline) {
    let acquired = false;
    try {
      await mkdir(lockPath);
      acquired = true;
    } catch (error) {
      if (error?.code !== "EEXIST") throw error;
      const lockStat = await stat(lockPath).catch(() => null);
      if (lockStat && Date.now() - lockStat.mtimeMs > serveLockStaleMs) {
        await rm(lockPath, { recursive: true, force: true });
        continue;
      }
      await new Promise((resolve) => setTimeout(resolve, pollMs));
    }
    if (!acquired) continue;
    try {
      return await callback();
    } finally {
      await rm(lockPath, { recursive: true, force: true });
    }
  }
  throw new Error(`Timed out waiting for Architext serve lifecycle lock: ${lockPath}`);
}

function pidExists(pid) {
  if (!pid || !Number.isInteger(pid)) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

export function isLoopbackServeUrl(url) {
  try {
    const parsed = new URL(url);
    return parsed.protocol === "http:" && isLoopbackHost(parsed.hostname);
  } catch {
    return false;
  }
}

async function urlReachable(url) {
  if (!isLoopbackServeUrl(url)) return false;
  try {
    const response = await fetch(url, { method: "GET" });
    return response.ok;
  } catch {
    return false;
  }
}

async function waitForUrl(url, timeoutMs = 5000) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    if (await urlReachable(url)) return true;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  return false;
}

export function browserOpenCommand(platform, url) {
  if (platform === "darwin") return { command: "open", args: [url] };
  if (platform === "win32") return { command: "cmd", args: ["/c", "start", "", url] };
  if (platform === "linux") return { command: "xdg-open", args: [url] };
  return null;
}

async function openSystemBrowser(url) {
  const openCommand = browserOpenCommand(process.platform, url);
  if (!openCommand) return { ok: false, message: `No browser launcher is configured for ${process.platform}` };
  return new Promise((resolve) => {
    let settled = false;
    const child = spawn(openCommand.command, openCommand.args, { detached: true, stdio: "ignore" });
    child.once("error", (error) => {
      if (settled) return;
      settled = true;
      resolve({ ok: false, message: error.message });
    });
    child.unref();
    setTimeout(() => {
      if (settled) return;
      settled = true;
      resolve({ ok: true });
    }, 100);
  });
}

function formatServeLink(url) {
  if (!process.stdout.isTTY) return url;
  return `\u001B]8;;${url}\u0007${url}\u001B]8;;\u0007`;
}

async function serveForeground({ target, options, createViewerServer }) {
  const server = await createViewerServer({ target, host: options.host, port: options.port });
  const url = serveUrl(options);
  const state = {
    target: path.resolve(target),
    pid: process.pid,
    host: options.host,
    port: options.port,
    url,
    mode: "foreground",
    startedAt: new Date().toISOString()
  };
  await writeServeState(target, state);
  server.once("close", () => {
    void removeServeStateIfOwned(target, state);
  });
  console.log(`Serving Architext for ${target}`);
  console.log(`Open ${formatServeLink(url)}`);
  if (options.open && !options.noOpen) {
    const opened = await openSystemBrowser(url);
    if (!opened.ok) console.error(`Browser launch failed: ${opened.message}`);
  }
}

async function staleServeState(state) {
  if (!state) return true;
  if (!pidExists(state.pid)) return true;
  return !(await urlReachable(state.url));
}

async function readServeInstances({ cleanupStale = true } = {}) {
  const entries = await readdir(serveRuntimeDir, { withFileTypes: true }).catch(() => []);
  const instances = [];
  for (const entry of entries) {
    if (!entry.isFile() || !entry.name.endsWith(".json")) continue;
    const id = entry.name.slice(0, -".json".length);
    const state = await readServeStateById(id);
    if (!state) continue;
    const stale = await staleServeState(state);
    if (stale) {
      if (cleanupStale) await removeServeStateById(id);
      continue;
    }
    instances.push({ id, ...state, status: "running" });
  }
  return instances.sort((left, right) => left.startedAt.localeCompare(right.startedAt) || left.id.localeCompare(right.id));
}

function knownInstanceError(id, instances) {
  const known = instances.length ? ` Known instances: ${instances.map((instance) => instance.id).join(", ")}` : " No running instances are recorded.";
  return new Error(`Unknown Architext serve instance: ${id}.${known}`);
}

async function resolveInstance(options, target) {
  if (options.serveInstance) {
    const state = await readServeStateById(options.serveInstance);
    const instances = await readServeInstances();
    const instance = state && instances.find((candidate) => candidate.id === options.serveInstance);
    if (!instance) throw knownInstanceError(options.serveInstance, instances);
    return instance;
  }
  const state = await readServeState(target);
  if (!state) return null;
  if (await staleServeState(state)) {
    await removeServeState(target);
    return null;
  }
  return { id: serveStateKey(target), ...state, status: "running" };
}

async function serveBackground({ target, options, cliEntryPath }) {
  return withServeStateLock(target, async () => {
    const existing = await readServeState(target);
    if (existing && !(await staleServeState(existing))) {
      console.log(`Architext is already serving ${existing.target}`);
      console.log(`Open ${formatServeLink(existing.url)}`);
      if (options.open && !options.noOpen) {
        const opened = await openSystemBrowser(existing.url);
        if (!opened.ok) console.error(`Browser launch failed: ${opened.message}`);
      }
      return;
    }
    if (existing) await removeServeState(target);

    const logPath = path.join(serveRuntimeDir, `${serveStateKey(target)}.log`);
    const logFd = openSync(logPath, "a");
    let child;
    try {
      child = spawn(process.execPath, [
        cliEntryPath,
        "serve",
        target,
        "--foreground",
        "--host",
        options.host,
        "--port",
        String(options.port),
        "--no-open"
      ], {
        detached: true,
        stdio: ["ignore", logFd, logFd]
      });
      child.unref();
    } finally {
      closeSync(logFd);
    }

    const url = serveUrl(options);
    if (!(await waitForUrl(url))) {
      if (pidExists(child.pid)) {
        try {
          process.kill(child.pid, "SIGTERM");
        } catch {
          // The child may have exited between the liveness check and signal.
        }
      }
      throw new Error(`Architext background serve did not become reachable at ${url}. Check ${logPath}`);
    }

    await writeServeState(target, {
      target: path.resolve(target),
      pid: child.pid,
      host: options.host,
      port: options.port,
      url,
      logPath,
      mode: "background",
      startedAt: new Date().toISOString()
    });
    console.log(`Serving Architext for ${target} in the background`);
    console.log(`Open ${formatServeLink(url)}`);
    if (options.open && !options.noOpen) {
      const opened = await openSystemBrowser(url);
      if (!opened.ok) console.error(`Browser launch failed: ${opened.message}`);
    }
  });
}

async function serveStatus(target, options) {
  const state = await resolveInstance(options, target);
  if (!state) {
    console.log(`No recorded Architext serve instance for ${target}`);
    return;
  }
  console.log(`Architext is serving ${state.target}`);
  console.log(`ID: ${state.id}`);
  console.log(`PID: ${state.pid}`);
  console.log(`Open ${formatServeLink(state.url)}`);
  console.log(`Mode: ${state.mode ?? "background"}`);
  if (state.logPath) console.log(`Logs: ${state.logPath}`);
}

async function stopState(state) {
  if (pidExists(state.pid)) {
    process.kill(state.pid, "SIGTERM");
    const stopped = await new Promise((resolve) => {
      const started = Date.now();
      const poll = () => {
        if (!pidExists(state.pid)) resolve(true);
        else if (Date.now() - started > 3000) resolve(false);
        else setTimeout(poll, 100);
      };
      poll();
    });
    if (!stopped) console.error(`Architext server ${state.pid} did not stop after SIGTERM`);
  }
  await removeServeStateById(state.id);
}

async function stopServe(target, options) {
  const state = await resolveInstance(options, target);
  if (!state) {
    console.log(`No recorded Architext serve instance for ${target}`);
    return;
  }
  await stopState(state);
  console.log(`Stopped Architext serve instance ${state.id} for ${state.target}`);
}

async function listServeInstances(options) {
  const instances = await readServeInstances();
  const filtered = options.serveInstance
    ? instances.filter((instance) => instance.id === options.serveInstance)
    : instances;
  if (options.serveInstance && filtered.length === 0) throw knownInstanceError(options.serveInstance, instances);
  if (options.json) {
    console.log(JSON.stringify({ instances: filtered }, null, 2));
    return;
  }
  if (filtered.length === 0) {
    console.log("No recorded Architext serve instances are running.");
    return;
  }
  console.log("Architext serve instances:");
  for (const instance of filtered) {
    console.log(`${instance.id}  ${instance.pid}  ${instance.mode ?? "background"}  ${instance.url}  ${instance.target}`);
    if (instance.logPath) console.log(`  Logs: ${instance.logPath}`);
    console.log(`  Started: ${instance.startedAt}`);
  }
}

async function restartServe(target, options, cliEntryPath, refreshTarget) {
  const state = await resolveInstance(options, target);
  if (!state) {
    console.log(`No recorded Architext serve instance for ${target}`);
    return;
  }
  if (state.mode === "foreground") {
    throw new Error("Foreground serve instances cannot be restarted; stop the owning terminal process and start serve again.");
  }
  if (!refreshTarget) throw new Error("Serve refresh is not configured.");
  console.log(`Syncing Architext target before restart: ${state.target}`);
  await refreshTarget(state.target);
  await stopState(state);
  await serveBackground({
    target: state.target,
    options: {
      ...options,
      host: state.host,
      port: state.port,
      background: true,
      open: false,
      noOpen: true,
      serveRestart: false
    },
    cliEntryPath
  });
  console.log(`Restarted Architext background server ${state.id} for ${state.target}`);
}

export async function runServeLifecycle({ target, options, createViewerServer, cliEntryPath, refreshTarget }) {
  if (options.serveList) {
    await listServeInstances(options);
    return;
  }
  if (options.serveStatus) {
    await serveStatus(target, options);
    return;
  }
  if (options.serveStop) {
    await stopServe(target, options);
    return;
  }
  if (options.serveRestart) {
    await restartServe(target, options, cliEntryPath, refreshTarget);
    return;
  }
  if (options.background) {
    await serveBackground({ target, options, cliEntryPath });
    return;
  }
  await serveForeground({ target, options, createViewerServer });
}
