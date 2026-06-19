import { appendFileSync, closeSync, openSync } from "node:fs";
import { mkdir, readdir, rm, stat } from "node:fs/promises";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import path from "node:path";
import { isLoopbackHost } from "./command-line.mjs";
import { readJson, writeJson } from "./runtime.mjs";

const serveRuntimeDir = path.join(tmpdir(), "architext-serve");

// Env-gated RCA instrumentation for the serve lifecycle (Rule 14). When
// ARCHITEXT_SERVE_DIAG is set, every lifecycle step records {pid, step, ms,
// detail} as JSONL so an intermittent --refresh/--background failure can be
// root-caused from recorded state instead of guessed. Zero cost when unset.
const serveDiagPath = process.env.ARCHITEXT_SERVE_DIAG || "";
function serveDiag(step, detail = {}) {
  if (!serveDiagPath) return;
  try {
    appendFileSync(serveDiagPath, `${JSON.stringify({ ts: Date.now(), pid: process.pid, step, ...detail })}\n`);
  } catch {
    // Diagnostics must never break the lifecycle.
  }
}
const serveLockStaleMs = 30000;
const serveStateLockTimeoutMs = 5000;
const serveStateLockPollMs = 50;
const serveStartupTimeoutMs = 15000;
const serveStartupPollMs = 100;
const browserOpenSettleMs = 100;
const serveStopTimeoutMs = 3000;
const serveStopKillTimeoutMs = 5000;
const serveStopPollMs = 100;

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

function isMissingOrCorruptStateError(error) {
  return error?.code === "ENOENT" || error instanceof SyntaxError;
}

export async function readServeState(target) {
  const statePath = serveStatePath(target);
  try {
    return await readJson(statePath);
  } catch (error) {
    // Only discard state when it is genuinely gone (ENOENT) or corrupt (parse
    // failure). Transient errors (EACCES, EMFILE) must not orphan a live server.
    if (isMissingOrCorruptStateError(error)) await rm(statePath, { force: true });
    return null;
  }
}

async function readServeStateById(id) {
  if (!/^[a-f0-9]{24}$/.test(id)) return null;
  const statePath = serveStatePathById(id);
  try {
    return await readJson(statePath);
  } catch (error) {
    // Only discard state when it is genuinely gone (ENOENT) or corrupt (parse
    // failure). Transient errors (EACCES, EMFILE) must not orphan a live server.
    if (isMissingOrCorruptStateError(error)) await rm(statePath, { force: true });
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

async function withServeStateLock(target, callback, { timeoutMs = serveStateLockTimeoutMs, pollMs = serveStateLockPollMs } = {}) {
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

async function waitForUrl(url, timeoutMs = serveStartupTimeoutMs) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    if (await urlReachable(url)) return true;
    await new Promise((resolve) => setTimeout(resolve, serveStartupPollMs));
  }
  return false;
}

async function waitForChildServeState(target, pid, timeoutMs = serveStartupTimeoutMs) {
  const started = Date.now();
  let lastState = null;
  let lastReachable = null;
  while (Date.now() - started < timeoutMs) {
    const state = await readServeState(target);
    lastState = state;
    if (state?.pid === pid) lastReachable = await urlReachable(state.url);
    if (state?.pid === pid && lastReachable) return state;
    await new Promise((resolve) => setTimeout(resolve, serveStartupPollMs));
  }
  serveDiag("waitForChildServeState.timeout", {
    ms: Date.now() - started,
    expectedPid: pid,
    sawPid: lastState?.pid ?? null,
    sawUrl: lastState?.url ?? null,
    lastReachable
  });
  return null;
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
    }, browserOpenSettleMs);
  });
}

function formatServeLink(url) {
  if (!process.stdout.isTTY) return url;
  return `\u001B]8;;${url}\u0007${url}\u001B]8;;\u0007`;
}

async function serveForeground({ target, options, createViewerServer }) {
  const { server, port } = await createViewerServer({ target, host: options.host, port: options.port });
  const url = serveUrl({ ...options, port });
  const state = {
    target: path.resolve(target),
    pid: process.pid,
    host: options.host,
    port,
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

    serveDiag("serveBackground.spawned", { childPid: child.pid, requestedPort: options.port, logPath });
    const childState = await waitForChildServeState(target, child.pid);
    if (!childState) {
      const childAlive = pidExists(child.pid);
      serveDiag("serveBackground.unreachable", { childPid: child.pid, childAlive, requestedPort: options.port });
      if (childAlive) {
        try {
          process.kill(child.pid, "SIGTERM");
        } catch {
          // The child may have exited between the liveness check and signal.
        }
      }
      const url = serveUrl(options);
      throw new Error(`Architext background serve did not become reachable at ${url}. Check ${logPath}`);
    }
    serveDiag("serveBackground.reachable", { childPid: child.pid, boundPort: childState.port });

    await writeServeState(target, {
      target: path.resolve(target),
      pid: child.pid,
      host: childState.host,
      port: childState.port,
      url: childState.url,
      logPath,
      mode: "background",
      startedAt: new Date().toISOString()
    });
    console.log(`Serving Architext for ${target} in the background`);
    console.log(`Open ${formatServeLink(childState.url)}`);
    if (options.open && !options.noOpen) {
      const opened = await openSystemBrowser(childState.url);
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

async function waitForPidGone(pid, timeoutMs, pollMs = serveStopPollMs) {
  const started = Date.now();
  while (pidExists(pid)) {
    if (Date.now() - started > timeoutMs) return false;
    await new Promise((resolve) => setTimeout(resolve, pollMs));
  }
  return true;
}

// Stop a serve child and DO NOT return until it is genuinely gone. SIGTERM is
// tried first for a clean exit, but under heavy load a child may not be
// scheduled to act on SIGTERM within the stop window (and the caller's own poll
// loop is itself starved, so a fixed wall-clock wait can expire while the child
// is merely descheduled). The serve refresh re-spawns on this child's EXACT
// port, so a surviving old process means the re-spawn collides on a held port —
// observed in CI as "serve refresh ... did not become reachable". SIGKILL is
// delivered by the kernel regardless of scheduling, so we escalate and confirm
// death before returning, guaranteeing the port is free for the re-spawn.
export async function stopServeProcess(pid, { termTimeoutMs = serveStopTimeoutMs, killTimeoutMs = serveStopKillTimeoutMs, pollMs = serveStopPollMs } = {}) {
  if (!pidExists(pid)) return true;
  try {
    process.kill(pid, "SIGTERM");
  } catch {
    return true; // Exited between the liveness check and the signal.
  }
  if (await waitForPidGone(pid, termTimeoutMs, pollMs)) return true;
  try {
    process.kill(pid, "SIGKILL");
  } catch {
    return true; // Exited after the SIGTERM wait but before the escalation.
  }
  return waitForPidGone(pid, killTimeoutMs, pollMs);
}

async function stopState(state) {
  const started = Date.now();
  const stopped = await stopServeProcess(state.pid);
  serveDiag("stopState.signalled", { targetPid: state.pid, stopped, ms: Date.now() - started, port: state.port });
  if (!stopped) console.error(`Architext server ${state.pid} did not stop after SIGTERM and SIGKILL`);
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
  serveDiag("restart.refresh.start", { id: state.id, oldPid: state.pid, port: state.port });
  await refreshTarget(state.target);
  serveDiag("restart.refresh.done", { id: state.id });
  await stopState(state);
  serveDiag("restart.respawn.start", { id: state.id, port: state.port });
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
