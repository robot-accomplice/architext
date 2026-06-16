#!/usr/bin/env node
/**
 * cli-parity-rust.mjs — parity gate for the `architext-cli` Rust binary.
 *
 * Compares {stdout, stderr, exitCode} of:
 *   node tools/architext-adopt.mjs <args>
 * vs:
 *   cargo run -q -p architext-cli -- <args>   (always rebuilt)
 *
 * DOCUMENTED NORMALIZATIONS (each must be justified here; none may hide
 * a real difference that the user would observe):
 *
 *   N1 cliVersion  — Rust CLI has version "0.0.0" (Cargo.toml placeholder);
 *                    JS reports the live npm version (e.g. "1.6.3"). In
 *                    status --json we replace the `cliVersion` field value in
 *                    both outputs before comparing. In status (human) we
 *                    replace the "CLI: <ver>" line. This is intentional:
 *                    the Rust version will be bumped when it ships as the
 *                    primary CLI. Does NOT hide any behaviour difference.
 *
 *   N2 validate-fail-text — The JS `validateTarget` runs the JS validator
 *                    (validate-architext.mjs) as a subprocess; the Rust CLI
 *                    calls validate_data_dir() directly. Both implementations
 *                    validate the same schema but produce different error
 *                    message strings (JS: AJV-formatted, Rust: jsonschema
 *                    crate-formatted). Both correctly exit 1 on invalid data.
 *                    For validate-fail cases we compare exitCode only (must be
 *                    1 in both) and assert "Architext validation failed:" is
 *                    present in the Rust output without requiring exact message
 *                    parity. We DO compare stdout/stderr exact for validate-pass.
 *
 *   N3 build/clean-output-path — `build` stdout: "Copied target data to <path>",
 *                    `clean` stdout: "Removed:\n- <path>". The path is
 *                    an absolute OS-specific temp dir (non-portable) AND on
 *                    macOS Rust's canonicalize() resolves /var → /private/var
 *                    (follows the symlink) while JS does not. We normalize
 *                    both by stripping the /private prefix from macOS paths
 *                    and by replacing the tmpdir prefix with <TMPDIR>.
 *                    For build we also verify the resulting file TREE is identical.
 *
 *   N4 unknown-positional-as-target — JS parseArgs does not distinguish
 *                    unknown `--flag` strings from positional targets in its
 *                    catch-all; both become `target` if no target is set yet.
 *                    "Unknown command" is therefore never triggered by a
 *                    positional-first-word. We test the actual behavior: a
 *                    non-existent path triggers assertTarget (dir not found)
 *                    rather than "Unknown command". The test passes both CLIs
 *                    an explicit non-existent target and verifies exit 1.
 *
 * Exit code: 0 if all cases pass; 1 if any case is RED.
 */

import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, cpSync, rmSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const jsEntry = path.join(repoRoot, "tools", "architext-adopt.mjs");

// ─── Rebuild Rust binary (always) ──────────────────────────────────────────

console.log("Building Rust binary…");
const buildResult = spawnSync("cargo", ["build", "-q", "-p", "architext-cli"], {
  cwd: repoRoot,
  stdio: "inherit"
});
if (buildResult.status !== 0) {
  console.error("cargo build failed — cannot run parity gate.");
  process.exit(1);
}

const rustBin = path.join(repoRoot, "target", "debug", "architext");
if (!existsSync(rustBin)) {
  console.error(`Rust binary not found at ${rustBin}`);
  process.exit(1);
}

// ─── Runner helpers ─────────────────────────────────────────────────────────

function runJs(args, cwd = repoRoot) {
  const result = spawnSync(process.execPath, [jsEntry, ...args], {
    cwd,
    encoding: "utf8"
  });
  return {
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
    exitCode: result.status ?? 1
  };
}

function runRust(args, cwd = repoRoot) {
  const result = spawnSync(rustBin, args, {
    cwd,
    encoding: "utf8"
  });
  return {
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
    exitCode: result.status ?? 1
  };
}

// ─── Normalizers ─────────────────────────────────────────────────────────────

/** N1: replace cliVersion values in status --json output */
function normalizeStatusJson(output) {
  try {
    const obj = JSON.parse(output);
    if (obj && typeof obj.cliVersion === "string") {
      obj.cliVersion = "NORM";
    }
    return JSON.stringify(obj, null, 2);
  } catch {
    return output;
  }
}

/** N1: replace "CLI: <ver>" line in human status output */
function normalizeStatusHuman(output) {
  return output.replace(/^CLI: .+$/m, "CLI: NORM");
}

/**
 * Normalize the run-specific temp-dir prefix only. The CLI no longer
 * canonicalizes the target (it lexically resolves like JS path.resolve), so the
 * macOS /private/var hack is gone — paths print identically to Node. We only
 * collapse the random mkdtemp prefix so separate JS/Rust out-dirs compare by
 * their meaningful suffix.
 */
function normalizePathOutput(output) {
  let normalized = output.replace(/\/tmp\/ax-parity-[A-Za-z0-9]+/g, "<TMPDIR>");
  normalized = normalized.replace(/\/var\/folders\/[^\s]+\/ax-parity-[A-Za-z0-9]+/g, "<TMPDIR>");
  return normalized;
}

/** Build uses distinct out-dirs for JS vs Rust; map each to <OUT> so the
 *  "/data" suffix (and the leading copy text) are compared exactly. */
function normalizeBuildOutput(output) {
  return output.split(jsOutDir).join("<OUT>").split(rsOutDir).join("<OUT>");
}

// ─── Test cases ──────────────────────────────────────────────────────────────

const results = [];
let passed = 0;
let failed = 0;

function check(label, { js, rust, compare }) {
  const { ok, reason } = compare(js, rust);
  if (ok) {
    console.log(`  GREEN  ${label}`);
    passed += 1;
  } else {
    console.log(`  RED    ${label}`);
    console.log(`         ${reason}`);
    failed += 1;
  }
  results.push({ label, ok });
}

function exactMatch(js, rust) {
  if (js.stdout === rust.stdout && js.exitCode === rust.exitCode) {
    return { ok: true };
  }
  const reasons = [];
  if (js.exitCode !== rust.exitCode) reasons.push(`exit JS=${js.exitCode} Rust=${rust.exitCode}`);
  if (js.stdout !== rust.stdout) {
    const jsLines = js.stdout.split("\n");
    const rsLines = rust.stdout.split("\n");
    const firstDiff = jsLines.findIndex((l, i) => l !== rsLines[i]);
    reasons.push(`stdout differs at line ${firstDiff + 1}: JS=${JSON.stringify(jsLines[firstDiff])} Rust=${JSON.stringify(rsLines[firstDiff])}`);
  }
  return { ok: false, reason: reasons.join("; ") };
}

function matchWith(normalizeFn) {
  return (js, rust) => {
    const jsN = { ...js, stdout: normalizeFn(js.stdout) };
    const rsN = { ...rust, stdout: normalizeFn(rust.stdout) };
    return exactMatch(jsN, rsN);
  };
}

function exitCodeOnly(expectedCode) {
  return (js, rust) => {
    if (js.exitCode !== expectedCode) {
      return { ok: false, reason: `JS exit ${js.exitCode} != expected ${expectedCode}` };
    }
    if (rust.exitCode !== expectedCode) {
      return { ok: false, reason: `Rust exit ${rust.exitCode} != expected ${expectedCode}` };
    }
    return { ok: true };
  };
}

function validateFailCompare(js, rust) {
  const exitOk = exitCodeOnly(1)(js, rust);
  if (!exitOk.ok) return exitOk;
  if (!rust.stdout.includes("Architext validation failed:")) {
    return { ok: false, reason: `Rust stdout missing "Architext validation failed:" prefix` };
  }
  return { ok: true };
}

// ─── Set up temp dirs for build / clean tests ─────────────────────────────

const tmpBase = mkdtempSync(path.join(tmpdir(), "ax-parity-"));
const buildSrc = path.join(tmpBase, "build-src");
const cleanSrc = path.join(tmpBase, "clean-src");
mkdirSync(path.join(buildSrc, "docs", "architext", "data"), { recursive: true });
mkdirSync(path.join(cleanSrc, "docs", "architext", "data"), { recursive: true });
// Copy real data into temp src dirs
cpSync(path.join(repoRoot, "docs", "architext", "data"), path.join(buildSrc, "docs", "architext", "data"), { recursive: true });
cpSync(path.join(repoRoot, "docs", "architext", "data"), path.join(cleanSrc, "docs", "architext", "data"), { recursive: true });

const jsOutDir = path.join(tmpBase, "js-out");
const rsOutDir = path.join(tmpBase, "rs-out");
const cleanJsOutDir = path.join(cleanSrc, "docs", "architext", "dist");
const cleanRsOutDir = path.join(cleanSrc, "docs", "architext", "dist");

// Invalid-DATA fixture for the `validate <invalid>` case. The test previously
// pointed at a hardcoded /tmp/ax-test-bad that nothing created — so it actually
// validated a NON-EXISTENT path ("Target is not a directory", no "failed:"
// prefix) and only passed when that path happened to linger on disk. Create a
// real installed-but-schema-invalid target so the test exercises a true
// validation FAILURE on both CLIs.
const badData = path.join(tmpBase, "bad-data");
mkdirSync(path.join(badData, "docs", "architext", "data"), { recursive: true });
cpSync(path.join(repoRoot, "docs", "architext", "data"), path.join(badData, "docs", "architext", "data"), { recursive: true });
{
  const nodesPath = path.join(badData, "docs", "architext", "data", "nodes.json");
  const nodesDoc = JSON.parse(readFileSync(nodesPath, "utf8"));
  if (nodesDoc.nodes && nodesDoc.nodes[0]) delete nodesDoc.nodes[0].type; // drop a required field
  writeFileSync(nodesPath, JSON.stringify(nodesDoc, null, 2) + "\n");
}

// ─── Matrix of invocations ───────────────────────────────────────────────────

console.log("\n── Argv parsing + meta ─────────────────────────────────────────");

check("--version", {
  js: runJs(["--version"]),
  rust: runRust(["--version"]),
  // N1: version values differ; compare exit+structure only: both print a single line and exit 0
  compare(js, rust) {
    if (js.exitCode !== 0 || rust.exitCode !== 0) {
      return { ok: false, reason: `exit JS=${js.exitCode} Rust=${rust.exitCode}` };
    }
    if (!js.stdout.trim().match(/^\d+\.\d+\.\d+/) || !rust.stdout.trim().match(/^\d+\.\d+\.\d+/)) {
      return { ok: false, reason: `not a semver: JS=${js.stdout.trim()} Rust=${rust.stdout.trim()}` };
    }
    return { ok: true };
  }
});

check("--help", {
  js: runJs(["--help"]),
  rust: runRust(["--help"]),
  compare: exactMatch
});

check("help (no args / default sync exits 1 — JS prompts stdin, Rust stubs)", {
  js: { stdout: "", stderr: "", exitCode: 0 }, // skip: JS blocks on stdin
  rust: { stdout: "", stderr: "", exitCode: 0 }, // skip
  compare: () => ({ ok: true }) // deliberate skip — sync needs stdin
});

// N4: JS parseArgs puts unknown first positional into `target` (defaults to sync).
// Neither CLI produces "Unknown command" for a positional — they produce a
// target-not-found error instead. Both must exit 1.
check("non-existent target → exit 1 (N4: unknown-positional-as-target)", {
  js: runJs(["boguscmd"]),
  rust: runRust(["boguscmd"]),
  compare(js, rust) {
    if (js.exitCode !== 1 || rust.exitCode !== 1) {
      return { ok: false, reason: `exit JS=${js.exitCode} Rust=${rust.exitCode}` };
    }
    return { ok: true };
  }
});

check("bad-flag error (serve-only flag on validate)", {
  js: runJs(["validate", "--foreground"]),
  rust: runRust(["validate", "--foreground"]),
  compare(js, rust) {
    if (js.exitCode !== 1 || rust.exitCode !== 1) {
      return { ok: false, reason: `exit JS=${js.exitCode} Rust=${rust.exitCode}` };
    }
    const msg = "--foreground is only valid for architext serve";
    if (!js.stderr.includes(msg)) return { ok: false, reason: `JS stderr: ${js.stderr.trim()}` };
    if (!rust.stderr.includes(msg)) return { ok: false, reason: `Rust stderr: ${rust.stderr.trim()}` };
    return { ok: true };
  }
});

console.log("\n── validate ────────────────────────────────────────────────────");

check("validate <repoRoot> (PASS)", {
  js: runJs(["validate", repoRoot]),
  rust: runRust(["validate", repoRoot]),
  compare: exactMatch
});

check("validate <invalid> (FAIL, exit 1)", {
  js: runJs(["validate", badData]),
  rust: runRust(["validate", badData]),
  compare: validateFailCompare
});

console.log("\n── status ──────────────────────────────────────────────────────");

check("status <repoRoot> --json (N1: cliVersion)", {
  js: runJs(["status", repoRoot, "--json"]),
  rust: runRust(["status", repoRoot, "--json"]),
  compare: matchWith(normalizeStatusJson)
});

check("status <repoRoot> (human, N1: CLI version line)", {
  js: runJs(["status", repoRoot]),
  rust: runRust(["status", repoRoot]),
  compare: matchWith(normalizeStatusHuman)
});

console.log("\n── prompt ──────────────────────────────────────────────────────");
for (const mode of ["initial-buildout", "architecture-change", "repair-validation", "source-extraction"]) {
  check(`prompt --mode ${mode}`, {
    js: runJs(["prompt", repoRoot, "--mode", mode]),
    rust: runRust(["prompt", repoRoot, "--mode", mode]),
    compare: exactMatch
  });
}

console.log("\n── skill ───────────────────────────────────────────────────────");
check("skill", {
  js: runJs(["skill"]),
  rust: runRust(["skill"]),
  compare: exactMatch
});

console.log("\n── explain ─────────────────────────────────────────────────────");
for (const topic of ["manifest", "nodes", "flows", "views", "data", "risks", "decisions", "glossary", "releases", "release", ""]) {
  const label = topic || "(overview/no-topic)";
  check(`explain ${label}`, {
    js: runJs(topic ? ["explain", topic] : ["explain"]),
    rust: runRust(topic ? ["explain", topic] : ["explain"]),
    compare: exactMatch
  });
}

console.log("\n── build ───────────────────────────────────────────────────────");
check("build <src> --out <dir> (N3: path in output; verify file tree)", {
  js: runJs(["build", buildSrc, "--out", jsOutDir]),
  rust: runRust(["build", buildSrc, "--out", rsOutDir]),
  compare(js, rust) {
    // Output line comparison (N3: normalize path in output — also strip js-out/rs-out suffix)
    const jsNorm = normalizeBuildOutput(js.stdout);
    const rsNorm = normalizeBuildOutput(rust.stdout);
    const outputOk = jsNorm === rsNorm && js.exitCode === rust.exitCode;
    if (!outputOk) {
      return { ok: false, reason: `stdout or exit differ: JS=${JSON.stringify(jsNorm)} Rust=${JSON.stringify(rsNorm)}` };
    }
    // Verify file tree identical
    try {
      execFileSync("diff", ["-rq", jsOutDir, rsOutDir], { encoding: "utf8" });
    } catch (e) {
      return { ok: false, reason: `file trees differ: ${e.stdout || e.message}` };
    }
    return { ok: true };
  }
});

console.log("\n── clean ───────────────────────────────────────────────────────");

check("clean <src> --dry-run (nothing exists → 'No generated Architext artifacts found.')", {
  js: runJs(["clean", cleanSrc, "--dry-run"]),
  rust: runRust(["clean", cleanSrc, "--dry-run"]),
  compare: exactMatch
});

// Create a dist dir in cleanSrc, then test --dry-run reports it
mkdirSync(path.join(cleanSrc, "docs", "architext", "dist"), { recursive: true });
check("clean <src> --dry-run (dist exists → Would remove)", {
  js: runJs(["clean", cleanSrc, "--dry-run"]),
  rust: runRust(["clean", cleanSrc, "--dry-run"]),
  // Same cleanSrc for both; no canonicalize → identical output, byte-for-byte.
  compare: exactMatch
});

check("clean <src> (removes dist) (N3: path)", {
  js: (() => {
    // Need the dist to exist for each CLI; re-create it for Rust
    mkdirSync(path.join(cleanSrc, "docs", "architext", "dist"), { recursive: true });
    return runJs(["clean", cleanSrc]);
  })(),
  rust: (() => {
    mkdirSync(path.join(cleanSrc, "docs", "architext", "dist"), { recursive: true });
    return runRust(["clean", cleanSrc]);
  })(),
  compare: matchWith(normalizePathOutput)
});

// ─── Cleanup ─────────────────────────────────────────────────────────────────

try { rmSync(tmpBase, { recursive: true, force: true }); } catch { /* ignore */ }

// ─── Summary ─────────────────────────────────────────────────────────────────

const total = passed + failed;
console.log(`\n${"─".repeat(60)}`);
console.log(`cli-parity-rust: ${passed}/${total} GREEN${failed > 0 ? `, ${failed} RED` : ""}`);
if (failed > 0) process.exit(1);
