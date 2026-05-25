import { createInterface } from "node:readline/promises";
import { stdin as input, stdout as output } from "node:process";

const defaultPackageName = "@robotaccomplice/architext";
const defaultExecutable = "architext";

function versionParts(version) {
  const [core, prerelease = ""] = version
    .replace(/^v/, "")
    .split("+", 1)[0]
    .split("-", 2);
  return {
    core: core
      .split(".")
      .map((part) => Number(part))
      .concat([0, 0, 0])
      .slice(0, 3),
    prerelease: prerelease ? prerelease.split(".") : []
  };
}

function comparePrereleaseIdentifiers(left, right) {
  const leftNumber = /^\d+$/.test(left) ? Number(left) : null;
  const rightNumber = /^\d+$/.test(right) ? Number(right) : null;
  if (leftNumber !== null && rightNumber !== null) return Math.sign(leftNumber - rightNumber);
  if (leftNumber !== null) return -1;
  if (rightNumber !== null) return 1;
  return left.localeCompare(right);
}

function comparePrerelease(left, right) {
  if (!left.length && !right.length) return 0;
  if (!left.length) return 1;
  if (!right.length) return -1;
  const length = Math.max(left.length, right.length);
  for (let index = 0; index < length; index += 1) {
    if (left[index] === undefined) return -1;
    if (right[index] === undefined) return 1;
    const comparison = comparePrereleaseIdentifiers(left[index], right[index]);
    if (comparison !== 0) return comparison;
  }
  return 0;
}

export function compareVersions(left, right) {
  const leftParts = versionParts(left);
  const rightParts = versionParts(right);
  for (let index = 0; index < 3; index += 1) {
    if (leftParts.core[index] > rightParts.core[index]) return 1;
    if (leftParts.core[index] < rightParts.core[index]) return -1;
  }
  return comparePrerelease(leftParts.prerelease, rightParts.prerelease);
}

export function parseRefreshSelection(answer, instances) {
  const normalized = answer.trim().toLowerCase();
  if (!normalized || ["a", "all", "y", "yes"].includes(normalized)) {
    return instances.map((instance) => instance.id);
  }
  if (["n", "no", "none", "skip"].includes(normalized)) return [];

  const ids = normalized.split(/[\s,]+/).filter(Boolean).map((token) => {
    const index = Number(token);
    if (Number.isInteger(index) && index >= 1 && index <= instances.length) return instances[index - 1].id;
    return token;
  });
  const known = new Set(instances.map((instance) => instance.id));
  const unknown = ids.filter((id) => !known.has(id));
  if (unknown.length) throw new Error(`Unknown instance selection: ${unknown.join(", ")}`);
  return [...new Set(ids)];
}

async function prompt(question) {
  const rl = createInterface({ input, output });
  try {
    return await rl.question(question);
  } finally {
    rl.close();
  }
}

async function promptYesNo(promptLine, question, defaultValue) {
  const suffix = defaultValue ? "Y/n" : "y/N";
  const answer = (await promptLine(`${question} [${suffix}] `)).trim().toLowerCase();
  if (!answer) return defaultValue;
  return ["y", "yes"].includes(answer);
}

function latestVersionFrom(outputText) {
  return outputText.trim().replace(/^"|"$/g, "");
}

function parseListOutput(outputText) {
  const parsed = JSON.parse(outputText || "{}");
  return Array.isArray(parsed.instances) ? parsed.instances : [];
}

export async function runPackageUpdateCheck({
  currentVersion,
  options,
  packageName = defaultPackageName,
  executable = defaultExecutable,
  cwd = process.cwd(),
  runCommand,
  tryRunCommand,
  promptLine = prompt
}) {
  const latestResult = tryRunCommand("npm", ["view", packageName, "version"], cwd);
  if (!latestResult.ok) throw new Error(`Could not check npm for ${packageName}: ${latestResult.output}`);

  const latestVersion = latestVersionFrom(latestResult.output);
  console.log(`Architext current: ${currentVersion}`);
  console.log(`Architext latest: ${latestVersion}`);

  if (compareVersions(latestVersion, currentVersion) <= 0) {
    console.log("Architext is already current.");
    return;
  }

  const shouldInstall = options.yes || await promptYesNo(promptLine, `Install ${packageName}@${latestVersion}?`, true);
  if (!shouldInstall) {
    console.log("Package update skipped.");
    return;
  }

  runCommand("npm", ["install", "-g", `${packageName}@${latestVersion}`], cwd);
  console.log(`Installed ${packageName}@${latestVersion}.`);

  const listResult = tryRunCommand(executable, ["--list", "--json"], cwd);
  if (!listResult.ok) throw new Error(`Could not list running Architext instances after update: ${listResult.output}`);
  const instances = parseListOutput(listResult.output);
  if (!instances.length) {
    console.log("No running Architext instances need refresh.");
    return;
  }

  console.log("Running Architext instances:");
  instances.forEach((instance, index) => {
    console.log(`${index + 1}. ${instance.id}  ${instance.url}  ${instance.target}`);
  });

  const selectionAnswer = options.yes
    ? "all"
    : await promptLine("Refresh instances with the newly installed Architext? [all/none/id list] ");
  const selectedIds = parseRefreshSelection(selectionAnswer, instances);
  if (!selectedIds.length) {
    console.log("No running instances refreshed.");
    return;
  }

  for (const id of selectedIds) {
    runCommand(executable, ["serve", "--refresh", "--instance", id], cwd);
  }
}
