import { execFileSync } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

function run(command, args, options = {}) {
  return execFileSync(command, args, {
    encoding: "utf8",
    stdio: options.stdio ?? ["ignore", "pipe", "pipe"],
    shell: process.platform === "win32",
    ...options
  });
}

function packCurrentPackage() {
  const output = run("npm", ["pack", "--json", "--silent"]);
  const pack = JSON.parse(output.slice(output.indexOf("[")));
  return pack[0].filename;
}

async function main() {
  const repoRoot = path.resolve(import.meta.dirname, "..");
  const packageFile = packCurrentPackage();
  const prefix = await mkdtemp(path.join(tmpdir(), "architext-prefix-"));
  const target = await mkdtemp(path.join(tmpdir(), "architext-target-"));
  try {
    run("npm", ["install", "-g", "--prefix", prefix, path.join(repoRoot, packageFile)], { stdio: "inherit" });
    const architext = path.join(prefix, "bin", process.platform === "win32" ? "architext.cmd" : "architext");
    run(architext, ["--version"], { stdio: "inherit" });
    run(architext, ["sync", target, "--yes", "--branch", "none"], { stdio: "inherit" });
    run(architext, ["doctor", target, "--dry-run"], { stdio: "inherit" });
    run(architext, ["validate", target], { stdio: "inherit" });
  } finally {
    await rm(path.join(repoRoot, packageFile), { force: true });
    await rm(prefix, { recursive: true, force: true });
    await rm(target, { recursive: true, force: true });
  }
}

await main();
