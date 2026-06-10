// Serves the target repository's file list for the Repo Tree view. Prefers
// `git ls-files` (tracked files, honours .gitignore); falls back to a filtered
// filesystem walk when the target is not a git work tree. The viewer builds the
// tree and maps files to architecture nodes (via node.sourcePaths) client-side.

import { readdir } from "node:fs/promises";
import path from "node:path";
import { git, gitAvailable } from "../cli/runtime.mjs";

const IGNORED_DIRS = new Set([
  ".git", "node_modules", "dist", ".vite", "coverage", ".nyc_output", ".cache", ".next", ".turbo"
]);
const MAX_WALK_FILES = 20000; // guard against pathological trees

async function filesystemWalk(root) {
  const files = [];
  async function walk(dir, relative) {
    if (files.length >= MAX_WALK_FILES) return;
    const entries = await readdir(dir, { withFileTypes: true }).catch(() => []);
    for (const entry of entries) {
      const childRelative = relative ? `${relative}/${entry.name}` : entry.name;
      if (entry.isDirectory()) {
        if (IGNORED_DIRS.has(entry.name)) continue;
        await walk(path.join(dir, entry.name), childRelative);
      } else if (entry.isFile()) {
        if (files.length >= MAX_WALK_FILES) return;
        files.push(childRelative);
      }
    }
  }
  await walk(root, "");
  return files.sort();
}

export async function repoTreeFiles(target, { gitAvailableFn = gitAvailable, gitFn = git, walkFn = filesystemWalk } = {}) {
  if (gitAvailableFn(target)) {
    try {
      const out = gitFn(target, ["ls-files"]);
      const files = out.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
      if (files.length > 0) return { files, source: "git" };
    } catch {
      // fall through to the filesystem walk
    }
  }
  return { files: await walkFn(target), source: "filesystem" };
}

export async function repoTreeApiRequest(target, options) {
  return repoTreeFiles(target, options);
}
