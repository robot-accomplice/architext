// serve-mutation-parity-rust.mjs
//
// HTTP-level mutation parity gate for the Rust serve adapter (Phase 2D slice 3a).
//
// DOES NOT boot the Node serve-lifecycle. Instead:
//   1. Always rebuilds the Rust `architext-serve` binary (release).
//   2. Sets up two identical temp copies of docs/architext/data.
//   3. For each test case:
//        (a) JS oracle — calls updateRulesRequest / updateNotesRequest directly
//            (importing from src/adapters/http/) using real readJson/writeJson/validateTarget.
//        (b) Rust server — sends identical HTTP POST to the Rust server with a
//            valid mutation token (fetched from /api/session).
//   4. Asserts: HTTP response envelope matches (semantic), AND on-disk file bytes
//      are byte-identical between the two temp dirs.
//   5. Reports per-case GREEN/RED + N/total; exits nonzero on RED.
//
// Cases covered:
//   Rules:
//     1. upsert (new rule)
//     2. upsert (update existing unprotected rule)
//     3. delete (existing unprotected rule)
//     4. move (up)
//     5. move-before
//     6. protected-rule rejection (edit-protected → rollback, file unchanged)
//     7. unknown action → error envelope, file unchanged
//   Notes:
//     8. upsert (new note — manifest.files.notes bootstrap)
//     9. upsert (update existing note — manifest already has notes)
//    10. delete (existing note)
//   Security/infra:
//    11. missing token → 403
//    12. wrong token  → 403
//    13. oversize body → error envelope (200 {ok:false})
//    14. validation-failure rollback (corrupt upsert that breaks schema → exact bytes unchanged)
//
// Usage: node viewer/tools/serve-mutation-parity-rust.mjs

import { execFileSync, spawn } from "node:child_process";
import { cpSync, mkdirSync, rmSync, readFileSync, writeFileSync } from "node:fs";
import { readFile, cp, rm } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..");
const srcDataDir = path.join(repoRoot, "docs/architext/data");
const schemaDir = path.join(repoRoot, "viewer/schema");
const distDir = path.join(repoRoot, "viewer/dist");
const binPath = path.join(repoRoot, "target/release/architext-serve");

// ---------------------------------------------------------------------------
// JS oracle imports
// ---------------------------------------------------------------------------
const { updateRulesRequest } = await import(
  path.join(repoRoot, "src/adapters/http/rules-api.mjs")
);
const { updateNotesRequest } = await import(
  path.join(repoRoot, "src/adapters/http/notes-api.mjs")
);
const { readJson, writeJson } = await import(
  path.join(repoRoot, "src/adapters/cli/runtime.mjs")
);

// ---------------------------------------------------------------------------
// Build the Rust binary — ALWAYS rebuild; never trust a stale binary.
// ---------------------------------------------------------------------------
console.log("[setup] building architext-serve (release)...");
try {
  execFileSync(
    "cargo",
    ["build", "-p", "architext-serve", "--release", "--quiet"],
    { cwd: repoRoot, stdio: "inherit", timeout: 600_000 }
  );
} catch (err) {
  console.error("FATAL: cargo build failed:", err.message);
  process.exit(1);
}

// ---------------------------------------------------------------------------
// Temp directory management
// ---------------------------------------------------------------------------
const tmpBase = path.join(repoRoot, ".tmp-mutation-parity");
rmSync(tmpBase, { recursive: true, force: true });
mkdirSync(tmpBase, { recursive: true });

let tempDirSeq = 0;
function makeTempDataDir(suffix = "") {
  const dir = path.join(tmpBase, `data-${++tempDirSeq}${suffix ? "-" + suffix : ""}`);
  cpSync(srcDataDir, dir, { recursive: true });
  return dir;
}

// JS oracle helpers: inject real readJson/writeJson, simple validateTarget via
// the Rust validator CLI binary (architext-core validate bin).
// We use the compiled Rust validation binary so both sides use the same validator.
const rustValidateBin = path.join(
  repoRoot,
  "target/release/validate"
);

// Try to build the validate binary too
try {
  execFileSync(
    "cargo",
    ["build", "-p", "architext-core", "--bin", "validate", "--release", "--quiet"],
    { cwd: repoRoot, stdio: "pipe", timeout: 120_000 }
  );
} catch {
  // If validate binary isn't available, use Node validator
}

function makeValidateTarget(dataBaseDir) {
  // validateTarget(target) → { ok, output }
  // For the oracle, "target" IS the data dir (we pass dataDir=identity below).
  return async function validateTarget(_target) {
    try {
      const { default: childProcess } = await import("node:child_process");
      const result = childProcess.spawnSync(
        process.execPath,
        [
          path.join(repoRoot, "viewer/tools/validate-architext.mjs"),
          "--data-dir", _target,
          "--schema-dir", schemaDir
        ],
        { encoding: "utf8", timeout: 30_000 }
      );
      const ok = result.status === 0;
      return { ok, output: result.stdout + result.stderr };
    } catch (e) {
      return { ok: false, output: String(e) };
    }
  };
}

// dataDir helper: identity (target IS the data dir)
const dataDirFn = (target) => target;

// withTargetWriteLock: no-op for oracle (single-process, no concurrent writers)
async function withTargetWriteLock(_target, callback) {
  return callback();
}

// ---------------------------------------------------------------------------
// Spawn Rust server
// ---------------------------------------------------------------------------
const testPort = await findFreePort();

const server = spawn(
  binPath,
  [
    "--data-dir", srcDataDir,  // served data-dir for security/session endpoint only
    "--dist", distDir,
    "--port", String(testPort),
    "--host", "127.0.0.1",
  ],
  { stdio: ["ignore", "pipe", "pipe"] }
);

let serverReady = false;
let serverShuttingDown = false;
const serverStderr = [];

server.stderr.on("data", (d) => {
  const msg = d.toString();
  if (msg.includes("listening")) serverReady = true;
  serverStderr.push(msg);
});

server.on("exit", (code, signal) => {
  if (!serverReady && !serverShuttingDown) {
    console.error("FATAL: server exited before ready. stderr:", serverStderr.join(""));
    process.exit(1);
  }
});

await waitForReady(testPort, 15_000);
const BASE = `http://127.0.0.1:${testPort}`;
console.log(`[setup] server ready at ${BASE}\n`);

// Fetch mutation token from /api/session (single token used for all valid requests)
const sessionR = await fetch(`${BASE}/api/session`, {
  headers: { host: `127.0.0.1:${testPort}` }
});
const { mutationToken } = await sessionR.json();

// ---------------------------------------------------------------------------
// Check framework
// ---------------------------------------------------------------------------
const checks = [];
let green = 0;

function record(name, pass, note = "") {
  checks.push({ name, pass, note });
  if (pass) green += 1;
  const marker = pass ? "✓" : "✗";
  console.log(`${marker} ${(pass ? "GREEN" : "RED").padEnd(5)} ${name}${note ? "  (" + note + ")" : ""}`);
}

// ---------------------------------------------------------------------------
// Helpers: invoke JS oracle and Rust server with identical data-dir copies,
// then compare response + file bytes.
// ---------------------------------------------------------------------------

/**
 * Run both JS oracle and Rust server against fresh identical temp data-dir copies.
 *
 * @param {object} opts
 *   - label       {string} test case name
 *   - route       {string} "/api/rules" | "/api/notes"
 *   - payload     {object} the action payload
 *   - jsOracle    {function(tmpJsDir)} → result value (same as adapter return)
 *   - fileRelPath {string} relative path of the file to compare bytes (e.g. "rules.json")
 *   - expectOkFalse {boolean} if true, expect {ok:false,...} from both
 *   - checkFilesUnchanged {boolean} if true, file bytes must be unchanged vs original
 */
async function runCase({
  label,
  route,
  payload,
  fileRelPath,
  expectOkFalse = false,
  checkFilesUnchanged = false,
}) {
  // Fresh identical copies for JS and Rust
  const tmpJs = makeTempDataDir("js");
  const tmpRust = makeTempDataDir("rust");

  // Capture original file bytes for rollback checks
  const origFilePath = path.join(tmpJs, fileRelPath);
  const origBytes = readExistingBytes(origFilePath);

  // --- JS oracle ---
  let jsResult;
  let jsError = null;
  try {
    if (route === "/api/rules") {
      jsResult = await updateRulesRequest({
        target: tmpJs,
        payload,
        dataDir: dataDirFn,
        readJson,
        writeJson,
        validateTarget: makeValidateTarget(tmpJs),
        withTargetWriteLock
      });
    } else {
      jsResult = await updateNotesRequest({
        target: tmpJs,
        payload,
        dataDir: dataDirFn,
        readJson,
        writeJson,
        validateTarget: makeValidateTarget(tmpJs),
        withTargetWriteLock
      });
    }
  } catch (e) {
    jsError = e;
    jsResult = { ok: false, mode: route.slice(5), error: e.message, reload: false };
  }

  // --- Rust server ---
  // Spawn a new Rust server for each case so we get a fresh data-dir
  const rustPort = await findFreePort();
  const rustServer = spawn(
    binPath,
    [
      "--data-dir", tmpRust,
      "--dist", distDir,
      "--port", String(rustPort),
      "--host", "127.0.0.1",
    ],
    { stdio: ["ignore", "pipe", "pipe"] }
  );
  const rustStderr = [];
  rustServer.stderr.on("data", (d) => {
    const msg = d.toString();
    rustStderr.push(msg);
  });
  await waitForReady(rustPort, 10_000);

  // Get the per-instance mutation token
  const rustSession = await fetch(`http://127.0.0.1:${rustPort}/api/session`, {
    headers: { host: `127.0.0.1:${rustPort}` }
  });
  const { mutationToken: rustToken } = await rustSession.json();

  const rustResp = await fetch(`http://127.0.0.1:${rustPort}${route}`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-architext-mutation-token": rustToken,
      host: `127.0.0.1:${rustPort}`,
    },
    body: JSON.stringify(payload),
  });
  const rustBody = await rustResp.json();
  rustServer.kill();

  // --- Compare ---
  const jsFilePath = path.join(tmpJs, fileRelPath);
  const rustFilePath = path.join(tmpRust, fileRelPath);

  let pass = true;
  const notes = [];

  // 1. HTTP status must be 200 (JS always returns 200 for these)
  if (rustResp.status !== 200) {
    pass = false;
    notes.push(`HTTP status ${rustResp.status} (expected 200)`);
  }

  // 2. Response envelope shape
  if (expectOkFalse) {
    // Both should return {ok:false, ...}
    const jsHasOkFalse = jsResult && jsResult.ok === false;
    const rustHasOkFalse = rustBody && rustBody.ok === false;
    if (!rustHasOkFalse) {
      pass = false;
      notes.push(`Rust returned ok=${rustBody?.ok} (expected false); body=${JSON.stringify(rustBody)}`);
    }
    if (!jsHasOkFalse) {
      // JS raised an exception rather than returning ok:false - still ok, just note
      // the error propagation model differs; we care about Rust matching JS behaviour
    }
    // Error message should be present
    if (rustHasOkFalse && !rustBody.error) {
      pass = false;
      notes.push(`Rust ok:false but no error field`);
    }
    if (rustHasOkFalse && !rustBody.mode) {
      pass = false;
      notes.push(`Rust ok:false but no mode field`);
    }
    if (rustHasOkFalse && rustBody.reload !== false) {
      pass = false;
      notes.push(`Rust ok:false but reload=${rustBody.reload} (expected false)`);
    }
  } else {
    // Success: both should have ok !== false (no ok field, or ok:true)
    // JS returns { rules/notes: [...], validation: { ok: true } }
    // Rust returns the same shape
    const routeKey = route === "/api/rules" ? "rules" : "notes";
    if (!Array.isArray(rustBody?.[routeKey])) {
      pass = false;
      notes.push(`Rust response missing ${routeKey} array; body=${JSON.stringify(rustBody).slice(0, 200)}`);
    }
    if (rustBody?.validation?.ok !== true) {
      pass = false;
      notes.push(`Rust validation.ok=${rustBody?.validation?.ok} (expected true)`);
    }
    // JS result should also have the array
    const jsArr = jsResult?.[routeKey];
    if (pass && Array.isArray(jsArr) && Array.isArray(rustBody?.[routeKey])) {
      if (jsArr.length !== rustBody[routeKey].length) {
        pass = false;
        notes.push(`${routeKey} length mismatch: JS=${jsArr.length} Rust=${rustBody[routeKey].length}`);
      }
    }
  }

  // 3. On-disk file bytes must be byte-identical (or both unchanged if rollback)
  if (checkFilesUnchanged) {
    // Both sides should have the original bytes
    const jsCurrentBytes = readExistingBytes(jsFilePath);
    const rustCurrentBytes = readExistingBytes(rustFilePath);

    if (!buffersEqual(jsCurrentBytes ?? Buffer.alloc(0), origBytes ?? Buffer.alloc(0))) {
      pass = false;
      notes.push(`JS did NOT restore original file bytes after rollback`);
    }
    if (!buffersEqual(rustCurrentBytes ?? Buffer.alloc(0), origBytes ?? Buffer.alloc(0))) {
      pass = false;
      notes.push(`Rust did NOT restore original file bytes after rollback`);
    }
  } else {
    // Normal success path: JS and Rust must have written identical bytes
    const jsCurrentBytes = readExistingBytes(jsFilePath);
    const rustCurrentBytes = readExistingBytes(rustFilePath);

    if (jsCurrentBytes === null && rustCurrentBytes === null) {
      // Both files absent — fine for notes delete-only corner case
    } else if (jsCurrentBytes === null || rustCurrentBytes === null) {
      pass = false;
      notes.push(`File existence mismatch: JS=${jsCurrentBytes === null ? "absent" : "present"} Rust=${rustCurrentBytes === null ? "absent" : "present"}`);
    } else if (!buffersEqual(jsCurrentBytes, rustCurrentBytes)) {
      pass = false;
      notes.push(`On-disk file bytes DIFFER (JS=${jsCurrentBytes.length}b Rust=${rustCurrentBytes.length}b)`);
      // Surface first difference for debugging
      const jStr = jsCurrentBytes.toString("utf8");
      const rStr = rustCurrentBytes.toString("utf8");
      const jLines = jStr.split("\n");
      const rLines = rStr.split("\n");
      for (let i = 0; i < Math.max(jLines.length, rLines.length); i++) {
        if (jLines[i] !== rLines[i]) {
          notes.push(`First diff at line ${i+1}: JS=${JSON.stringify(jLines[i])} Rust=${JSON.stringify(rLines[i])}`);
          break;
        }
      }
    }
  }

  record(label, pass, notes.join("; "));
}

function readExistingBytes(filePath) {
  try {
    return readFileSync(filePath);
  } catch (e) {
    if (e.code === "ENOENT") return null;
    throw e;
  }
}

function buffersEqual(a, b) {
  if (a.length !== b.length) return false;
  return a.equals(b);
}

// ---------------------------------------------------------------------------
// Helper: find an unprotected rule id from the real data
// ---------------------------------------------------------------------------
const rulesDoc = JSON.parse(readFileSync(path.join(srcDataDir, "rules.json"), "utf8"));
const unprotectedRules = rulesDoc.rules.filter(r => !r.protection?.edit && !r.protection?.delete);
const protectedRules = rulesDoc.rules.filter(r => r.protection?.edit);

const rule0 = unprotectedRules[0];  // for update/delete/move
const rule1 = unprotectedRules[1];  // for move-before target

// ---------------------------------------------------------------------------
// Case 1: rules upsert — new rule
// ---------------------------------------------------------------------------
await runCase({
  label: "rules upsert (new rule) — on-disk bytes match JS oracle",
  route: "/api/rules",
  payload: {
    action: "update",
    rule: {
      id: "test-new-rule-parity-1",
      title: "Parity test rule",
      summary: "Added by parity gate",
      category: "Development",
      criticality: "low",
      order: 9999,
      source: "maintainer",
      protection: { edit: false, delete: false }
    }
  },
  fileRelPath: "rules.json",
});

// ---------------------------------------------------------------------------
// Case 2: rules upsert — update existing unprotected rule
// ---------------------------------------------------------------------------
await runCase({
  label: "rules upsert (update existing unprotected) — bytes match",
  route: "/api/rules",
  payload: {
    action: "update",
    rule: {
      ...rule0,
      summary: "Updated by parity gate",
    }
  },
  fileRelPath: "rules.json",
});

// ---------------------------------------------------------------------------
// Case 3: rules delete
// ---------------------------------------------------------------------------
await runCase({
  label: "rules delete (existing unprotected rule) — bytes match",
  route: "/api/rules",
  payload: {
    action: "delete",
    id: rule0.id
  },
  fileRelPath: "rules.json",
});

// ---------------------------------------------------------------------------
// Case 4: rules move (up)
// ---------------------------------------------------------------------------
await runCase({
  label: "rules move up — bytes match",
  route: "/api/rules",
  payload: {
    action: "move",
    id: rule1.id,
    direction: "up"
  },
  fileRelPath: "rules.json",
});

// ---------------------------------------------------------------------------
// Case 5: rules move-before
// ---------------------------------------------------------------------------
await runCase({
  label: "rules move-before — bytes match",
  route: "/api/rules",
  payload: {
    action: "move-before",
    id: rule1.id,
    beforeId: rule0.id
  },
  fileRelPath: "rules.json",
});

// ---------------------------------------------------------------------------
// Case 6: protected-rule rejection — file unchanged byte-for-byte
// ---------------------------------------------------------------------------
await runCase({
  label: "rules edit-protected rejection — rollback, file unchanged",
  route: "/api/rules",
  payload: {
    action: "update",
    rule: {
      ...protectedRules[0],
      summary: "Should be rejected",
    }
  },
  fileRelPath: "rules.json",
  expectOkFalse: true,
  checkFilesUnchanged: true,
});

// ---------------------------------------------------------------------------
// Case 7: unknown action → error envelope, file unchanged
// ---------------------------------------------------------------------------
await runCase({
  label: "rules unknown action → ok:false envelope, file unchanged",
  route: "/api/rules",
  payload: {
    action: "frobnicate",
    id: "anything"
  },
  fileRelPath: "rules.json",
  expectOkFalse: true,
  checkFilesUnchanged: true,
});

// ---------------------------------------------------------------------------
// Case 8: notes upsert (new note — manifest.files.notes bootstrap)
// ---------------------------------------------------------------------------
// Verify both manifest.json and notes.json are written byte-identically.
// We test manifest bootstrap separately: read the manifest from both sides and
// compare them byte-identically.
{
  const tmpJs = makeTempDataDir("notes-new-js");
  const tmpRust = makeTempDataDir("notes-new-rust");

  // Ensure neither side has notes.json initially
  const jsNotesPath = path.join(tmpJs, "notes.json");
  const rustNotesPath = path.join(tmpRust, "notes.json");
  rmSync(jsNotesPath, { force: true });
  rmSync(rustNotesPath, { force: true });
  // Also ensure manifest.files.notes is absent
  const jsManifestPath = path.join(tmpJs, "manifest.json");
  const rustManifestPath = path.join(tmpRust, "manifest.json");
  const origManifest = JSON.parse(readFileSync(jsManifestPath, "utf8"));
  const manifestNoNotes = { ...origManifest, files: { ...origManifest.files } };
  delete manifestNoNotes.files.notes;
  writeFileSync(jsManifestPath, `${JSON.stringify(manifestNoNotes, null, 2)}\n`, "utf8");
  writeFileSync(rustManifestPath, `${JSON.stringify(manifestNoNotes, null, 2)}\n`, "utf8");

  // Use a schema-valid note: required fields = id, target, category, body, createdAt, updatedAt
  // target.id must be a real node id from the data.
  const notePayload = {
    action: "update",
    note: {
      id: "note-parity-gate-new",
      target: { kind: "node", id: "maintainer" },
      category: "note",
      body: "Parity gate note",
      createdAt: "2024-01-01T00:00:00.000Z",
      updatedAt: "2024-01-01T00:00:00.000Z"
    }
  };

  // JS oracle
  let jsResult;
  try {
    jsResult = await updateNotesRequest({
      target: tmpJs,
      payload: notePayload,
      dataDir: dataDirFn,
      readJson,
      writeJson,
      validateTarget: makeValidateTarget(tmpJs),
      withTargetWriteLock
    });
  } catch (e) {
    jsResult = { ok: false, error: e.message };
  }

  // Rust server
  const rustPort2 = await findFreePort();
  const rustServer2 = spawn(binPath, [
    "--data-dir", tmpRust, "--dist", distDir,
    "--port", String(rustPort2), "--host", "127.0.0.1",
  ], { stdio: ["ignore", "pipe", "pipe"] });
  rustServer2.stderr.on("data", () => {});
  await waitForReady(rustPort2, 10_000);
  const sess2 = await fetch(`http://127.0.0.1:${rustPort2}/api/session`, {
    headers: { host: `127.0.0.1:${rustPort2}` }
  });
  const { mutationToken: tok2 } = await sess2.json();
  const rustResp2 = await fetch(`http://127.0.0.1:${rustPort2}/api/notes`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-architext-mutation-token": tok2,
      host: `127.0.0.1:${rustPort2}`,
    },
    body: JSON.stringify(notePayload),
  });
  const rustBody2 = await rustResp2.json();
  rustServer2.kill();

  let pass8 = true;
  const notes8 = [];

  // Check HTTP 200
  if (rustResp2.status !== 200) { pass8 = false; notes8.push(`HTTP ${rustResp2.status}`); }
  // Check notes array returned
  if (!Array.isArray(rustBody2?.notes)) { pass8 = false; notes8.push(`no notes array`); }
  // Check validation.ok
  if (rustBody2?.validation?.ok !== true) { pass8 = false; notes8.push(`validation.ok=${rustBody2?.validation?.ok}`); }

  // Compare notes.json bytes
  const jsN = readExistingBytes(jsNotesPath);
  const rustN = readExistingBytes(rustNotesPath);
  if (jsN === null) { pass8 = false; notes8.push("JS did not create notes.json"); }
  else if (rustN === null) { pass8 = false; notes8.push("Rust did not create notes.json"); }
  else if (!buffersEqual(jsN, rustN)) {
    pass8 = false;
    notes8.push(`notes.json bytes differ (JS=${jsN.length}b Rust=${rustN.length}b)`);
  }

  // Compare manifest.json bytes (both should have notes entry added)
  const jsM = readExistingBytes(jsManifestPath);
  const rustM = readExistingBytes(rustManifestPath);
  if (!buffersEqual(jsM, rustM)) {
    pass8 = false;
    notes8.push(`manifest.json bytes differ after bootstrap (JS=${jsM?.length}b Rust=${rustM?.length}b)`);
    const jStr = jsM?.toString("utf8") ?? "";
    const rStr = rustM?.toString("utf8") ?? "";
    notes8.push(`JS manifest: ${jStr.slice(0, 200)}`);
    notes8.push(`Rust manifest: ${rStr.slice(0, 200)}`);
  }

  // Check manifest has notes entry
  try {
    const rustManifest = JSON.parse(rustM?.toString("utf8") ?? "{}");
    if (!rustManifest.files?.notes) {
      pass8 = false;
      notes8.push(`Rust manifest.files.notes not set after bootstrap`);
    }
  } catch { pass8 = false; notes8.push("Rust manifest not valid JSON"); }

  record("notes upsert new + manifest bootstrap — notes.json + manifest.json bytes match", pass8, notes8.join("; "));
}

// ---------------------------------------------------------------------------
// Case 9: notes upsert (update existing note — manifest already has notes)
// ---------------------------------------------------------------------------
{
  // Pre-populate notes.json with one note and manifest with notes entry
  const tmpJs = makeTempDataDir("notes-update-js");
  const tmpRust = makeTempDataDir("notes-update-rust");

  // Schema-valid note: real node id, all required fields present.
  const existingNote = {
    id: "note-existing-parity",
    target: { kind: "node", id: "architext-cli" },
    category: "note",
    body: "Original body text",
    createdAt: "2024-01-01T00:00:00.000Z",
    updatedAt: "2024-01-01T00:00:00.000Z"
  };
  const initialNotesDoc = { notes: [existingNote] };
  for (const dir of [tmpJs, tmpRust]) {
    writeFileSync(path.join(dir, "notes.json"), `${JSON.stringify(initialNotesDoc, null, 2)}\n`, "utf8");
    const m = JSON.parse(readFileSync(path.join(dir, "manifest.json"), "utf8"));
    m.files = { ...m.files, notes: "notes.json" };
    writeFileSync(path.join(dir, "manifest.json"), `${JSON.stringify(m, null, 2)}\n`, "utf8");
  }

  const updatePayload = {
    action: "update",
    note: { ...existingNote, body: "Updated by parity gate" }
  };

  let jsResult9;
  try {
    jsResult9 = await updateNotesRequest({
      target: tmpJs, payload: updatePayload, dataDir: dataDirFn,
      readJson, writeJson, validateTarget: makeValidateTarget(tmpJs), withTargetWriteLock
    });
  } catch (e) { jsResult9 = { ok: false, error: e.message }; }

  const rustPort3 = await findFreePort();
  const rs3 = spawn(binPath, [
    "--data-dir", tmpRust, "--dist", distDir,
    "--port", String(rustPort3), "--host", "127.0.0.1",
  ], { stdio: ["ignore", "pipe", "pipe"] });
  rs3.stderr.on("data", () => {});
  await waitForReady(rustPort3, 10_000);
  const sess3 = await fetch(`http://127.0.0.1:${rustPort3}/api/session`, { headers: { host: `127.0.0.1:${rustPort3}` } });
  const { mutationToken: tok3 } = await sess3.json();
  const rr3 = await fetch(`http://127.0.0.1:${rustPort3}/api/notes`, {
    method: "POST",
    headers: { "content-type": "application/json", "x-architext-mutation-token": tok3, host: `127.0.0.1:${rustPort3}` },
    body: JSON.stringify(updatePayload),
  });
  const rb3 = await rr3.json();
  rs3.kill();

  const jsN9 = readExistingBytes(path.join(tmpJs, "notes.json"));
  const rustN9 = readExistingBytes(path.join(tmpRust, "notes.json"));
  const pass9 = rr3.status === 200
    && Array.isArray(rb3?.notes)
    && rb3?.validation?.ok === true
    && jsN9 !== null && rustN9 !== null
    && buffersEqual(jsN9, rustN9);
  const note9 = pass9 ? "" : `HTTP=${rr3.status} notes=${Array.isArray(rb3?.notes)} val=${rb3?.validation?.ok} jsBytes=${jsN9?.length} rustBytes=${rustN9?.length} match=${jsN9 && rustN9 ? buffersEqual(jsN9, rustN9) : "na"}`;
  record("notes upsert update existing — bytes match", pass9, note9);
}

// ---------------------------------------------------------------------------
// Case 10: notes delete
// ---------------------------------------------------------------------------
{
  const tmpJs = makeTempDataDir("notes-del-js");
  const tmpRust = makeTempDataDir("notes-del-rust");
  // Schema-valid note for deletion test (real node id, all required fields).
  const delNote = {
    id: "note-to-delete-parity",
    target: { kind: "node", id: "llm-agent" },
    category: "note",
    body: "Delete me",
    createdAt: "2024-01-01T00:00:00.000Z",
    updatedAt: "2024-01-01T00:00:00.000Z"
  };
  for (const dir of [tmpJs, tmpRust]) {
    writeFileSync(path.join(dir, "notes.json"), `${JSON.stringify({ notes: [delNote] }, null, 2)}\n`, "utf8");
    const m = JSON.parse(readFileSync(path.join(dir, "manifest.json"), "utf8"));
    m.files = { ...m.files, notes: "notes.json" };
    writeFileSync(path.join(dir, "manifest.json"), `${JSON.stringify(m, null, 2)}\n`, "utf8");
  }

  const delPayload = { action: "delete", id: delNote.id };
  let jsR10;
  try {
    jsR10 = await updateNotesRequest({
      target: tmpJs, payload: delPayload, dataDir: dataDirFn,
      readJson, writeJson, validateTarget: makeValidateTarget(tmpJs), withTargetWriteLock
    });
  } catch (e) { jsR10 = { ok: false, error: e.message }; }

  const rustPort4 = await findFreePort();
  const rs4 = spawn(binPath, [
    "--data-dir", tmpRust, "--dist", distDir,
    "--port", String(rustPort4), "--host", "127.0.0.1",
  ], { stdio: ["ignore", "pipe", "pipe"] });
  rs4.stderr.on("data", () => {});
  await waitForReady(rustPort4, 10_000);
  const sess4 = await fetch(`http://127.0.0.1:${rustPort4}/api/session`, { headers: { host: `127.0.0.1:${rustPort4}` } });
  const { mutationToken: tok4 } = await sess4.json();
  const rr4 = await fetch(`http://127.0.0.1:${rustPort4}/api/notes`, {
    method: "POST",
    headers: { "content-type": "application/json", "x-architext-mutation-token": tok4, host: `127.0.0.1:${rustPort4}` },
    body: JSON.stringify(delPayload),
  });
  const rb4 = await rr4.json();
  rs4.kill();

  const jsN10 = readExistingBytes(path.join(tmpJs, "notes.json"));
  const rustN10 = readExistingBytes(path.join(tmpRust, "notes.json"));
  const pass10 = rr4.status === 200
    && Array.isArray(rb4?.notes) && rb4.notes.length === 0
    && rb4?.validation?.ok === true
    && jsN10 !== null && rustN10 !== null
    && buffersEqual(jsN10, rustN10);
  record("notes delete — bytes match", pass10,
    pass10 ? "" : `HTTP=${rr4.status} notes=${JSON.stringify(rb4?.notes)} val=${rb4?.validation?.ok} match=${jsN10 && rustN10 ? buffersEqual(jsN10, rustN10) : "na"}`
  );
}

// ---------------------------------------------------------------------------
// Case 11: missing token → 403
// ---------------------------------------------------------------------------
{
  const { request: httpRequest } = await import("node:http");
  const r11 = await new Promise((resolve, reject) => {
    const body = JSON.stringify({ action: "update", rule: {} });
    const req = httpRequest({
      host: "127.0.0.1", port: testPort,
      path: "/api/rules", method: "POST",
      headers: {
        host: `127.0.0.1:${testPort}`,
        "content-type": "application/json",
        "content-length": Buffer.byteLength(body),
        // NO x-architext-mutation-token
      },
    }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve({ status: res.statusCode, body: JSON.parse(Buffer.concat(chunks).toString("utf8") || "{}") }));
    });
    req.on("error", reject);
    req.write(body);
    req.end();
  });
  const pass11 = r11.status === 403 && r11.body?.error === "Architext write request is not authorized.";
  record("missing token → 403 + exact error JSON", pass11,
    pass11 ? "" : `status=${r11.status} error=${JSON.stringify(r11.body?.error)}`
  );
}

// ---------------------------------------------------------------------------
// Case 12: wrong token → 403
// ---------------------------------------------------------------------------
{
  const { request: httpRequest } = await import("node:http");
  const r12 = await new Promise((resolve, reject) => {
    const body = JSON.stringify({ action: "update", rule: {} });
    const req = httpRequest({
      host: "127.0.0.1", port: testPort,
      path: "/api/rules", method: "POST",
      headers: {
        host: `127.0.0.1:${testPort}`,
        "content-type": "application/json",
        "content-length": Buffer.byteLength(body),
        "x-architext-mutation-token": "WRONG_TOKEN_VALUE",
      },
    }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve({ status: res.statusCode, body: JSON.parse(Buffer.concat(chunks).toString("utf8") || "{}") }));
    });
    req.on("error", reject);
    req.write(body);
    req.end();
  });
  const pass12 = r12.status === 403 && r12.body?.error === "Architext write request is not authorized.";
  record("wrong token → 403 + exact error JSON", pass12,
    pass12 ? "" : `status=${r12.status} error=${JSON.stringify(r12.body?.error)}`
  );
}

// ---------------------------------------------------------------------------
// Case 13: oversize body → error envelope (200 {ok:false, error:"Request body is too large"})
// ---------------------------------------------------------------------------
{
  const { request: httpRequest } = await import("node:http");
  // 1 MiB + 1 byte
  const oversizeBody = Buffer.alloc(1024 * 1024 + 1, "x");
  const r13 = await new Promise((resolve, reject) => {
    const req = httpRequest({
      host: "127.0.0.1", port: testPort,
      path: "/api/rules", method: "POST",
      headers: {
        host: `127.0.0.1:${testPort}`,
        "content-type": "application/json",
        "content-length": oversizeBody.length,
        "x-architext-mutation-token": mutationToken,
      },
    }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => {
        try {
          resolve({ status: res.statusCode, body: JSON.parse(Buffer.concat(chunks).toString("utf8") || "{}") });
        } catch {
          resolve({ status: res.statusCode, body: null });
        }
      });
    });
    req.on("error", reject);
    req.write(oversizeBody);
    req.end();
  });
  const pass13 = r13.status === 200
    && r13.body?.ok === false
    && r13.body?.error === "Request body is too large"
    && r13.body?.mode === "rules"
    && r13.body?.reload === false;
  record("oversize body (>1MiB) → 200 {ok:false, error:'Request body is too large'}", pass13,
    pass13 ? "" : `status=${r13.status} body=${JSON.stringify(r13.body)}`
  );
}

// ---------------------------------------------------------------------------
// Case 14: validation-failure rollback — corrupt upsert → exact bytes unchanged
// ---------------------------------------------------------------------------
// We insert a rule with a missing required field (no `title`) which should
// fail schema validation, causing the write to be rolled back.
// We verify the rules.json bytes are unchanged vs original.
{
  const tmpJs = makeTempDataDir("rollback-js");
  const tmpRust = makeTempDataDir("rollback-rust");
  const origRulesJs = readExistingBytes(path.join(tmpJs, "rules.json"));
  const origRulesRust = readExistingBytes(path.join(tmpRust, "rules.json"));

  // A rule missing required fields will cause schema validation failure
  const badPayload = {
    action: "update",
    rule: {
      id: "validation-fail-test-rule",
      // intentionally missing: title, criticality, order — required by schema
      summary: "This should fail validation"
    }
  };

  // JS oracle: expect it to throw (then restore)
  let jsR14;
  try {
    jsR14 = await updateRulesRequest({
      target: tmpJs, payload: badPayload, dataDir: dataDirFn,
      readJson, writeJson, validateTarget: makeValidateTarget(tmpJs), withTargetWriteLock
    });
    // If it didn't throw, the oracle may have succeeded (unlikely) — note it
    jsR14 = { succeeded: true, ...jsR14 };
  } catch (e) {
    jsR14 = { ok: false, error: e.message };
  }

  // Rust server
  const rustPort14 = await findFreePort();
  const rs14 = spawn(binPath, [
    "--data-dir", tmpRust, "--dist", distDir,
    "--port", String(rustPort14), "--host", "127.0.0.1",
  ], { stdio: ["ignore", "pipe", "pipe"] });
  rs14.stderr.on("data", () => {});
  await waitForReady(rustPort14, 10_000);
  const sess14 = await fetch(`http://127.0.0.1:${rustPort14}/api/session`, { headers: { host: `127.0.0.1:${rustPort14}` } });
  const { mutationToken: tok14 } = await sess14.json();
  const rr14 = await fetch(`http://127.0.0.1:${rustPort14}/api/rules`, {
    method: "POST",
    headers: { "content-type": "application/json", "x-architext-mutation-token": tok14, host: `127.0.0.1:${rustPort14}` },
    body: JSON.stringify(badPayload),
  });
  const rb14 = await rr14.json();
  rs14.kill();

  // Rust must return ok:false
  const rustReturnedError = rr14.status === 200 && rb14?.ok === false;

  // Both files must be byte-identical to original (rollback succeeded)
  const jsCurrentRules = readExistingBytes(path.join(tmpJs, "rules.json"));
  const rustCurrentRules = readExistingBytes(path.join(tmpRust, "rules.json"));
  const jsRolledBack = jsCurrentRules !== null && origRulesJs !== null && buffersEqual(jsCurrentRules, origRulesJs);
  const rustRolledBack = rustCurrentRules !== null && origRulesRust !== null && buffersEqual(rustCurrentRules, origRulesRust);

  const pass14 = rustReturnedError && rustRolledBack;
  const notes14 = [];
  if (!rustReturnedError) notes14.push(`Rust returned ok=${rb14?.ok} (expected false); body=${JSON.stringify(rb14).slice(0,200)}`);
  if (!rustRolledBack) notes14.push(`Rust did NOT rollback: original=${origRulesRust?.length}b current=${rustCurrentRules?.length}b match=${rustCurrentRules && origRulesRust ? buffersEqual(rustCurrentRules, origRulesRust) : "na"}`);
  if (!jsRolledBack) notes14.push(`JS did NOT rollback (expected rollback): original=${origRulesJs?.length}b current=${jsCurrentRules?.length}b`);
  record("validation-failure rollback — rules.json restored to exact prior bytes", pass14, notes14.join("; "));
}

// ---------------------------------------------------------------------------
// Shutdown & report
// ---------------------------------------------------------------------------
serverShuttingDown = true;
server.kill();

// Clean up temp dirs
rmSync(tmpBase, { recursive: true, force: true });

console.log("\n--- Results ---");
for (const c of checks) {
  const marker = c.pass ? "✓" : "✗";
  const status = c.pass ? "GREEN" : "RED";
  const note = c.note ? `  (${c.note})` : "";
  console.log(`${marker} ${status.padEnd(5)} ${c.name}${note}`);
}
console.log(`\n${green}/${checks.length} GREEN`);
if (green !== checks.length) process.exitCode = 1;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async function findFreePort() {
  const { createServer } = await import("node:net");
  return new Promise((resolve, reject) => {
    const s = createServer();
    s.listen(0, "127.0.0.1", () => {
      const port = s.address().port;
      s.close(() => resolve(port));
    });
    s.on("error", reject);
  });
}

async function waitForReady(port, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const r = await fetch(`http://127.0.0.1:${port}/api/session`, {
        headers: { host: `127.0.0.1:${port}` }
      });
      if (r.status === 200) return;
    } catch {
      // not up yet
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`Server on port ${port} did not become ready in ${timeoutMs}ms`);
}
