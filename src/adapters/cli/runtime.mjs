import { execFileSync } from "node:child_process";
import { mkdir, readFile, rename, rm, stat, writeFile } from "node:fs/promises";
import path from "node:path";

export function run(command, args, cwd, extraEnv = {}) {
  console.log(`Running: ${command} ${args.join(" ")}`);
  execFileSync(command, args, {
    cwd,
    stdio: "inherit",
    shell: process.platform === "win32",
    env: { ...process.env, ...extraEnv }
  });
}

export function tryRun(command, args, cwd, extraEnv = {}) {
  try {
    return {
      ok: true,
      output: execFileSync(command, args, {
        cwd,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "pipe"],
        shell: process.platform === "win32",
        env: { ...process.env, ...extraEnv }
      }).trim()
    };
  } catch (error) {
    return {
      ok: false,
      output: `${error.stdout?.toString?.() ?? ""}${error.stderr?.toString?.() ?? ""}`.trim() || error.message
    };
  }
}

export function git(target, args) {
  return execFileSync("git", args, { cwd: target, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
}

export function gitAvailable(target) {
  try {
    git(target, ["rev-parse", "--is-inside-work-tree"]);
    return true;
  } catch {
    return false;
  }
}

export async function readJson(file) {
  return JSON.parse(await readFile(file, "utf8"));
}

export async function writeJson(file, value) {
  const directory = path.dirname(file);
  await mkdir(directory, { recursive: true });
  const temporaryFile = path.join(directory, `.${path.basename(file)}.${process.pid}.${Date.now()}.tmp`);
  try {
    await writeFile(temporaryFile, `${JSON.stringify(value, null, 2)}\n`, "utf8");
    await rename(temporaryFile, file);
  } catch (error) {
    await rm(temporaryFile, { force: true }).catch(() => {});
    throw error;
  }
}

export async function assertDirectory(target, label = "Target") {
  const targetStat = await stat(target).catch(() => null);
  if (!targetStat?.isDirectory()) throw new Error(`${label} is not a directory: ${target}`);
}
