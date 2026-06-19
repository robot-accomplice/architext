#!/usr/bin/env node
// Generate the per-platform native-binary npm packages for Architext 1.7.0.
//
// Each `@robotaccomplice/architext-<platform>-<arch>` package is a tiny payload:
// a package.json with `os`/`cpu` set (so npm installs only the matching one) and
// the platform's `architext` binary (`architext.exe` on win32). The root package
// declares them as optionalDependencies; the launcher resolves whichever one npm
// installed for the host.
//
// This generator stamps the package DIRECTORIES (package.json + binary) into a
// staging dir; it does NOT publish. Actual binaries are produced by the
// cross-compile CI matrix and are NOT committed to the repo — this script copies
// a binary into place only when one is provided.
//
// Usage:
//   node scripts/build-binary-packages.mjs --binary <path> --target <key> [--out <dir>]
//   node scripts/build-binary-packages.mjs --binaries-dir <dir> [--out <dir>]
//
// --binary/--target stamps a single package from one freshly built binary.
// --binaries-dir scans for `<key>/architext[.exe]` layouts (the CI artifact
// layout) and stamps every package it finds. --out defaults to dist-binaries/.
// Packages with no binary available are still stamped (package.json only) so the
// set is inspectable, but a missing binary is reported as a warning.

import { existsSync, mkdirSync, copyFileSync, cpSync, writeFileSync, chmodSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { SUPPORTED_TARGETS, binaryName } from "../tools/native-binary-resolver.mjs";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const rootPkg = JSON.parse(readFileSync(join(repoRoot, "package.json"), "utf8"));
const VERSION = rootPkg.version;

function parseArgs(argv) {
  const out = { binary: null, target: null, binariesDir: null, outDir: join(repoRoot, "dist-binaries") };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--binary") out.binary = argv[++i];
    else if (a === "--target") out.target = argv[++i];
    else if (a === "--binaries-dir") out.binariesDir = argv[++i];
    else if (a === "--out") out.outDir = resolve(argv[++i]);
    else {
      throw new Error(`Unknown argument: ${a}`);
    }
  }
  return out;
}

// Map a target key to its os/cpu npm fields.
function osCpuFor(key) {
  const [platform, arch] = key.split("-");
  return { os: [platform], cpu: [arch] };
}

function packageJsonFor(key) {
  const name = SUPPORTED_TARGETS[key];
  const { os, cpu } = osCpuFor(key);
  return {
    name,
    version: VERSION,
    description: `Native Architext CLI binary for ${key}.`,
    license: rootPkg.license,
    repository: rootPkg.repository,
    // Payload: the native binary + the viewer dist shipped beside it (the serve
    // command resolves `<exe_dir>/dist`, so the installed binary is self-contained).
    files: [binaryName(key.split("-")[0]), "dist"],
    os,
    cpu,
    // No bin/main: this package is resolved by the root launcher, not run directly.
    publishConfig: { access: "public" }
  };
}

// Stamp one package dir. Returns { dir, hasBinary }.
function stampPackage(key, outDir, binarySrc) {
  if (!SUPPORTED_TARGETS[key]) {
    throw new Error(`Unsupported target key: ${key}. Known: ${Object.keys(SUPPORTED_TARGETS).join(", ")}`);
  }
  const pkgDir = join(outDir, key);
  mkdirSync(pkgDir, { recursive: true });

  const pkgJson = packageJsonFor(key);
  writeFileSync(join(pkgDir, "package.json"), `${JSON.stringify(pkgJson, null, 2)}\n`);

  writeFileSync(
    join(pkgDir, "README.md"),
    `# ${pkgJson.name}\n\n` +
      `Native Architext CLI binary for \`${key}\`. Installed automatically as an ` +
      `optionalDependency of [\`@robotaccomplice/architext\`](https://www.npmjs.com/package/@robotaccomplice/architext); ` +
      `not meant to be installed or run directly.\n`
  );

  const binName = binaryName(key.split("-")[0]);
  let hasBinary = false;
  if (binarySrc) {
    if (!existsSync(binarySrc)) {
      throw new Error(`Binary not found: ${binarySrc}`);
    }
    const dest = join(pkgDir, binName);
    copyFileSync(binarySrc, dest);
    if (!binName.endsWith(".exe")) {
      chmodSync(dest, 0o755);
    }
    hasBinary = true;

    // Ship the Trunk-built viewer dist beside the binary so `architext serve`
    // resolves `<exe_dir>/dist` with no repo-relative assets. The dist is a build
    // artifact (trunk) — it must exist before stamping.
    const distSrc = join(repoRoot, "crates", "architext-viewer", "dist");
    if (existsSync(join(distSrc, "index.html"))) {
      cpSync(distSrc, join(pkgDir, "dist"), { recursive: true });
    } else {
      console.warn(`! viewer dist not found at ${distSrc} — run \`trunk build\` first; package ${key} will lack the viewer`);
    }
  }
  return { dir: pkgDir, hasBinary, binName };
}

function discoverBinary(binariesDir, key) {
  const platform = key.split("-")[0];
  const candidate = join(binariesDir, key, binaryName(platform));
  return existsSync(candidate) ? candidate : null;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  mkdirSync(args.outDir, { recursive: true });

  const results = [];
  if (args.binary || args.target) {
    if (!args.binary || !args.target) {
      throw new Error("--binary and --target must be provided together.");
    }
    results.push({ key: args.target, ...stampPackage(args.target, args.outDir, args.binary) });
  } else {
    // Stamp the whole matrix; attach binaries found under --binaries-dir.
    for (const key of Object.keys(SUPPORTED_TARGETS)) {
      const binarySrc = args.binariesDir ? discoverBinary(args.binariesDir, key) : null;
      results.push({ key, ...stampPackage(key, args.outDir, binarySrc) });
    }
  }

  for (const r of results) {
    const status = r.hasBinary ? `binary: ${r.binName}` : "NO BINARY (package.json only)";
    console.log(`  ${r.key.padEnd(14)} → ${r.dir}  (${status})`);
    if (!r.hasBinary) {
      console.warn(`    warning: no binary staged for ${r.key}; this package would be empty if published.`);
    }
  }
  console.log(`\nStaged ${results.length} package(s) under ${args.outDir} (version ${VERSION}).`);
}

main();
