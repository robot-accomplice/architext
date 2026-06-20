// Package-shape smoke for the Rust-only (1.7.0) package: assert `npm pack` ships
// the native-binary launcher + the embedded-schema source + the viewer dist, and
// that NO JS engine/impl leaks (the cutover removed it). The full
// install-and-run-native smoke runs in the release-binaries CI, where the
// per-platform binary (the optionalDependency) is actually built.
import { execFileSync } from "node:child_process";

// `--ignore-scripts` so the `prepack` (Trunk build) output doesn't pollute the
// `--json` stream; the dist is expected to already be built (release flow runs
// `build:viewer` before this smoke).
const out = execFileSync("npm", ["pack", "--dry-run", "--json", "--ignore-scripts"], {
  encoding: "utf8",
  shell: process.platform === "win32",
});
const entries = JSON.parse(out)[0].files.map((f) => f.path);

const mustHave = [
  "tools/architext-adopt.mjs",
  "tools/native-binary-resolver.mjs",
  "viewer/schema/manifest.schema.json",
  "crates/architext-viewer/dist/index.html",
];
const mustNotHave = [
  (p) => p.startsWith("src/"),
  (p) => p.startsWith("viewer/src/"),
  (p) => p.endsWith(".test.mjs"),
  (p) => p.includes("route-diff-harness") || p.includes("parity"),
];

const missing = mustHave.filter((p) => !entries.includes(p));
const leaked = entries.filter((p) => mustNotHave.some((bad) => bad(p)));

if (missing.length || leaked.length) {
  if (missing.length) console.error(`✗ pack missing required files:\n  ${missing.join("\n  ")}`);
  if (leaked.length) console.error(`✗ pack leaked JS engine/impl files:\n  ${leaked.join("\n  ")}`);
  process.exit(1);
}
console.log(
  `✓ pack shape OK: ${entries.length} files — launcher + embedded schemas + viewer dist present, no JS engine/impl`,
);
