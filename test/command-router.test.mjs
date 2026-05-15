import assert from "node:assert/strict";
import test from "node:test";
import { createCommandHandlers, routeCommand } from "../src/adapters/cli/command-router.mjs";

test("command router maps lifecycle aliases to the sync use case", async () => {
  const calls = [];
  const handlers = createCommandHandlers({
    sync: async (target, options) => calls.push(["sync", target, options.command])
  });

  await routeCommand({ options: { command: "install" }, target: "/tmp/repo", handlers });
  await routeCommand({ options: { command: "upgrade" }, target: "/tmp/repo", handlers });
  await routeCommand({ options: { command: "migrate" }, target: "/tmp/repo", handlers });

  assert.deepEqual(calls, [
    ["sync", "/tmp/repo", "install"],
    ["sync", "/tmp/repo", "upgrade"],
    ["sync", "/tmp/repo", "migrate"]
  ]);
});

test("command router fails loudly for unknown commands", async () => {
  await assert.rejects(
    routeCommand({ options: { command: "unknown" }, target: "/tmp/repo", handlers: createCommandHandlers({}) }),
    /Unknown command: unknown/
  );
});
