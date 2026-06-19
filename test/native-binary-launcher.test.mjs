// Unit + integration tests for the 1.7.0 native-binary launcher bridge.
//
//   1. platform-key + resolution logic in isolation (no real install).
//   2. fallback-to-JS proof: `node tools/architext-adopt.mjs version` with NO
//      optional binary present runs the bundled Node CLI.
//   3. native-exec proof: point the resolver at the freshly built
//      `target/release/architext` via a temp shim package → the launcher execs
//      the native binary and forwards argv + exit code.

import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, mkdirSync, writeFileSync, rmSync, copyFileSync, chmodSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  SUPPORTED_TARGETS,
  platformKey,
  binaryName,
  resolveNativeBinary
} from "../tools/native-binary-resolver.mjs";

const repoRoot = path.resolve(import.meta.dirname, "..");
const launcher = path.join(repoRoot, "tools", "architext-adopt.mjs");

test("platformKey composes process.platform-process.arch", () => {
  assert.equal(platformKey("darwin", "arm64"), "darwin-arm64");
  assert.equal(platformKey("win32", "x64"), "win32-x64");
});

test("binaryName is architext.exe only on win32", () => {
  assert.equal(binaryName("win32"), "architext.exe");
  assert.equal(binaryName("darwin"), "architext");
  assert.equal(binaryName("linux"), "architext");
});

test("the five supported targets are exactly the documented matrix", () => {
  assert.deepEqual(Object.keys(SUPPORTED_TARGETS).sort(), [
    "darwin-arm64",
    "darwin-x64",
    "linux-arm64",
    "linux-x64",
    "win32-x64"
  ]);
});

test("resolveNativeBinary returns null for an unsupported platform", () => {
  const got = resolveNativeBinary({ platform: "sunos", arch: "sparc" });
  assert.equal(got, null);
});

test("resolveNativeBinary returns null when the optionalDependency is not installed", () => {
  // A requireFn whose .resolve always throws simulates "package not installed".
  const requireFn = {
    resolve() {
      throw new Error("Cannot find module");
    }
  };
  const got = resolveNativeBinary({ platform: "linux", arch: "x64", requireFn });
  assert.equal(got, null);
});

test("resolveNativeBinary resolves the matching package binary specifier", () => {
  const calls = [];
  const requireFn = {
    resolve(spec) {
      calls.push(spec);
      return `/fake/node_modules/${spec}`;
    }
  };
  const got = resolveNativeBinary({ platform: "darwin", arch: "arm64", requireFn });
  assert.equal(got, "/fake/node_modules/@robotaccomplice/architext-darwin-arm64/architext");
  assert.deepEqual(calls, ["@robotaccomplice/architext-darwin-arm64/architext"]);
});

test("resolveNativeBinary appends .exe for win32", () => {
  const requireFn = { resolve: (spec) => `/fake/${spec}` };
  const got = resolveNativeBinary({ platform: "win32", arch: "x64", requireFn });
  assert.equal(got, "/fake/@robotaccomplice/architext-win32-x64/architext.exe");
});

test("launcher falls back to the bundled JS CLI when no native binary is installed", () => {
  // This repo's node_modules has no @robotaccomplice/architext-<key> optionalDep,
  // so the launcher must take the JS fallback path. `version` prints the package
  // version from the JS CLI.
  const out = execFileSync(process.execPath, [launcher, "version"], {
    cwd: repoRoot,
    encoding: "utf8"
  }).trim();
  // JS CLI emits the package.json version. Assert it is a semver, proving the
  // JS path ran (the native binary, if any, would print its own 0.0.0 cargo
  // version — but more importantly, no native binary is installed here).
  assert.match(out, /^\d+\.\d+\.\d+$/);
});

test("launcher execs the native binary (argv + exit code forwarded) when one is resolvable", (t) => {
  const builtBinary = path.join(repoRoot, "target", "release", "architext");
  if (!existsSync(builtBinary)) {
    t.skip("native binary not built (run: cargo build --release -p architext-cli)");
    return;
  }

  // Build a temp node_modules shim so the launcher's require.resolve finds a
  // package matching THIS host's platform key, pointing at the real binary.
  const key = platformKey();
  const pkgName = SUPPORTED_TARGETS[key];
  if (!pkgName) {
    t.skip(`host platform ${key} is not in the supported matrix`);
    return;
  }

  const tmp = mkdtempSync(path.join(os.tmpdir(), "architext-native-"));
  t.after(() => rmSync(tmp, { recursive: true, force: true }));

  const pkgDir = path.join(tmp, "node_modules", pkgName);
  mkdirSync(pkgDir, { recursive: true });
  writeFileSync(
    path.join(pkgDir, "package.json"),
    JSON.stringify({ name: pkgName, version: "0.0.0" }, null, 2)
  );
  const binName = binaryName();
  const binDest = path.join(pkgDir, binName);
  copyFileSync(builtBinary, binDest);
  chmodSync(binDest, 0o755);

  // Copy the launcher + resolver into the temp tree's tools/ so that
  // createRequire(import.meta.url) resolves against the temp node_modules.
  const tmpTools = path.join(tmp, "tools");
  mkdirSync(tmpTools, { recursive: true });
  copyFileSync(launcher, path.join(tmpTools, "architext-adopt.mjs"));
  copyFileSync(
    path.join(repoRoot, "tools", "native-binary-resolver.mjs"),
    path.join(tmpTools, "native-binary-resolver.mjs")
  );

  const tmpLauncher = path.join(tmpTools, "architext-adopt.mjs");

  // `version` is a fast, side-effect-free command. The native binary prints its
  // cargo version "0.0.0"; the JS fallback would print the package.json semver.
  // Getting "0.0.0" proves the native binary actually ran.
  const result = spawnSync(process.execPath, [tmpLauncher, "version"], {
    encoding: "utf8"
  });
  assert.equal(result.status, 0, `exit code forwarded; stderr=${result.stderr}`);
  assert.equal(result.stdout.trim(), "0.0.0", "native binary executed (cargo version)");

  // argv + non-zero exit forwarding: a bogus argument makes the native binary
  // exit 1, and its stderr names the exact argument we passed — proving argv
  // reached the native process.
  const fail = spawnSync(process.execPath, [tmpLauncher, "definitely-not-a-command"], {
    encoding: "utf8"
  });
  assert.equal(fail.status, 1, "non-zero exit code forwarded from native binary");
  assert.match(fail.stderr, /definitely-not-a-command/, "argv forwarded to native binary");
});
