#!/usr/bin/env node
/**
 * domain-parity-rust.mjs — differential gate for domain module JS ↔ Rust parity.
 *
 * For each fixture under crates/architext-core/tests/domain-fixtures/<op>/*.json:
 *   1. Run the corresponding JS function, capturing result or thrown error message.
 *   2. Run `cargo run -q -p architext-core --bin domain_dump -- <op> <fixture>`.
 *   3. Compare: both error with byte-identical message, OR both succeed with
 *      semantically-equal output (object-key-order-insensitive, array-order-sensitive).
 *
 * Exits nonzero on any RED.
 */

import { createRequire } from "module";
import { fileURLToPath } from "url";
import path from "path";
import fs from "fs";
import { execFileSync } from "child_process";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "../..");
const fixtureBase = path.join(repoRoot, "crates/architext-core/tests/domain-fixtures");

// ─── Import JS domain modules ────────────────────────────────────────────────
const { orderedRules, upsertRule, deleteRule, moveRule, moveRuleBefore } =
  await import(`${repoRoot}/src/domain/architecture-model/rules.mjs`);
const { upsertNote, deleteNote, notesForTarget } =
  await import(`${repoRoot}/src/domain/architecture-model/notes.mjs`);
const { schemaMigrationPlan } =
  await import(`${repoRoot}/src/domain/lifecycle/schema-migrations.mjs`);
const { c4IssuesForView, c4DrilldownIssues, repairC4Views } =
  await import(`${repoRoot}/src/domain/architecture-model/c4-quality.mjs`);
const {
  nextMinorVersion, buildReleasePlan, mergeExistingReleasePlan, releasePlanChanges,
  saveReleasePlanDraft, approveReleasePlan,
} = await import(`${repoRoot}/src/domain/architecture-model/release-planning.mjs`);
const {
  generatedReleaseIndex, releaseIndexGenerationChanges,
  releaseSummaryFromDetail, deriveReleaseCounts,
} = await import(`${repoRoot}/src/domain/architecture-model/release-history.mjs`);
const { releaseItems } =
  await import(`${repoRoot}/src/domain/architecture-model/release-scopes.mjs`);
const { plannedInstructionRuleMigration, upsertRulePointer } =
  await import(`${repoRoot}/src/domain/lifecycle/instruction-rule-migration.mjs`);
const {
  normalizeSyncInstructionFiles, defaultSyncChoices, rememberedSyncChoices,
  applyExplicitSyncOptions, syncOperation, syncWritePlan, shouldValidateSync,
  persistedSyncChoices, syncMetadataPatch,
} = await import(`${repoRoot}/src/adapters/cli/sync-plan.mjs`);

// ─── JS dispatcher ───────────────────────────────────────────────────────────
function runJs(op, fixture) {
  switch (op) {
    case "rules.ordered":
      return orderedRules(fixture.rules);
    case "rules.upsert":
      return upsertRule(fixture.document, fixture.rule);
    case "rules.delete":
      return deleteRule(fixture.document, fixture.id);
    case "rules.move":
      return moveRule(fixture.document, fixture.id, fixture.direction);
    case "rules.moveBefore":
      return moveRuleBefore(fixture.document, fixture.id, fixture.beforeId);
    case "notes.upsert":
      return upsertNote(fixture.document, fixture.note);
    case "notes.delete":
      return deleteNote(fixture.document, fixture.id);
    case "notes.forTarget":
      return notesForTarget(fixture.notes, fixture.kind, fixture.id);
    case "schema.migrationPlan":
      return schemaMigrationPlan({ currentVersion: fixture.currentVersion, targetVersion: fixture.targetVersion });
    case "c4.issuesForView": {
      const nodeMap = new Map((fixture.nodes ?? []).map((n) => [n.id, n]));
      return c4IssuesForView(fixture.view, nodeMap);
    }
    case "c4.drilldownIssues": {
      const nodeMap = new Map((fixture.nodes ?? []).map((n) => [n.id, n]));
      return c4DrilldownIssues(fixture.views, nodeMap);
    }
    case "c4.repairViews": {
      const nodeMap = new Map((fixture.nodes ?? []).map((n) => [n.id, n]));
      return repairC4Views(fixture.views, nodeMap);
    }
    // ── release ────────────────────────────────────────────────────────────
    case "release.nextMinorVersion":
      return nextMinorVersion(fixture.releaseIndex);
    case "release.items":
      return releaseItems(fixture.detail);
    case "release.deriveCounts":
      return deriveReleaseCounts(fixture.detail);
    case "release.summaryFromDetail":
      return releaseSummaryFromDetail(fixture.detail, fixture.file);
    case "release.generatedIndex":
      return generatedReleaseIndex(fixture.existingIndex, fixture.detailEntries);
    case "release.indexGenerationChanges":
      return releaseIndexGenerationChanges(fixture.existingIndex, fixture.generatedIndex);
    case "release.build":
      return buildReleasePlan(fixture);
    case "release.merge":
      return mergeExistingReleasePlan(fixture.existingDetail, fixture.proposedDetail);
    case "release.changes":
      return releasePlanChanges(fixture);
    case "release.saveDraft":
      return saveReleasePlanDraft(fixture);
    case "release.approve":
      return approveReleasePlan(fixture);
    // ── instruction rules ───────────────────────────────────────────────
    case "instr.plannedMigration":
      return plannedInstructionRuleMigration({ files: fixture.files, existingRules: fixture.existingRules });
    case "instr.upsertRulePointer":
      return upsertRulePointer(fixture.text);
    // ── sync plan ────────────────────────────────────────────────────────
    case "sync.normalizeInstructionFiles":
      return normalizeSyncInstructionFiles(fixture.files, fixture.validInstructionFiles);
    case "sync.defaultChoices":
      return defaultSyncChoices({ rootPackageExists: fixture.rootPackageExists, instructionFiles: fixture.instructionFiles });
    case "sync.rememberedChoices":
      return rememberedSyncChoices(fixture.metadata, { instructionFiles: fixture.instructionFiles });
    case "sync.applyExplicitOptions":
      return applyExplicitSyncOptions(fixture.choices, fixture.options, { instructionFiles: fixture.instructionFiles });
    case "sync.operation":
      return syncOperation({ installing: fixture.installing, migrating: fixture.migrating });
    case "sync.writePlan":
      return syncWritePlan({ installing: fixture.installing, migrating: fixture.migrating, doctorRepairAvailable: fixture.doctorRepairAvailable, syncChoices: fixture.syncChoices, options: fixture.options });
    case "sync.shouldValidate":
      return shouldValidateSync({ options: fixture.options, installing: fixture.installing });
    case "sync.persistedChoices":
      return persistedSyncChoices(fixture);
    case "sync.metadataPatch":
      return syncMetadataPatch(fixture);
    default:
      throw new Error(`Unknown op: ${op}`);
  }
}

// ─── Rust dispatcher ─────────────────────────────────────────────────────────
function runRust(op, fixturePath) {
  try {
    const stdout = execFileSync(
      "cargo",
      ["run", "-q", "-p", "architext-core", "--bin", "domain_dump", "--", op, fixturePath],
      { cwd: repoRoot, encoding: "utf8", stdio: ["pipe", "pipe", "pipe"] }
    );
    return JSON.parse(stdout.trim());
  } catch (e) {
    const stderr = (e.stderr ?? "").toString();
    throw new Error(`cargo failed: ${e.message}\n${stderr}`);
  }
}

// ─── Semantic equality ────────────────────────────────────────────────────────
// Deep-equal: object keys order-insensitive, arrays order-sensitive, numbers numeric.
function semanticEqual(a, b) {
  if (a === b) return true;
  if (typeof a !== typeof b) return false;
  if (a === null || b === null) return a === b;
  if (Array.isArray(a) && Array.isArray(b)) {
    if (a.length !== b.length) return false;
    return a.every((v, i) => semanticEqual(v, b[i]));
  }
  if (Array.isArray(a) || Array.isArray(b)) return false;
  if (typeof a === "object") {
    const ka = Object.keys(a).sort();
    const kb = Object.keys(b).sort();
    if (ka.length !== kb.length) return false;
    if (!ka.every((k, i) => k === kb[i])) return false;
    return ka.every((k) => semanticEqual(a[k], b[k]));
  }
  // primitives
  if (typeof a === "number" && typeof b === "number") {
    return a === b || (isNaN(a) && isNaN(b));
  }
  return a === b;
}

// ─── Runner ──────────────────────────────────────────────────────────────────
let total = 0;
let green = 0;
const rows = [];

const ops = fs.readdirSync(fixtureBase).sort();

for (const op of ops) {
  const opDir = path.join(fixtureBase, op);
  if (!fs.statSync(opDir).isDirectory()) continue;
  const fixtures = fs.readdirSync(opDir)
    .filter((f) => f.endsWith(".json"))
    .sort();
  for (const fname of fixtures) {
    const fixturePath = path.join(opDir, fname);
    const fixture = JSON.parse(fs.readFileSync(fixturePath, "utf8"));
    total++;

    // Run JS
    let jsResult = null;
    let jsError = null;
    try {
      jsResult = runJs(op, fixture);
      // Round-trip through JSON so the comparison reflects what actually lands
      // on disk: writeJson does JSON.stringify, which DROPS undefined-valued
      // keys. The Rust port reads back via JSON.parse (no undefined possible),
      // so comparing raw in-memory JS objects (which can carry `key: undefined`)
      // would diverge from disk reality. This makes the gate the disk contract.
      if (jsResult !== null && jsResult !== undefined) {
        jsResult = JSON.parse(JSON.stringify(jsResult));
      }
    } catch (e) {
      jsError = e.message;
    }

    // Run Rust
    let rustResult = null;
    let rustError = null;
    try {
      rustResult = runRust(op, fixturePath);
      if (rustResult && typeof rustResult.__error__ === "string") {
        rustError = rustResult.__error__;
        rustResult = null;
      }
    } catch (e) {
      rustError = `CARGO_FAIL: ${e.message}`;
    }

    // Compare
    let pass = false;
    let detail = "";
    if (jsError !== null && rustError !== null) {
      pass = jsError === rustError;
      if (!pass) {
        detail = `JS error: ${JSON.stringify(jsError)}\nRS error: ${JSON.stringify(rustError)}`;
      }
    } else if (jsError === null && rustError === null) {
      pass = semanticEqual(jsResult, rustResult);
      if (!pass) {
        detail = `JS: ${JSON.stringify(jsResult, null, 2)}\nRS: ${JSON.stringify(rustResult, null, 2)}`;
      }
    } else {
      detail = jsError
        ? `JS threw ${JSON.stringify(jsError)} but Rust returned ${JSON.stringify(rustResult)}`
        : `Rust threw ${JSON.stringify(rustError)} but JS returned ${JSON.stringify(jsResult)}`;
    }

    if (pass) green++;
    rows.push({ op, fname: fname.replace(".json", ""), pass, detail });
  }
}

// ─── Report ───────────────────────────────────────────────────────────────────
const COL_OP = 20;
const COL_NAME = 36;

console.log(
  `${"op".padEnd(COL_OP)} ${"fixture".padEnd(COL_NAME)} result`
);
console.log("─".repeat(COL_OP + COL_NAME + 10));
for (const r of rows) {
  const icon = r.pass ? "\x1b[32mGREEN\x1b[0m" : "\x1b[31mRED\x1b[0m  ";
  console.log(`${r.op.padEnd(COL_OP)} ${r.fname.padEnd(COL_NAME)} ${icon}`);
  if (!r.pass && r.detail) {
    for (const line of r.detail.split("\n")) {
      console.log("    " + line);
    }
  }
}
console.log("─".repeat(COL_OP + COL_NAME + 10));
console.log(`\n${green}/${total} GREEN`);
process.exit(green === total ? 0 : 1);
