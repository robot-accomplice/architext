// Pure model for the Repo Tree view: builds a folder tree from a flat file
// list and resolves which architecture node "owns" a path (via node.sourcePaths)
// so the UI can colour files/folders by C4 type or by Flow responsibility.

// Build a nested tree from flat repo-relative paths.
// Each node: { name, path, type: "dir"|"file", children?: [] }. Dirs first, then
// files, both alphabetical — a stable, file-explorer-like order.
export function buildRepoTree(files) {
  const root = { name: "", path: "", type: "dir", children: [] };
  const dirIndex = new Map([["", root]]);

  const ensureDir = (dirPath) => {
    if (dirIndex.has(dirPath)) return dirIndex.get(dirPath);
    const slash = dirPath.lastIndexOf("/");
    const parentPath = slash === -1 ? "" : dirPath.slice(0, slash);
    const name = slash === -1 ? dirPath : dirPath.slice(slash + 1);
    const parent = ensureDir(parentPath);
    const dir = { name, path: dirPath, type: "dir", children: [] };
    parent.children.push(dir);
    dirIndex.set(dirPath, dir);
    return dir;
  };

  for (const file of files) {
    if (!file) continue;
    const slash = file.lastIndexOf("/");
    const dir = ensureDir(slash === -1 ? "" : file.slice(0, slash));
    dir.children.push({ name: slash === -1 ? file : file.slice(slash + 1), path: file, type: "file" });
  }

  const sortChildren = (node) => {
    if (node.type !== "dir") return;
    node.children.sort((a, b) => (a.type === b.type ? a.name.localeCompare(b.name) : a.type === "dir" ? -1 : 1));
    node.children.forEach(sortChildren);
  };
  sortChildren(root);
  return root;
}

// Index node.sourcePaths so a repo path resolves to its owning node by the
// LONGEST matching source path (exact file, or a directory prefix). Longest
// match wins so a file-level mapping beats a coarser folder-level one.
export function buildOwnerIndex(nodes) {
  const entries = [];
  for (const node of nodes ?? []) {
    for (const raw of node.sourcePaths ?? []) {
      const prefix = String(raw).replace(/\/+$/, "");
      if (prefix) entries.push({ prefix, node });
    }
  }
  return entries;
}

export function resolveOwner(path, ownerIndex) {
  let best = null;
  let bestLen = -1;
  for (const entry of ownerIndex) {
    const isMatch = path === entry.prefix || path.startsWith(`${entry.prefix}/`);
    if (isMatch && entry.prefix.length > bestLen) {
      best = entry.node;
      bestLen = entry.prefix.length;
    }
  }
  return best;
}

// The dominant owner of a directory subtree (for folder colouring): the most
// common owning node id among the files beneath it, or null when none/mixed
// without a clear majority is still returned as the top node for a hint.
export function dominantOwner(dirNode, ownerIndex) {
  const counts = new Map();
  const byId = new Map();
  const visit = (node) => {
    if (node.type === "file") {
      const owner = resolveOwner(node.path, ownerIndex);
      if (owner) {
        counts.set(owner.id, (counts.get(owner.id) ?? 0) + 1);
        byId.set(owner.id, owner);
      }
    } else {
      node.children.forEach(visit);
    }
  };
  visit(dirNode);
  let bestId = null;
  let bestCount = 0;
  let distinct = 0;
  for (const [id, count] of counts) {
    distinct += 1;
    if (count > bestCount) { bestCount = count; bestId = id; }
  }
  return { owner: bestId ? byId.get(bestId) : null, mixed: distinct > 1 };
}
