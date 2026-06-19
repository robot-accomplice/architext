// Native-binary resolution for the `architext` launcher.
//
// Architext 1.7.0 ships a per-platform native Rust binary as an
// optionalDependency (the esbuild/swc distribution pattern). This module is the
// dependency-free, side-effect-free resolver the launcher uses to find that
// binary. It is split out from the launcher so the platform-key + resolution
// logic can be unit-tested in isolation, and so the launcher itself stays a
// thin shim.
//
// Resolution is INTENTIONALLY non-fatal: when no matching optionalDependency is
// installed (an existing JS-only 1.6.x install, an unsupported platform, or a
// failed optional install), `resolveNativeBinary` returns null and the launcher
// falls back to the bundled Node CLI. This keeps the transition additive.

import { createRequire } from "node:module";

// The five supported `${platform}-${arch}` targets, mapped to their npm package
// name. Centralized here so the launcher, the package generator, and the tests
// agree on the matrix. `arch` uses Node's `process.arch` vocabulary (x64,
// arm64); `platform` uses `process.platform` (darwin, linux, win32).
export const SUPPORTED_TARGETS = Object.freeze({
  "darwin-arm64": "@robotaccomplice/architext-darwin-arm64",
  "darwin-x64": "@robotaccomplice/architext-darwin-x64",
  "linux-x64": "@robotaccomplice/architext-linux-x64",
  "linux-arm64": "@robotaccomplice/architext-linux-arm64",
  "win32-x64": "@robotaccomplice/architext-win32-x64"
});

/** Compute the `${platform}-${arch}` key for the current (or a given) process. */
export function platformKey(platform = process.platform, arch = process.arch) {
  return `${platform}-${arch}`;
}

/** The binary basename inside a platform package (`architext.exe` on win32). */
export function binaryName(platform = process.platform) {
  return platform === "win32" ? "architext.exe" : "architext";
}

/**
 * Resolve the absolute path to the native `architext` binary for this platform,
 * or null if the matching optionalDependency is not installed / not supported.
 *
 * `requireFn` is injectable so tests can simulate a binary-present install
 * without actually installing an optionalDependency: pass a `require` whose
 * `.resolve` returns a path for the expected package specifier.
 */
export function resolveNativeBinary(
  { platform = process.platform, arch = process.arch, requireFn } = {}
) {
  const key = platformKey(platform, arch);
  const pkg = SUPPORTED_TARGETS[key];
  if (!pkg) {
    return null; // Unsupported platform → JS fallback.
  }

  const req = requireFn ?? createRequire(import.meta.url);
  const specifier = `${pkg}/${binaryName(platform)}`;
  try {
    return req.resolve(specifier);
  } catch {
    return null; // optionalDependency not installed → JS fallback.
  }
}
