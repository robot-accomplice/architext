import assert from "node:assert/strict";
import test from "node:test";
import { compareVersions, parseRefreshSelection, runPackageUpdateCheck } from "../src/adapters/cli/update-check.mjs";

test("package update version comparison uses semantic version precedence", () => {
  assert.equal(compareVersions("1.4.5", "1.4.4"), 1);
  assert.equal(compareVersions("1.4.4", "1.4.4"), 0);
  assert.equal(compareVersions("1.4.3", "1.4.4"), -1);
  assert.equal(compareVersions("v2.0.0", "1.9.9"), 1);
});

test("package update refresh selection supports all, none, indexes, and ids", () => {
  const instances = [
    { id: "aaa111" },
    { id: "bbb222" },
    { id: "ccc333" }
  ];

  assert.deepEqual(parseRefreshSelection("", instances), ["aaa111", "bbb222", "ccc333"]);
  assert.deepEqual(parseRefreshSelection("none", instances), []);
  assert.deepEqual(parseRefreshSelection("2, ccc333", instances), ["bbb222", "ccc333"]);
  assert.deepEqual(parseRefreshSelection("aaa111 aaa111", instances), ["aaa111"]);
  assert.throws(() => parseRefreshSelection("missing", instances), /Unknown instance selection/);
});

test("package update check installs newer package and refreshes selected instances", async () => {
  const calls = [];
  const answers = ["yes", "2, aaa111"];
  await runPackageUpdateCheck({
    currentVersion: "1.4.4",
    options: { yes: false },
    cwd: "/repo",
    runCommand: (command, args, cwd) => calls.push({ type: "run", command, args, cwd }),
    tryRunCommand: (command, args, cwd) => {
      calls.push({ type: "try", command, args, cwd });
      if (args[0] === "view") return { ok: true, output: "1.4.5" };
      return {
        ok: true,
        output: JSON.stringify({
          instances: [
            { id: "aaa111", url: "http://127.0.0.1:4317/", target: "/one" },
            { id: "bbb222", url: "http://127.0.0.1:4318/", target: "/two" }
          ]
        })
      };
    },
    promptLine: async () => answers.shift()
  });

  assert.deepEqual(
    calls.filter((call) => call.type === "run").map((call) => [call.command, call.args]),
    [
      ["npm", ["install", "-g", "@robotaccomplice/architext@1.4.5"]],
      ["architext", ["serve", "--refresh", "--instance", "bbb222"]],
      ["architext", ["serve", "--refresh", "--instance", "aaa111"]]
    ]
  );
});

test("package update check does nothing when npm latest is current", async () => {
  const runs = [];
  await runPackageUpdateCheck({
    currentVersion: "1.4.4",
    options: { yes: true },
    runCommand: (...args) => runs.push(args),
    tryRunCommand: () => ({ ok: true, output: "1.4.4" }),
    promptLine: async () => {
      throw new Error("prompt should not be called");
    }
  });
  assert.deepEqual(runs, []);
});
