#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { copyFile, mkdir, readdir, readFile, stat, writeFile } from "node:fs/promises";
import { createInterface } from "node:readline/promises";
import path from "node:path";
import { stdin as input, stdout as output } from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const templateDir = path.join(repoRoot, "docs", "architext");
const appendixPath = path.join(templateDir, "AGENTS_APPENDIX.md");
const skippedDirectories = new Set(["node_modules", "dist", ".git"]);
const skippedFiles = new Set([".DS_Store"]);
const instructionFiles = ["AGENTS.md", "CLAUDE.md"];

function usage() {
  return `Usage:
  node /path/to/architext/tools/architext-adopt.mjs [install|upgrade|sync] [options]

Options:
  --target <repo>          Target repository. Defaults to the current directory.
  --yes                    Accept default prompts.
  --dry-run                Show intended changes without writing files.
  --force                  Re-copy template files even when versions match.
  --overwrite-data         Replace docs/architext/data/*.json with neutral starter data during upgrade.
  --append-agents          Append/create both AGENTS.md and CLAUDE.md without prompting.
  --no-agents              Do not prompt for AGENTS.md or CLAUDE.md changes.
  --skip-install           Do not run npm install after writing artifacts.
  --skip-validate          Do not run npm run validate after writing artifacts.
  --branch current         Use the current branch without prompting.
  --branch new             Create a new branch before writing.
  --branch none            Skip git branch handling.
  --branch-name <name>     Branch name to use with --branch new.

Default command is sync: install when Architext is absent, upgrade when the
installed template version differs, and no-op when it is current unless --force
is passed. Install seeds neutral starter data instead of the Architext demo.
Upgrade preserves target docs/architext/data/*.json by default.`;
}

function parseArgs(argv) {
  const knownCommands = new Set(["install", "upgrade", "sync", "help"]);
  const first = argv[0];
  const hasCommand = first && !first.startsWith("--") && knownCommands.has(first);
  const command = hasCommand ? first : "sync";
  const rest = hasCommand ? argv.slice(1) : argv;
  const options = {
    command,
    target: process.cwd(),
    yes: false,
    dryRun: false,
    force: false,
    overwriteData: false,
    appendAgents: false,
    noAgents: false,
    skipInstall: false,
    skipValidate: false,
    branch: "",
    branchName: ""
  };

  for (let index = 0; index < rest.length; index += 1) {
    const arg = rest[index];
    if (arg === "--target") {
      options.target = rest[++index] ?? "";
    } else if (arg === "--yes" || arg === "-y") {
      options.yes = true;
    } else if (arg === "--dry-run") {
      options.dryRun = true;
    } else if (arg === "--force") {
      options.force = true;
    } else if (arg === "--overwrite-data") {
      options.overwriteData = true;
    } else if (arg === "--append-agents") {
      options.appendAgents = true;
    } else if (arg === "--no-agents") {
      options.noAgents = true;
    } else if (arg === "--skip-install") {
      options.skipInstall = true;
    } else if (arg === "--skip-validate") {
      options.skipValidate = true;
    } else if (arg === "--branch") {
      options.branch = rest[++index] ?? "";
    } else if (arg === "--branch-name") {
      options.branchName = rest[++index] ?? "";
    } else if (arg === "--help" || arg === "-h") {
      options.command = "help";
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }

  return options;
}

function runCommand(command, args, cwd) {
  console.log(`Running: ${command} ${args.join(" ")}`);
  execFileSync(command, args, {
    cwd,
    stdio: "inherit",
    shell: process.platform === "win32"
  });
}

function git(target, args) {
  return execFileSync("git", args, {
    cwd: target,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "ignore"]
  }).trim();
}

function gitAvailable(target) {
  try {
    git(target, ["rev-parse", "--is-inside-work-tree"]);
    return true;
  } catch {
    return false;
  }
}

function defaultBranchName(operation, version) {
  return `architext/${operation}-${version}`.replaceAll(".", "-");
}

async function promptYesNo(rl, question, defaultValue) {
  const suffix = defaultValue ? "Y/n" : "y/N";
  const answer = (await rl.question(`${question} [${suffix}] `)).trim().toLowerCase();
  if (!answer) return defaultValue;
  return ["y", "yes"].includes(answer);
}

async function promptText(rl, question, defaultValue) {
  const answer = (await rl.question(`${question} [${defaultValue}] `)).trim();
  return answer || defaultValue;
}

function isDataFile(relativePath) {
  const parts = relativePath.split(path.sep);
  return parts[0] === "data" && parts.length === 2 && parts[1].endsWith(".json");
}

async function readJson(file) {
  return JSON.parse(await readFile(file, "utf8"));
}

async function readTemplateVersion() {
  return (await readJson(path.join(templateDir, "package.json"))).version;
}

async function readInstalledVersion(target) {
  const packagePath = path.join(target, "docs", "architext", "package.json");
  if (!existsSync(packagePath)) return null;
  return (await readJson(packagePath)).version ?? null;
}

async function detectOperation(options, target, templateVersion) {
  const destination = path.join(target, "docs", "architext");
  const installed = existsSync(destination);
  const installedVersion = await readInstalledVersion(target);

  if (options.command === "install") {
    return { operation: "install", installed, installedVersion, shouldWrite: true };
  }

  if (options.command === "upgrade") {
    return { operation: "upgrade", installed, installedVersion, shouldWrite: true };
  }

  if (!installed) {
    return { operation: "install", installed, installedVersion, shouldWrite: true };
  }

  const needsUpgrade = installedVersion !== templateVersion;
  return {
    operation: "upgrade",
    installed,
    installedVersion,
    shouldWrite: options.force || needsUpgrade,
    needsUpgrade
  };
}

async function walkFiles(directory, base = directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];

  for (const entry of entries) {
    if (entry.isDirectory() && skippedDirectories.has(entry.name)) continue;
    if (entry.isFile() && skippedFiles.has(entry.name)) continue;

    const absolutePath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...await walkFiles(absolutePath, base));
    } else if (entry.isFile()) {
      files.push(path.relative(base, absolutePath));
    }
  }

  return files;
}

async function copyTemplate({ operation, target, force, dryRun, overwriteData }) {
  const destination = path.join(target, "docs", "architext");
  const destinationExists = existsSync(destination);

  if (operation === "install" && destinationExists && !force) {
    throw new Error(`${destination} already exists. Use upgrade, sync, or install with --force.`);
  }

  if (operation === "upgrade" && !destinationExists) {
    throw new Error(`${destination} does not exist. Use install or sync first.`);
  }

  const files = await walkFiles(templateDir);
  const copied = [];
  const preserved = [];
  const skippedData = [];

  for (const relativePath of files) {
    if (isDataFile(relativePath)) {
      if (operation === "upgrade" && !overwriteData) {
        preserved.push(relativePath);
      } else {
        skippedData.push(relativePath);
      }
      continue;
    }

    const source = path.join(templateDir, relativePath);
    const targetPath = path.join(destination, relativePath);

    copied.push(relativePath);
    if (!dryRun) {
      await mkdir(path.dirname(targetPath), { recursive: true });
      await copyFile(source, targetPath);
    }
  }

  if ((operation === "install" || overwriteData) && !dryRun) {
    await writeStarterData(target);
  }

  return { destination, copied, preserved, skippedData };
}

function slugify(value) {
  const slug = value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return slug || "target-project";
}

async function writeJson(filePath, value) {
  await mkdir(path.dirname(filePath), { recursive: true });
  await writeFile(filePath, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

async function writeStarterData(target) {
  const dataDir = path.join(target, "docs", "architext", "data");
  const projectName = path.basename(target);
  const projectId = slugify(projectName);
  const systemId = `${projectId}-system`;
  const actorId = "project-team";
  const dataId = "architecture-knowledge";
  const flowId = "architecture-buildout";

  await writeJson(path.join(dataDir, "manifest.json"), {
    schemaVersion: await readTemplateVersion(),
    project: {
      id: projectId,
      name: projectName,
      summary: "Architext has been installed. Replace this starter model with the real project architecture."
    },
    generatedAt: new Date().toISOString(),
    defaultViewId: "system-map",
    files: {
      nodes: "nodes.json",
      flows: "flows.json",
      views: "views.json",
      dataClassification: "data-classification.json",
      decisions: "decisions.json",
      risks: "risks.json",
      glossary: "glossary.json"
    },
    notes: [
      "Starter data only. Ask an LLM to inspect the codebase and build out docs/architext/data/*.json.",
      "Do not treat this starter model as architecture documentation for the target project."
    ]
  });

  await writeJson(path.join(dataDir, "nodes.json"), {
    nodes: [
      {
        id: actorId,
        type: "actor",
        name: "Project Team",
        summary: "Placeholder actor for the team or user initiating the Architext build-out.",
        responsibilities: ["Replace starter data with real architecture facts"],
        owner: "Project maintainers",
        sourcePaths: [],
        runtime: "Repository workflow",
        interfaces: ["Architext JSON"],
        dependencies: [systemId],
        dataHandled: [dataId],
        security: ["Unknown until architecture build-out is complete"],
        observability: ["Unknown until architecture build-out is complete"],
        relatedFlows: [flowId],
        relatedDecisions: [],
        knownRisks: ["architext-starter-data"],
        verification: ["cd docs/architext && npm run validate"]
      },
      {
        id: systemId,
        type: "software-system",
        name: projectName,
        summary: "Placeholder system boundary. Replace with the real project systems, services, stores, flows, and dependencies.",
        responsibilities: ["Pending architecture discovery"],
        owner: "Project maintainers",
        sourcePaths: [],
        runtime: "Unknown until architecture build-out is complete",
        interfaces: ["Unknown until architecture build-out is complete"],
        dependencies: [],
        dataHandled: [dataId],
        security: ["Unknown until architecture build-out is complete"],
        observability: ["Unknown until architecture build-out is complete"],
        relatedFlows: [flowId],
        relatedDecisions: ["architext-buildout-required"],
        knownRisks: ["architext-starter-data"],
        verification: ["cd docs/architext && npm run validate"]
      }
    ]
  });

  await writeJson(path.join(dataDir, "flows.json"), {
    flows: [
      {
        id: flowId,
        name: "Architext build-out required",
        status: "planned",
        summary: "Starter flow showing that architecture data still needs to be generated from the target repository.",
        trigger: "Architext installed into the project",
        actors: [actorId],
        steps: [
          {
            id: "inspect-project",
            from: actorId,
            to: systemId,
            action: "inspectCodebaseAndReplaceStarterData",
            summary: "An LLM should inspect the repository and replace every starter JSON file with real architecture data.",
            data: [dataId]
          }
        ],
        guarantees: ["Validation passes for starter data"],
        failureBehavior: ["Rendered site is not useful until project-specific data replaces the starter model"],
        observability: ["Validation output"],
        verification: ["cd docs/architext && npm run validate"],
        knownGaps: ["All project architecture facts are pending discovery"]
      }
    ]
  });

  await writeJson(path.join(dataDir, "views.json"), {
    views: [
      {
        id: "system-map",
        name: "System Map",
        type: "system-map",
        summary: "Starter view. Replace with the real project system map.",
        lanes: [
          { id: "people", name: "People", nodeIds: [actorId] },
          { id: "system", name: "System", nodeIds: [systemId] }
        ]
      },
      {
        id: "dataflow",
        name: "Dataflow",
        type: "dataflow",
        summary: "Starter dataflow. Replace with real data movement.",
        lanes: [
          { id: "source", name: "Source", nodeIds: [actorId] },
          { id: "target", name: "Target", nodeIds: [systemId] }
        ]
      },
      {
        id: "sequence",
        name: "Sequence",
        type: "sequence",
        summary: "Starter sequence for the build-out flow.",
        lanes: [
          { id: "participants", name: "Participants", nodeIds: [actorId, systemId] }
        ]
      },
      {
        id: "deployment",
        name: "Deployment",
        type: "deployment",
        summary: "Starter deployment view. Replace with real runtime placement.",
        lanes: [
          { id: "unknown", name: "Unknown", nodeIds: [systemId] }
        ]
      },
      {
        id: "c4-context",
        name: "C4 Context",
        type: "c4-context",
        summary: "Starter C4 context. Replace with real actors, system boundary, and external systems.",
        lanes: [
          { id: "people", name: "People", nodeIds: [actorId] },
          { id: "system", name: "System", nodeIds: [systemId] }
        ]
      },
      {
        id: "c4-container",
        name: "C4 Container",
        type: "c4-container",
        summary: "Starter C4 container view. Replace with deployable units and dependencies.",
        lanes: [
          { id: "containers", name: "Containers", nodeIds: [systemId] }
        ]
      },
      {
        id: "c4-component",
        name: "C4 Component",
        type: "c4-component",
        summary: "Starter C4 component view. Replace with components inside a selected container.",
        lanes: [
          { id: "components", name: "Components", nodeIds: [systemId] }
        ]
      }
    ]
  });

  await writeJson(path.join(dataDir, "data-classification.json"), {
    classes: [
      {
        id: dataId,
        name: "Architecture Knowledge",
        sensitivity: "medium",
        handling: "Review generated architecture facts before treating them as project documentation."
      }
    ]
  });

  await writeJson(path.join(dataDir, "decisions.json"), {
    decisions: [
      {
        id: "architext-buildout-required",
        status: "planned",
        title: "Replace starter Architext data",
        context: "Architext was installed with neutral starter data so the adopted project does not render the template demo.",
        decision: "An LLM must inspect the target repository and replace docs/architext/data/*.json with project-specific architecture facts.",
        consequences: ["The site validates immediately", "The starter model is intentionally not useful as final documentation"],
        relatedNodes: [systemId],
        relatedFlows: [flowId]
      }
    ]
  });

  await writeJson(path.join(dataDir, "risks.json"), {
    risks: [
      {
        id: "architext-starter-data",
        title: "Starter data is not project architecture",
        category: "technical",
        severity: "high",
        status: "open",
        summary: "The installed Architext data is a placeholder until an LLM builds out the real architecture model.",
        mitigations: ["Run the LLM JSON build-out workflow", "Review generated JSON diffs", "Run npm run validate"],
        relatedNodes: [systemId],
        relatedFlows: [flowId]
      }
    ]
  });

  await writeJson(path.join(dataDir, "glossary.json"), {
    terms: [
      {
        term: "Architext starter data",
        definition: "A neutral validating placeholder installed into new projects before real architecture data is generated."
      }
    ]
  });
}

async function appendixMarkdown() {
  const appendix = await readFile(appendixPath, "utf8");
  const match = appendix.match(/```markdown\n([\s\S]*?)\n```/);
  return (match?.[1] ?? appendix).trim();
}

async function upsertInstructionFile({ target, fileName, dryRun }) {
  const destination = path.resolve(target, fileName);
  const marker = "## Architext Architecture Documentation";
  const appendix = await appendixMarkdown();
  const existing = existsSync(destination) ? await readFile(destination, "utf8") : "";

  if (existing.includes(marker)) {
    return { destination, changed: false, reason: "already present" };
  }

  if (!dryRun) {
    await mkdir(path.dirname(destination), { recursive: true });
    const prefix = existing.trimEnd();
    const next = `${prefix}${prefix ? "\n\n" : ""}${appendix}\n`;
    await writeFile(destination, next, "utf8");
  }

  return { destination, changed: true, created: !existing };
}

async function handleBranch({ target, operation, templateVersion, options, rl }) {
  if (options.dryRun || options.branch === "none" || !gitAvailable(target)) return;

  let branchChoice = options.branch;
  if (!branchChoice && !options.yes) {
    const createBranch = await promptYesNo(rl, "Create a new git branch for Architext changes?", false);
    branchChoice = createBranch ? "new" : "current";
  }
  if (!branchChoice) branchChoice = "current";

  if (branchChoice === "current") return;
  if (branchChoice !== "new") throw new Error("--branch must be current, new, or none");

  const currentBranch = git(target, ["branch", "--show-current"]) || "current";
  const defaultName = options.branchName || defaultBranchName(operation, templateVersion);
  const branchName = options.branchName || options.yes
    ? defaultName
    : await promptText(rl, `New branch name from ${currentBranch}`, defaultName);

  git(target, ["checkout", "-b", branchName]);
  console.log(`Created and switched to branch ${branchName}`);
}

async function chooseInstructionFiles({ options, rl }) {
  if (options.noAgents) return [];
  if (options.appendAgents || options.yes) return instructionFiles;

  const selected = [];
  for (const fileName of instructionFiles) {
    const update = await promptYesNo(rl, `Append/create ${fileName} with Architext instructions?`, true);
    if (update) selected.push(fileName);
  }
  return selected;
}

async function runPostInstall({ target, options, wroteTemplate }) {
  if (options.dryRun || !wroteTemplate) return;

  const architextDir = path.join(target, "docs", "architext");

  if (!options.skipInstall) {
    runCommand("npm", ["install"], architextDir);
  }

  if (!options.skipValidate) {
    if (options.skipInstall && !existsSync(path.join(architextDir, "node_modules"))) {
      console.log("Skipping validation because --skip-install was used and node_modules is absent.");
      return;
    }
    runCommand("npm", ["run", "validate"], architextDir);
  }
}

async function assertTarget(target) {
  const targetStat = await stat(target).catch(() => null);
  if (!targetStat?.isDirectory()) {
    throw new Error(`Target is not a directory: ${target}`);
  }
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.command === "help") {
    console.log(usage());
    return;
  }

  const target = path.resolve(options.target || process.cwd());
  await assertTarget(target);

  const templateVersion = await readTemplateVersion();
  const detected = await detectOperation(options, target, templateVersion);

  console.log(`Target: ${target}`);
  console.log(`Architext template: ${templateVersion}`);
  console.log(`Installed: ${detected.installedVersion ?? "none"}`);
  console.log(`Operation: ${detected.operation}${detected.shouldWrite ? "" : " (current)"}`);

  const rl = createInterface({ input, output });
  try {
    const files = await chooseInstructionFiles({ options, rl });
    const hasTemplateWrites = detected.shouldWrite || options.force;

    if (!hasTemplateWrites) {
      console.log("No template upgrade needed. Use --force to refresh template-owned files anyway.");
    }

    if (!hasTemplateWrites && files.length === 0) {
      return;
    }

    await handleBranch({ target, operation: detected.operation, templateVersion, options, rl });

    if (!options.yes && !options.dryRun) {
      const proceed = await promptYesNo(rl, "Proceed with selected Architext changes in this branch?", true);
      if (!proceed) {
        console.log("Aborted.");
        return;
      }
    }

    let result = { copied: [], preserved: [], destination: path.join(target, "docs", "architext") };
    if (hasTemplateWrites) {
      result = await copyTemplate({
        operation: detected.operation,
        target,
        force: options.force,
        dryRun: options.dryRun,
        overwriteData: options.overwriteData
      });
      console.log(`${options.dryRun ? "Would copy" : "Copied"} ${result.copied.length} files to ${result.destination}`);
      if (result.preserved.length) {
        console.log(`Preserved ${result.preserved.length} target data files`);
      }
    }

    await runPostInstall({ target, options, wroteTemplate: hasTemplateWrites });

    for (const fileName of files) {
      const instructionResult = await upsertInstructionFile({ target, fileName, dryRun: options.dryRun });
      const verb = options.dryRun ? "Would update" : "Updated";
      console.log(
        instructionResult.changed
          ? `${verb} ${instructionResult.destination}`
          : `Skipped ${instructionResult.destination}: ${instructionResult.reason}`
      );
    }
  } finally {
    rl.close();
  }
}

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
