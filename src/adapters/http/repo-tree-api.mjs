// Serves the target repository's file list for the Repo Tree view. Prefers
// `git ls-files` (tracked files, honours .gitignore); falls back to a filtered
// filesystem walk when the target is not a git work tree. Each file is stat'd
// for size and last-modified time. The viewer builds the tree and maps files to
// architecture nodes (via node.sourcePaths) client-side.
//
// "Last modified" uses filesystem mtime (cheap, always available, accurate for
// in-place edits). Git per-file commit dates are intentionally not used here:
// they would cost one `git log` invocation per file (or a complex single-pass
// parse) which is disproportionate for the metadata column.

import { readdir, stat } from "node:fs/promises";
import path from "node:path";
import { git, gitAvailable } from "../cli/runtime.mjs";

const IGNORED_DIRS = new Set([
  ".git", "node_modules", "dist", ".vite", "coverage", ".nyc_output", ".cache", ".next", ".turbo"
]);
const MAX_WALK_FILES = 20000; // guard against pathological trees
const STAT_CONCURRENCY = 64; // bound open file descriptors while stat'ing

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

// Stat each repo-relative path against the target root, with bounded
// concurrency. Files that cannot be stat'd (listed by git but absent on disk)
// are returned with null size/mtime so the row still renders.
async function statEntries(target, paths, statFn) {
  const entries = new Array(paths.length);
  let cursor = 0;
  async function worker() {
    while (cursor < paths.length) {
      const index = cursor++;
      const relative = paths[index];
      try {
        const info = await statFn(path.join(target, relative));
        entries[index] = { path: relative, size: info.size, mtime: Math.round(info.mtimeMs) };
      } catch {
        entries[index] = { path: relative, size: null, mtime: null };
      }
    }
  }
  const workers = Array.from({ length: Math.min(STAT_CONCURRENCY, paths.length) }, worker);
  await Promise.all(workers);
  return entries;
}

export async function repoTreeFiles(
  target,
  { gitAvailableFn = gitAvailable, gitFn = git, walkFn = filesystemWalk, statFn = stat } = {}
) {
  let paths = null;
  let source = "filesystem";
  if (gitAvailableFn(target)) {
    try {
      const out = gitFn(target, ["ls-files"]);
      const tracked = out.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
      if (tracked.length > 0) {
        paths = tracked;
        source = "git";
      }
    } catch {
      // fall through to the filesystem walk
    }
  }
  if (!paths) paths = await walkFn(target);
  const files = await statEntries(target, paths, statFn);
  return { files, source };
}

export async function repoTreeApiRequest(target, options) {
  return repoTreeFiles(target, options);
}
