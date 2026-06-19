#!/usr/bin/env node
// `bin.architext` launcher (Architext 1.7.0 native-binary bridge).
//
// Resolves a per-platform native Rust binary published as an optionalDependency
// (`@robotaccomplice/architext-<platform>-<arch>`) and execs it with the user's
// argv, forwarding stdio and the exit code/terminating signal. When no matching
// binary is installed — an existing JS-only 1.6.x install, an unsupported
// platform, or a failed optional install — it falls back to the bundled Node
// CLI exactly as before. The fallback path keeps the transition additive: every
// 1.6.x user keeps a working CLI.

import { spawnSync } from "node:child_process";
import { resolveNativeBinary } from "./native-binary-resolver.mjs";

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
  // No native binary for this platform/install → bundled Node CLI fallback.
  const { main } = await import("../src/adapters/cli/architext-cli.mjs");
  main().catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}
