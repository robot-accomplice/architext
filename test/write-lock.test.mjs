import assert from "node:assert/strict";
import { mkdir, rm, stat, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { waitForDataWritesToSettle, withTargetWriteLock, writeLockPath } from "../src/adapters/cli/write-lock.mjs";

async function createTarget() {
  const root = await import("node:fs/promises").then(({ mkdtemp }) => mkdtemp(path.join(os.tmpdir(), "architext-write-lock-")));
  await mkdir(path.join(root, "docs", "architext", "data"), { recursive: true });
  await writeFile(path.join(root, "docs", "architext", "data", "manifest.json"), "{}\n", "utf8");
  return root;
}

test("waitForDataWritesToSettle waits for temporary writes to finish", async () => {
  const target = await createTarget();
  const temporaryFile = path.join(target, "docs", "architext", "data", ".manifest.json.1.tmp");
  await writeFile(temporaryFile, "{}\n", "utf8");

  const waiting = waitForDataWritesToSettle({ target, settleMs: 20, pollMs: 5, timeoutMs: 500 });
  await new Promise((resolve) => setTimeout(resolve, 30));
  await rm(temporaryFile);
  await waiting;
});

test("withTargetWriteLock creates and removes a target-scoped lock", async () => {
  const target = await createTarget();
  let lockVisible = false;

  await withTargetWriteLock(target, async () => {
    lockVisible = Boolean(await stat(writeLockPath(target)).catch(() => null));
  }, { settleMs: 5, pollMs: 5, timeoutMs: 500 });

  assert.equal(lockVisible, true);
  assert.equal(await stat(writeLockPath(target)).catch(() => null), null);
});

test("withTargetWriteLock times out behind an active lock", async () => {
  const target = await createTarget();
  await mkdir(writeLockPath(target), { recursive: true });
  await writeFile(path.join(writeLockPath(target), "owner.json"), `${JSON.stringify({
    pid: process.pid,
    createdAtMs: Date.now()
  })}\n`, "utf8");

  await assert.rejects(
    withTargetWriteLock(target, async () => {}, { settleMs: 5, pollMs: 5, timeoutMs: 40, staleMs: 10000 }),
    /Timed out waiting for Architext write lock/
  );
});

test("withTargetWriteLock recovers stale locks", async () => {
  const target = await createTarget();
  await mkdir(writeLockPath(target), { recursive: true });
  await writeFile(path.join(writeLockPath(target), "owner.json"), `${JSON.stringify({
    pid: 99999999,
    createdAtMs: Date.now() - 10000
  })}\n`, "utf8");

  let ran = false;
  await withTargetWriteLock(target, async () => {
    ran = true;
  }, { settleMs: 5, pollMs: 5, timeoutMs: 500, staleMs: 1 });

  assert.equal(ran, true);
});
