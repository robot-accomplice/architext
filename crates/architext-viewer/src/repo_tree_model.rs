//! Pure model for the Repo Tree surface: fold a flat file list into a nested
//! directory tree, and resolve which architecture node "owns" a repo path via
//! `node.sourcePaths`.
//!
//! Faithful port of `viewer/src/presentation/repoTreeModel.js`
//! (`buildRepoTree`, `buildOwnerIndex`, `resolveOwner`). Leptos-free and
//! native-testable. Owner color is NOT decided here — it is single-sourced from
//! `diagram::role_color_var` at the call site, keyed by the owning node's type.

use crate::data::models::Node;

/// One node in the repo tree: a directory (with sorted children) or a file
/// (carrying size/mtime for the metadata columns).
#[derive(Debug, Clone, PartialEq)]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    pub kind: TreeKind,
    pub size: Option<u64>,
    pub mtime: Option<i64>,
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeKind {
    Dir,
    File,
}

/// A flat file entry from `/api/repo-tree`'s `files[]`.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub size: Option<u64>,
    pub mtime: Option<i64>,
}

impl TreeNode {
    fn dir(name: &str, path: &str) -> Self {
        TreeNode {
            name: name.to_string(),
            path: path.to_string(),
            kind: TreeKind::Dir,
            size: None,
            mtime: None,
            children: Vec::new(),
        }
    }
}

/// Build a nested tree from a flat file list. Directories are interned as they
/// are first seen; children are sorted dirs-first then by name (a stable,
/// file-explorer-like order). Port of JS `buildRepoTree`.
pub fn build_repo_tree(files: &[FileEntry]) -> TreeNode {
    let mut root = TreeNode::dir("", "");

    for entry in files {
        if entry.path.is_empty() {
            continue;
        }
        let (dir_path, file_name) = match entry.path.rfind('/') {
            Some(i) => (&entry.path[..i], &entry.path[i + 1..]),
            None => ("", entry.path.as_str()),
        };
        let dir = ensure_dir(&mut root, dir_path);
        dir.children.push(TreeNode {
            name: file_name.to_string(),
            path: entry.path.clone(),
            kind: TreeKind::File,
            size: entry.size,
            mtime: entry.mtime,
            children: Vec::new(),
        });
    }

    sort_children(&mut root);
    root
}

/// Walk/create the directory chain for `dir_path` (relative to root) and return
/// a mutable reference to the leaf directory. `""` is the root itself.
fn ensure_dir<'a>(root: &'a mut TreeNode, dir_path: &str) -> &'a mut TreeNode {
    if dir_path.is_empty() {
        return root;
    }
    let mut cursor = root;
    let mut built = String::new();
    for segment in dir_path.split('/') {
        if !built.is_empty() {
            built.push('/');
        }
        built.push_str(segment);
        // Find-or-create the child dir with this accumulated path.
        let pos = cursor
            .children
            .iter()
            .position(|c| c.kind == TreeKind::Dir && c.path == built);
        let idx = match pos {
            Some(i) => i,
            None => {
                cursor.children.push(TreeNode::dir(segment, &built));
                cursor.children.len() - 1
            }
        };
        cursor = &mut cursor.children[idx];
    }
    cursor
}

/// Sort a directory's children dirs-first then by name, recursively. Port of the
/// JS `sortChildren` comparator (`localeCompare` on name within a kind).
fn sort_children(node: &mut TreeNode) {
    if node.kind != TreeKind::Dir {
        return;
    }
    node.children.sort_by(|a, b| match (a.kind, b.kind) {
        (TreeKind::Dir, TreeKind::File) => std::cmp::Ordering::Less,
        (TreeKind::File, TreeKind::Dir) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });
    for child in &mut node.children {
        sort_children(child);
    }
}

/// One `(prefix, node-index)` entry of the owner index. `prefix` is a
/// `sourcePaths` entry with trailing slashes trimmed.
#[derive(Debug, Clone)]
pub struct OwnerEntry {
    pub prefix: String,
    pub node_idx: usize,
}

/// Index `node.sourcePaths` so a repo path can resolve to its owning node.
/// Port of JS `buildOwnerIndex`. `node_idx` indexes into the `nodes` slice.
pub fn build_owner_index(nodes: &[Node]) -> Vec<OwnerEntry> {
    let mut entries = Vec::new();
    for (idx, node) in nodes.iter().enumerate() {
        for raw in &node.source_paths {
            let prefix = raw.trim_end_matches('/');
            if !prefix.is_empty() {
                entries.push(OwnerEntry { prefix: prefix.to_string(), node_idx: idx });
            }
        }
    }
    entries
}

/// Resolve a repo path to its owning node index by the LONGEST matching source
/// path (exact file or directory prefix). Longest match wins so a file-level
/// mapping beats a coarser folder-level one. Port of JS `resolveOwner`.
pub fn resolve_owner(path: &str, owner_index: &[OwnerEntry]) -> Option<usize> {
    let mut best: Option<usize> = None;
    let mut best_len: isize = -1;
    for entry in owner_index {
        let is_match =
            path == entry.prefix || path.starts_with(&format!("{}/", entry.prefix));
        if is_match && entry.prefix.len() as isize > best_len {
            best = Some(entry.node_idx);
            best_len = entry.prefix.len() as isize;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, node_type: &str, source_paths: &[&str]) -> Node {
        Node {
            id: id.to_string(),
            node_type: node_type.to_string(),
            name: id.to_string(),
            summary: None,
            owner: None,
            dependencies: Vec::new(),
            source_paths: source_paths.iter().map(|s| s.to_string()).collect(),
            related_flows: Vec::new(),
            related_decisions: Vec::new(),
            known_risks: Vec::new(),
            data_handled: Vec::new(),
        }
    }

    fn file(path: &str) -> FileEntry {
        FileEntry { path: path.to_string(), size: Some(1), mtime: Some(0) }
    }

    #[test]
    fn build_repo_tree_folds_flat_paths_into_a_sorted_nested_tree() {
        let files = vec![
            file("src/main.rs"),
            file("src/lib.rs"),
            file("README.md"),
            file("src/data/fetch.rs"),
        ];
        let tree = build_repo_tree(&files);

        // Root children: the `src` dir sorts before the `README.md` file.
        assert_eq!(tree.children.len(), 2);
        assert_eq!(tree.children[0].name, "src");
        assert_eq!(tree.children[0].kind, TreeKind::Dir);
        assert_eq!(tree.children[1].name, "README.md");
        assert_eq!(tree.children[1].kind, TreeKind::File);

        // Inside `src`: the `data` dir first, then files alphabetical.
        let src = &tree.children[0];
        assert_eq!(src.children[0].name, "data");
        assert_eq!(src.children[0].kind, TreeKind::Dir);
        assert_eq!(src.children[1].name, "lib.rs");
        assert_eq!(src.children[2].name, "main.rs");

        // Nested dir carries its full path; its file is reachable.
        let data = &src.children[0];
        assert_eq!(data.path, "src/data");
        assert_eq!(data.children[0].path, "src/data/fetch.rs");
        assert_eq!(data.children[0].kind, TreeKind::File);
    }

    #[test]
    fn resolve_owner_picks_the_longest_matching_source_path() {
        let nodes = vec![
            node("broad", "service", &["src"]),
            node("specific", "data-store", &["src/data"]),
            node("file-exact", "module", &["src/data/fetch.rs"]),
        ];
        let idx = build_owner_index(&nodes);

        // Exact file match beats both the dir-prefix and the broad-prefix owners.
        assert_eq!(resolve_owner("src/data/fetch.rs", &idx), Some(2));
        // A different file under src/data resolves to the dir-level owner.
        assert_eq!(resolve_owner("src/data/models.rs", &idx), Some(1));
        // A file only under src resolves to the broad owner.
        assert_eq!(resolve_owner("src/main.rs", &idx), Some(0));
        // Unmapped path → no owner.
        assert_eq!(resolve_owner("docs/readme.md", &idx), None);
    }

    #[test]
    fn owner_index_trims_trailing_slashes_and_skips_empties() {
        let nodes = vec![node("n", "service", &["src/", "", "  "])];
        let idx = build_owner_index(&nodes);
        // Trailing slash trimmed; empty entry skipped (the "  " is non-empty).
        let prefixes: Vec<&str> = idx.iter().map(|e| e.prefix.as_str()).collect();
        assert!(prefixes.contains(&"src"));
        assert!(!prefixes.contains(&""));
    }
}
