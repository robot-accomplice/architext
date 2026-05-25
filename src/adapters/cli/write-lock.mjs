import { constants } from "node:fs";
import { access, mkdir, readdir, readFile, rm, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import { architextDir, dataDir as defaultDataDir } from "../../domain/lifecycle/target-layout.mjs";

const defaultSettleMs = 300;
const defaultPollMs = 50;
const defaultTimeoutMs = 10000;
const defaultStaleMs = 120000;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export function writeLockPath(target) {
  return path.join(architextDir(target), ".architext-write.lock");
}

async function pathExists(entryPath) {
  try {
    await access(entryPath, constants.F_OK);
    return true;
  } catch {
    return false;
  }
}

async function dataFileEntries(root) {
  const rootStat = await stat(root).catch(() => null);
  if (!rootStat?.isDirectory()) return [];
  const entries = [];
  async function visit(directory) {
    for (const dirent of await readdir(directory, { withFileTypes: true })) {
      const entryPath = path.join(directory, dirent.name);
      if (dirent.isDirectory()) {
        await visit(entryPath);
        continue;
      }
      if (dirent.name.endsWith(".json") || dirent.name.endsWith(".tmp")) entries.push(entryPath);
    }
  }
  await visit(root);
  return entries.sort();
}

async function dataSignature(root) {
  const files = await dataFileEntries(root);
  const parts = [];
  let hasTemporaryWrite = false;
  for (const file of files) {
    const fileStat = await stat(file).catch(() => null);
    if (!fileStat?.isFile()) continue;
    if (file.endsWith(".tmp")) hasTemporaryWrite = true;
    parts.push(`${path.relative(root, file)}:${fileStat.size}:${fileStat.mtimeMs}`);
  }
  return { hasTemporaryWrite, signature: parts.join("|") };
}

export async function waitForDataWritesToSettle({
  target,
  dataDir = defaultDataDir,
  settleMs = defaultSettleMs,
  pollMs = defaultPollMs,
  timeoutMs = defaultTimeoutMs,
  now = () => Date.now()
}) {
  const root = dataDir(target);
  const deadline = now() + timeoutMs;
  let stableSince = 0;
  let previous = "";

  while (now() <= deadline) {
    const { hasTemporaryWrite, signature } = await dataSignature(root);
    if (!hasTemporaryWrite && signature === previous) {
      stableSince ||= now();
      if (now() - stableSince >= settleMs) return;
    } else {
      previous = signature;
      stableSince = 0;
    }
    await sleep(pollMs);
  }

  throw new Error(`Timed out waiting for Architext data writes to settle in ${root}`);
}

function processAlive(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error?.code === "EPERM";
  }
}

async function staleLock(lockDir, staleMs, now) {
  const ownerFile = path.join(lockDir, "owner.json");
  const lockStat = await stat(lockDir).catch(() => null);
  if (!lockStat?.isDirectory()) return true;
  const owner = await readFile(ownerFile, "utf8").then(JSON.parse).catch(() => null);
  if (owner?.pid && processAlive(owner.pid)) return false;
  const age = now() - (owner?.createdAtMs ?? lockStat.mtimeMs);
  return age >= staleMs;
}

async function removeTree(entryPath) {
  for (let attempt = 0; attempt < 5; attempt += 1) {
    try {
      await rm(entryPath, { recursive: true, force: true });
      return;
    } catch (error) {
      if (!["ENOTEMPTY", "EBUSY", "EPERM"].includes(error?.code) || attempt === 4) throw error;
      await sleep((attempt + 1) * 10);
    }
  }
}

async function reclaimStaleLock(lockDir, staleMs, now) {
  const reclaimMarker = path.join(lockDir, ".reclaiming");
  try {
    await mkdir(reclaimMarker);
  } catch (error) {
    if (["ENOENT", "ENOTDIR", "EINVAL"].includes(error?.code)) return false;
    if (error?.code === "EEXIST") return false;
    throw error;
  }
  if (await staleLock(lockDir, staleMs, now)) {
    await removeTree(lockDir);
    return true;
  }
  await removeTree(reclaimMarker);
  return false;
}

async function acquireLock(target, { pollMs, timeoutMs, staleMs, now }) {
  const lockDir = writeLockPath(target);
  const deadline = now() + timeoutMs;
  await mkdir(path.dirname(lockDir), { recursive: true });

  while (now() <= deadline) {
    try {
      await mkdir(lockDir);
      try {
        await writeFile(path.join(lockDir, "owner.json"), `${JSON.stringify({
          pid: process.pid,
          createdAt: new Date(now()).toISOString(),
          createdAtMs: now()
        }, null, 2)}\n`, "utf8");
      } catch (error) {
        await removeTree(lockDir);
        throw error;
      }
      return lockDir;
    } catch (error) {
      if (error?.code !== "EEXIST") throw error;
      if (await staleLock(lockDir, staleMs, now)) {
        await reclaimStaleLock(lockDir, staleMs, now);
        continue;
      }
      await sleep(pollMs);
    }
  }

  throw new Error(`Timed out waiting for Architext write lock: ${lockDir}`);
}

export async function withTargetWriteLock(target, callback, options = {}) {
  const settings = {
    pollMs: options.pollMs ?? defaultPollMs,
    settleMs: options.settleMs ?? defaultSettleMs,
    timeoutMs: options.timeoutMs ?? defaultTimeoutMs,
    staleMs: options.staleMs ?? defaultStaleMs,
    now: options.now ?? (() => Date.now()),
    dataDir: options.dataDir ?? defaultDataDir
  };

  await waitForDataWritesToSettle({ target, ...settings });
  const lockDir = await acquireLock(target, settings);
  try {
    await waitForDataWritesToSettle({ target, ...settings });
    return await callback();
  } finally {
    await removeTree(lockDir);
  }
}
