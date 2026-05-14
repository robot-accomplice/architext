#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { readFileSync } from "node:fs";
import { readdir, readFile, rm } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const architextDir = path.resolve(scriptDir, "..");
const projectRoot = path.resolve(architextDir, "..", "..");
const installMetadataFile = path.join(architextDir, ".architext-install.json");
const gitignoreEntries = ["docs/architext/node_modules/", "docs/architext/dist/"];

function usage() {
  return `Usage:
  node docs/architext/tools/architext-project.mjs <command> [options]

Commands:
  serve                  Run the local Architext viewer.
  validate              Validate Architext JSON.
  build                 Build the local Architext viewer.
  doctor                Check local installation health.
  status                Print local installation status. Use --json for machine output.
  prompt                Print LLM maintenance prompt.
  clean                 Remove docs/architext/dist.
  explain [topic]       Explain schema/data contracts.

Options:
  --json                Machine-readable status/doctor output.
  --node-modules        Let clean remove docs/architext/node_modules.
  --mode <name>         Prompt mode: initial-buildout, architecture-change, repair-validation.`;
}

function parseArgs(argv) {
  const command = argv[0] && !argv[0].startsWith("--") ? argv[0] : "serve";
  const rest = command === argv[0] ? argv.slice(1) : argv;
  const options = { command, json: false, nodeModules: false, mode: "initial-buildout", topic: "" };
  for (let index = 0; index < rest.length; index += 1) {
    const arg = rest[index];
    if (arg === "--json") options.json = true;
    else if (arg === "--node-modules") options.nodeModules = true;
    else if (arg === "--mode") options.mode = rest[++index] ?? "";
    else if (arg === "--help" || arg === "-h") options.command = "help";
    else if (options.command === "explain" && !options.topic) options.topic = arg;
    else throw new Error(`Unknown argument: ${arg}`);
  }
  return options;
}

function run(command, args, cwd, stdio = "inherit") {
  return execFileSync(command, args, {
    cwd,
    encoding: stdio === "inherit" ? undefined : "utf8",
    stdio,
    shell: process.platform === "win32"
  });
}

function tryRun(command, args, cwd) {
  try {
    return { ok: true, output: run(command, args, cwd, ["ignore", "pipe", "pipe"]).trim() };
  } catch (error) {
    return {
      ok: false,
      output: `${error.stdout?.toString?.() ?? ""}${error.stderr?.toString?.() ?? ""}`.trim() || error.message
    };
  }
}

function readJson(filePath) {
  return JSON.parse(readFileSync(filePath, "utf8"));
}

function gitAvailable() {
  return tryRun("git", ["rev-parse", "--is-inside-work-tree"], projectRoot).ok;
}

async function status({ runValidation = false } = {}) {
  const packagePath = path.join(architextDir, "package.json");
  const manifestPath = path.join(architextDir, "data", "manifest.json");
  const gitignorePath = path.join(projectRoot, ".gitignore");
  const packageJson = existsSync(packagePath) ? readJson(packagePath) : null;
  const manifest = existsSync(manifestPath) ? readJson(manifestPath) : null;
  const metadata = existsSync(installMetadataFile) ? readJson(installMetadataFile) : null;
  const gitignoreText = existsSync(gitignorePath) ? await readFile(gitignorePath, "utf8") : "";
  const gitignoreLines = gitignoreText.split(/\r?\n/);
  const trackedGenerated = gitAvailable()
    ? tryRun("git", ["ls-files", "docs/architext/node_modules", "docs/architext/dist"], projectRoot).output.split(/\r?\n/).filter(Boolean)
    : [];
  const validation = runValidation ? tryRun("npm", ["run", "validate"], architextDir) : null;
  return {
    projectRoot,
    architextDir,
    project: manifest?.project ?? null,
    installedVersion: packageJson?.version ?? null,
    metadata,
    gitignoreMissing: gitignoreEntries.filter((entry) => !gitignoreLines.includes(entry)),
    trackedGenerated,
    validation
  };
}

function printStatus(value, verbose = false) {
  console.log(`Project: ${value.project?.name ?? path.basename(projectRoot)}`);
  console.log(`Architext: ${value.installedVersion ?? "unknown"}`);
  console.log(`Gitignore: ${value.gitignoreMissing.length ? `missing ${value.gitignoreMissing.join(", ")}` : "ok"}`);
  console.log(`Generated artifacts tracked: ${value.trackedGenerated.length ? value.trackedGenerated.length : "none"}`);
  if (value.validation) {
    console.log(`Validation: ${value.validation.ok ? "passed" : "failed"}`);
    if (!value.validation.ok || verbose) console.log(value.validation.output);
  }
  if (value.metadata?.updatedAt) console.log(`Last update: ${value.metadata.updatedAt}`);
}

async function printPrompt(mode) {
  const value = await status();
  const projectName = value.project?.name ?? path.basename(projectRoot);
  const modes = new Set(["initial-buildout", "architecture-change", "repair-validation"]);
  const promptMode = modes.has(mode) ? mode : "initial-buildout";
  const lead = {
    "initial-buildout": `Build out Architext for ${projectName}. Replace neutral starter data with source-backed architecture facts.`,
    "architecture-change": `Update Architext for the architecture changes just made in ${projectName}. Keep existing stable IDs where concepts already exist.`,
    "repair-validation": `Repair Architext JSON validation failures for ${projectName}. Do not change application code for this task.`
  }[promptMode];
  console.log(`${lead}

First read AGENTS.md/CLAUDE.md if present, then docs/architext/LLM_ARCHITEXT.md, README.md, schema/*.schema.json, and data/*.json.

Rules:
- Update only docs/architext/data/*.json unless the schema or template is clearly wrong.
- Reuse stable IDs, create nodes before references, keep flows ordered, and prefer source-path-backed claims.
- Mark uncertainty and known gaps explicitly.
- Do not persist docs/architext/node_modules/ or docs/architext/dist/.
- Run cd docs/architext && npm run validate before claiming completion.`);
}

async function clean(options) {
  const candidates = [path.join(architextDir, "dist")];
  if (options.nodeModules) candidates.push(path.join(architextDir, "node_modules"));
  const removed = [];
  for (const candidate of candidates) {
    if (existsSync(candidate)) {
      await rm(candidate, { recursive: true, force: true });
      removed.push(candidate);
    }
  }
  console.log(removed.length ? `Removed:\n${removed.map((item) => `- ${item}`).join("\n")}` : "No generated Architext artifacts found.");
}

async function explain(topic) {
  const normalized = (topic || "overview").toLowerCase();
  const files = await readdir(path.join(architextDir, "schema"));
  const match = files.find((file) => file.startsWith(normalized) || file.includes(normalized));
  if (!match) {
    console.log("Architext data is split across manifest, nodes, flows, views, data classification, decisions, risks, and glossary JSON files.");
    return;
  }
  const schema = readJson(path.join(architextDir, "schema", match));
  console.log(`${normalized}: docs/architext/schema/${match}`);
  if (schema.required?.length) console.log(`Required fields: ${schema.required.join(", ")}`);
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.command === "help") return console.log(usage());
  if (options.command === "serve") return run("npm", ["run", "dev"], architextDir);
  if (options.command === "validate") return run("npm", ["run", "validate"], architextDir);
  if (options.command === "build") return run("npm", ["run", "build"], architextDir);
  if (options.command === "prompt") return printPrompt(options.mode);
  if (options.command === "clean") return clean(options);
  if (options.command === "explain") return explain(options.topic);
  if (options.command === "status" || options.command === "doctor") {
    const value = await status({ runValidation: options.command === "doctor" });
    if (options.json) console.log(JSON.stringify(value, null, 2));
    else printStatus(value, options.command === "doctor");
    return;
  }
  throw new Error(`Unknown command: ${options.command}`);
}

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
