// Per-platform native-package smoke: prove a STAMPED, self-contained Architext
// package works when run from OUTSIDE the source repo — i.e. the binary execs on
// this platform, validation uses the embedded schemas (no repo-relative
// viewer/schema), and serve resolves the viewer dist co-located beside the
// binary (<exe_dir>/dist). Cross-platform (spawn/http/kill all work on
// win/macos/linux), so the same script runs in every matrix job.
//
// Usage: node scripts/smoke-native-package.mjs <packageDir> <repoRoot>
//   <packageDir> — a dir stamped by build-binary-packages.mjs (binary + dist/)
//   <repoRoot>   — a repo with docs/architext/data to validate + serve
import { spawn, spawnSync } from "node:child_process";
import { get } from "node:http";
import { tmpdir } from "node:os";
import path from "node:path";

const [pkgDirArg, repoRootArg] = process.argv.slice(2);
if (!pkgDirArg || !repoRootArg) {
  console.error("usage: smoke-native-package.mjs <packageDir> <repoRoot>");
  process.exit(2);
}

const binName = process.platform === "win32" ? "architext.exe" : "architext";
const bin = path.resolve(pkgDirArg, binName);
const repo = path.resolve(repoRootArg);
// Run from OUTSIDE the repo so cwd carries no repo-relative assets — the binary
// must rely on its embedded schemas + co-located dist, not the source tree.
const cwd = tmpdir();
const PORT = 8731;

function fail(msg) {
  console.error(`✗ ${msg}`);
  process.exit(1);
}

// 1) The binary execs on this platform.
let r = spawnSync(bin, ["--version"], { cwd, encoding: "utf8" });
if (r.status !== 0) fail(`--version failed (status ${r.status}): ${r.stderr || r.error}`);
console.log(`✓ ${binName} --version → ${(r.stdout || "").trim()}`);

// 2) Validation uses the EMBEDDED schemas (run from /tmp, no viewer/schema on cwd).
r = spawnSync(bin, ["validate", repo], { cwd, encoding: "utf8" });
if (r.status !== 0) fail(`validate failed:\n${r.stdout}\n${r.stderr}`);
console.log("✓ validate passed using embedded schemas");

// 3) serve resolves the CO-LOCATED viewer dist (<exe_dir>/dist) and serves it.
const srv = spawn(bin, ["serve", repo, "--port", String(PORT)], { cwd, stdio: "ignore" });
srv.on("error", (e) => fail(`could not spawn serve: ${e.message}`));

const deadline = Date.now() + 30_000;
function poll() {
  get({ host: "127.0.0.1", port: PORT, path: "/" }, (res) => {
    let body = "";
    res.on("data", (c) => (body += c));
    res.on("end", () => {
      srv.kill();
      if (body.includes("<title>Architext</title>")) {
        console.log("✓ serve returned the viewer index from the co-located dist");
        process.exit(0);
      }
      fail(`serve responded but not with the viewer index (first 120 chars): ${body.slice(0, 120)}`);
    });
  }).on("error", () => {
    if (Date.now() > deadline) {
      srv.kill();
      fail("serve never became reachable within 30s");
    }
    setTimeout(poll, 500);
  });
}
setTimeout(poll, 750);
