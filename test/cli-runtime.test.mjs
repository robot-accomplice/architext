import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { assertDirectory, readJson, tryRun, writeJson } from "../src/adapters/cli/runtime.mjs";

test("readJson and writeJson preserve structured CLI data with stable formatting", async () => {
  const dir = await mkdtemp(path.join(tmpdir(), "architext-runtime-"));
  try {
    const file = path.join(dir, "nested", "data.json");
    await writeJson(file, { ok: true, values: [1, 2] });
    assert.deepEqual(await readJson(file), { ok: true, values: [1, 2] });
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("assertDirectory fails loud for missing targets", async () => {
  await assert.rejects(
    () => assertDirectory(path.join(tmpdir(), "architext-missing-target"), "Target"),
    /Target is not a directory/
  );
});

test("tryRun reports failing command output without throwing", () => {
  const result = tryRun(process.execPath, ["--eval", "console.error('bad target'); process.exit(12)"], process.cwd());
  assert.equal(result.ok, false);
  assert.match(result.output, /bad target/);
});
