#!/usr/bin/env node
// `bin.architext` launcher (Architext 1.7.0 native-binary bridge).
//
// Resolves a per-platform native Rust binary published as an optionalDependency
// (`@robotaccomplice/architext-<platform>-<arch>`) and execs it with the user's
// argv, forwarding stdio and the exit code/terminating signal.
//
// 1.7.0 is RUST-ONLY: the JS engine/CLI was removed at the cutover, so there is
// no bundled-Node fallback. A missing native binary (unsupported platform, or a
// blocked/failed optional install) is a hard, actionable error rather than a
// silent downgrade.

import { spawnSync } from "node:child_process";
import { platformKey, resolveNativeBinary, SUPPORTED_TARGETS } from "./native-binary-resolver.mjs";

const nativeBinary = resolveNativeBinary();

if (nativeBinary) {
  // Forward argv (minus node + this script) and inherit stdio so the native
  // binary owns the terminal. Forward the exit code, and re-raise the
  // terminating signal so callers (CI, shells) observe it faithfully.
  const result = spawnSync(nativeBinary, process.argv.slice(2), {
    stdio: "inherit"
  });
  if (result.error) {
    console.error(result.error.message);
    process.exit(1);
  }
  if (result.signal) {
    // Re-raise so the parent sees the signal, not a synthesized exit code.
    process.kill(process.pid, result.signal);
    // If still alive (signal could not be delivered), surface a non-zero exit.
    process.exit(1);
  }
  process.exit(result.status ?? 0);
} else {
  // No native binary for this platform/install. 1.7.0 is Rust-only — no JS
  // fallback — so this is a hard error with a clear remedy.
  const key = platformKey();
  const supported = Object.keys(SUPPORTED_TARGETS).join(", ");
  const known = Object.prototype.hasOwnProperty.call(SUPPORTED_TARGETS, key);
  console.error(
    `architext: no native binary found for ${key}.\n` +
      (known
        ? `The optional dependency ${SUPPORTED_TARGETS[key]} did not install. ` +
          `Reinstall with optional dependencies enabled:\n  npm install -g @robotaccomplice/architext\n`
        : `This platform is not yet supported. Supported targets: ${supported}.\n`)
  );
  process.exit(1);
}
