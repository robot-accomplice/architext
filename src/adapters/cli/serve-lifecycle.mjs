import { closeSync, openSync } from "node:fs";
import { mkdir, rm } from "node:fs/promises";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import path from "node:path";
import { readJson, writeJson } from "./runtime.mjs";

const serveRuntimeDir = path.join(tmpdir(), "architext-serve");

function serveUrl(options) {
  return `http://${options.host}:${options.port}/`;
}

function serveStateKey(target) {
  return createHash("sha256").update(path.resolve(target)).digest("hex").slice(0, 24);
}

function serveStatePath(target) {
  return path.join(serveRuntimeDir, `${serveStateKey(target)}.json`);
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

async function writeServeState(target, state) {
  await mkdir(serveRuntimeDir, { recursive: true });
  await writeJson(serveStatePath(target), state);
}

async function removeServeState(target) {
  await rm(serveStatePath(target), { force: true });
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

async function urlReachable(url) {
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
  await createViewerServer({ target, host: options.host, port: options.port });
  const url = serveUrl(options);
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

async function serveBackground({ target, options, cliEntryPath }) {
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

  await mkdir(serveRuntimeDir, { recursive: true });
  const logPath = path.join(serveRuntimeDir, `${serveStateKey(target)}.log`);
  const logFd = openSync(logPath, "a");
  const child = spawn(process.execPath, [
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

  const url = serveUrl(options);
  if (!(await waitForUrl(url))) {
    if (pidExists(child.pid)) {
      try {
        process.kill(child.pid, "SIGTERM");
      } catch {
        // The child may have exited between the liveness check and signal.
      }
    }
    closeSync(logFd);
    throw new Error(`Architext background serve did not become reachable at ${url}. Check ${logPath}`);
  }

  await writeServeState(target, {
    target: path.resolve(target),
    pid: child.pid,
    host: options.host,
    port: options.port,
    url,
    logPath,
    startedAt: new Date().toISOString()
  });
  closeSync(logFd);
  console.log(`Serving Architext for ${target} in the background`);
  console.log(`Open ${formatServeLink(url)}`);
  if (options.open && !options.noOpen) {
    const opened = await openSystemBrowser(url);
    if (!opened.ok) console.error(`Browser launch failed: ${opened.message}`);
  }
}

async function serveStatus(target) {
  const state = await readServeState(target);
  if (!state) {
    console.log(`No recorded Architext background server for ${target}`);
    return;
  }
  const reachable = !(await staleServeState(state));
  if (!reachable) {
    await removeServeState(target);
    console.log(`Removed stale Architext background server record for ${target}`);
    return;
  }
  console.log(`Architext is serving ${state.target}`);
  console.log(`PID: ${state.pid}`);
  console.log(`Open ${formatServeLink(state.url)}`);
  console.log(`Logs: ${state.logPath}`);
}

async function stopServe(target) {
  const state = await readServeState(target);
  if (!state) {
    console.log(`No recorded Architext background server for ${target}`);
    return;
  }
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
  await removeServeState(target);
  console.log(`Stopped Architext background server for ${target}`);
}

export async function runServeLifecycle({ target, options, createViewerServer, cliEntryPath }) {
  if (options.serveStatus) {
    await serveStatus(target);
    return;
  }
  if (options.serveStop) {
    await stopServe(target);
    return;
  }
  if (options.background) {
    await serveBackground({ target, options, cliEntryPath });
    return;
  }
  await serveForeground({ target, options, createViewerServer });
}
