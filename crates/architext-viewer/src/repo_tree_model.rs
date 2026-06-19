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

/// Classify a repo path as "noise" — high-volume tooling artifacts that bury
/// real source files (the `.playwright-mcp/console-*.log` spam) and any path
/// with a dot-prefixed directory segment (`.git/`, `.cache/`, …). Dotfiles at
/// the leaf (e.g. `.gitignore`) are NOT noise — only dot-prefixed *directory*
/// segments and the dedicated tooling dirs are. The "hide noise" toggle filters
/// these by default so the first screen shows real files.
pub fn is_noise_path(path: &str) -> bool {
    let mut segments: Vec<&str> = path.split('/').collect();
    // The last segment is the file name; a leading-dot file name is a dotfile,
    // not a noise *directory*, so only directory segments are inspected.
    segments.pop();
    segments
        .iter()
        .any(|seg| seg.starts_with('.') && !seg.is_empty())
}

/// Narrow a flat file list for rendering. Filtering happens at the file level
/// *before* `build_repo_tree`, so the surviving files' ancestor directories are
/// recreated automatically — keeping matching files plus their ancestor dirs
/// (the spec's filter behaviour) without any tree pruning. A file survives when:
/// it passes the text query (substring on its path, case-insensitive), it is not
/// hidden by the noise filter, and — when an owner is selected — it resolves to
/// that owning node index.
pub fn filter_files(
    files: &[FileEntry],
    query: &str,
    owner_filter: Option<usize>,
    hide_noise: bool,
    owner_index: &[OwnerEntry],
) -> Vec<FileEntry> {
    let q = query.trim().to_lowercase();
    files
        .iter()
        .filter(|f| {
            if hide_noise && is_noise_path(&f.path) {
                return false;
            }
            if !q.is_empty() && !f.path.to_lowercase().contains(&q) {
                return false;
            }
            if let Some(want) = owner_filter {
                if resolve_owner(&f.path, owner_index) != Some(want) {
                    return false;
                }
            }
            true
        })
        .cloned()
        .collect()
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

/// The dominant owner of a directory subtree (for folder colouring): the most
/// common owning node index among the files beneath it. `mixed` is true when
/// more than one distinct owner appears, so the UI shows a "mixed" hint instead
/// of a single owner color. Port of JS `dominantOwner`.
pub fn dominant_owner(dir: &TreeNode, owner_index: &[OwnerEntry]) -> (Option<usize>, bool) {
    use std::collections::HashMap;
    let mut counts: HashMap<usize, usize> = HashMap::new();
    visit_files(dir, owner_index, &mut counts);
    let distinct = counts.len();
    // Tie-break on the smallest node index so the result is deterministic
    // (JS relies on Map insertion order; index order is the stable analogue).
    let best = counts
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
        .map(|(idx, _)| *idx);
    (best, distinct > 1)
}

fn visit_files(
    node: &TreeNode,
    owner_index: &[OwnerEntry],
    counts: &mut std::collections::HashMap<usize, usize>,
) {
    match node.kind {
        TreeKind::File => {
            if let Some(idx) = resolve_owner(&node.path, owner_index) {
                *counts.entry(idx).or_insert(0) += 1;
            }
        }
        TreeKind::Dir => {
            for child in &node.children {
                visit_files(child, owner_index, counts);
            }
        }
    }
}

// ─── File-type icon + metadata formatting ──────────────────────────────────
//
// Faithful port of `viewer/src/presentation/repoTreeFormat.js`. The Leptos
// viewer renders inline `DiagramIcon` line glyphs (it does not vendor the
// React build's brand SVGs), so the brand-extension families collapse onto the
// closest stroke glyph in the shared `DiagramIcon` vocabulary, tinted per
// technology family — consistent with `node_icon`/`mode_icon`.

/// A file's icon: a `DiagramIcon` glyph key + a tint color literal. The tint is
/// a brand hue, not a `--c4-*` role token, so it never collides with the
/// owner-rail role color (DESIGN.md: role hue encodes node TYPE only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIcon {
    pub glyph: &'static str,
    pub color: &'static str,
}

fn extension_of(name: &str) -> String {
    match name.rfind('.') {
        // A leading dot (".gitignore") is a dotfile, not an extension.
        Some(dot) if dot > 0 => name[dot + 1..].to_lowercase(),
        _ => String::new(),
    }
}

/// Resolve the icon (glyph + tint) for a file name. Port of `fileIconSpec`,
/// mapping brand extensions onto a representative stroke glyph + brand hue
/// since the Leptos viewer is glyph-only.
pub fn file_icon(name: &str) -> FileIcon {
    let lower = name.to_lowercase();
    // Special filenames that carry meaning without a useful extension.
    match lower.as_str() {
        "dockerfile" => return FileIcon { glyph: "package", color: "#2496ed" },
        ".gitignore" | ".env" => return FileIcon { glyph: "gear", color: "#9aa0a6" },
        _ => {}
    }
    let ext = extension_of(name);
    let icon = |glyph, color| FileIcon { glyph, color };
    match ext.as_str() {
        // Brand families → representative glyph + brand hue.
        "ts" | "mts" | "cts" => icon("braces", "#3178c6"),
        "tsx" | "jsx" => icon("braces", "#61dafb"),
        "js" | "mjs" | "cjs" => icon("braces", "#f1e05a"),
        "html" | "htm" => icon("code", "#e34c26"),
        "css" => icon("hash", "#563d7c"),
        "scss" | "sass" => icon("hash", "#c76395"),
        "py" => icon("code", "#3572a5"),
        "go" => icon("code", "#00add8"),
        "rs" => icon("code", "#dea584"),
        "java" | "kt" => icon("code", "#b07219"),
        "c" | "h" => icon("code", "#555555"),
        "cpp" | "cc" | "cxx" | "hpp" => icon("code", "#f34b7d"),
        "cs" => icon("code", "#178600"),
        "php" => icon("code", "#4f5d95"),
        "rb" => icon("code", "#701516"),
        "swift" => icon("code", "#f05138"),
        "sh" | "bash" | "zsh" | "fish" => icon("code", "#89e051"),
        "vue" => icon("code", "#41b883"),
        "graphql" | "gql" => icon("hash", "#e10098"),
        // Tinted-glyph families (already glyph-only in React).
        "json" | "jsonc" => icon("braces", "#e3b341"),
        "yml" | "yaml" => icon("hash", "#e0654f"),
        "md" | "mdx" => icon("markdown", "#58a6ff"),
        "xml" => icon("code", "#8bc34a"),
        "less" => icon("hash", "#5a8fd6"),
        "toml" | "ini" | "env" | "conf" | "cfg" => icon("gear", "#9aa0a6"),
        "sql" => icon("database", "#e38c00"),
        "lock" => icon("lock", "#b08d57"),
        "svg" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" => icon("image", "#d97757"),
        "txt" | "log" | "csv" | "rst" => icon("file", "#9aa0a6"),
        // Generic.
        _ => icon("file", "#7d8590"),
    }
}

/// Human-readable byte size. `None` renders as "". Port of `formatSize`.
pub fn format_size(bytes: Option<u64>) -> String {
    let bytes = match bytes {
        Some(b) => b,
        None => return String::new(),
    };
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    let mut value = bytes as f64 / 1024.0;
    let mut unit_index = 0;
    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }
    if value < 10.0 {
        format!("{:.1} {}", value, UNITS[unit_index])
    } else {
        format!(
            "{} {}",
            architext_routing::js_compat::js_round(value) as i64,
            UNITS[unit_index]
        )
    }
}

const MINUTE_MS: i64 = 60 * 1000;
const HOUR_MS: i64 = 60 * MINUTE_MS;
const DAY_MS: i64 = 24 * HOUR_MS;

/// Coarse relative time for the Modified column ("today", "5d", "3mo", "1y").
/// UX review #7: minute/hour granularity (`1m`/`2m`) is low-value churn noise,
/// so anything under a day collapses to "today"; days and above stay. `now` is
/// injected for testability. `None` mtime renders as "".
pub fn format_relative_time(mtime: Option<i64>, now: i64) -> String {
    let mtime = match mtime {
        Some(m) => m,
        None => return String::new(),
    };
    let delta = (now - mtime).max(0);
    if delta < DAY_MS {
        return "today".to_string();
    }
    let days = delta / DAY_MS;
    if days < 30 {
        format!("{days}d")
    } else if days < 365 {
        format!("{}mo", days / 30)
    } else {
        format!("{}y", days / 365)
    }
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

    #[test]
    fn dominant_owner_picks_majority_and_flags_mixed() {
        // src → node 0; src/data → node 1. Tree: src has main.rs (0), lib.rs (0),
        // and data/fetch.rs (1) → owner 0 dominates, but two distinct owners → mixed.
        let nodes = vec![
            node("svc", "service", &["src"]),
            node("store", "data-store", &["src/data"]),
        ];
        let idx = build_owner_index(&nodes);
        let tree = build_repo_tree(&[
            file("src/main.rs"),
            file("src/lib.rs"),
            file("src/data/fetch.rs"),
        ]);
        let src = &tree.children[0];
        let (owner, mixed) = dominant_owner(src, &idx);
        assert_eq!(owner, Some(0), "service owns the majority of files under src");
        assert!(mixed, "two distinct owners under src → mixed");

        // A subtree with a single owner is not mixed.
        let data = &src.children[0];
        let (owner, mixed) = dominant_owner(data, &idx);
        assert_eq!(owner, Some(1));
        assert!(!mixed);

        // No owner anywhere → None, not mixed.
        let empty = build_repo_tree(&[file("docs/readme.md")]);
        let (owner, mixed) = dominant_owner(&empty.children[0], &idx);
        assert_eq!(owner, None);
        assert!(!mixed);
    }

    #[test]
    fn file_icon_maps_brands_glyphs_and_specials() {
        // Brand extension → representative glyph + brand hue.
        assert_eq!(file_icon("main.rs"), FileIcon { glyph: "code", color: "#dea584" });
        assert_eq!(file_icon("app.tsx"), FileIcon { glyph: "braces", color: "#61dafb" });
        // Tinted-glyph families.
        assert_eq!(file_icon("data.json"), FileIcon { glyph: "braces", color: "#e3b341" });
        assert_eq!(file_icon("README.md"), FileIcon { glyph: "markdown", color: "#58a6ff" });
        assert_eq!(file_icon("Cargo.lock"), FileIcon { glyph: "lock", color: "#b08d57" });
        // Special filenames (no usable extension).
        assert_eq!(file_icon("Dockerfile"), FileIcon { glyph: "package", color: "#2496ed" });
        assert_eq!(file_icon(".gitignore"), FileIcon { glyph: "gear", color: "#9aa0a6" });
        // Unknown / extensionless → generic file glyph.
        assert_eq!(file_icon("LICENSE"), FileIcon { glyph: "file", color: "#7d8590" });
    }

    #[test]
    fn format_size_matches_js_thresholds() {
        assert_eq!(format_size(None), "");
        assert_eq!(format_size(Some(0)), "0 B");
        assert_eq!(format_size(Some(512)), "512 B");
        assert_eq!(format_size(Some(1024)), "1.0 KB");
        assert_eq!(format_size(Some(1536)), "1.5 KB");
        // ≥ 10 in a unit drops the decimal and rounds (Math.round).
        assert_eq!(format_size(Some(10 * 1024)), "10 KB");
        assert_eq!(format_size(Some(1024 * 1024)), "1.0 MB");
    }

    #[test]
    fn format_relative_time_coarsens_subday_to_today() {
        let now = 1_000_000_000_000_i64;
        assert_eq!(format_relative_time(None, now), "");
        // UX #7: minute/hour churn collapses to "today" — no more 1m/2m/3h noise.
        assert_eq!(format_relative_time(Some(now), now), "today");
        assert_eq!(format_relative_time(Some(now - 5 * MINUTE_MS), now), "today");
        assert_eq!(format_relative_time(Some(now - 3 * HOUR_MS), now), "today");
        // Days and coarser stay as before.
        assert_eq!(format_relative_time(Some(now - 5 * DAY_MS), now), "5d");
        assert_eq!(format_relative_time(Some(now - 40 * DAY_MS), now), "1mo");
        assert_eq!(format_relative_time(Some(now - 400 * DAY_MS), now), "1y");
        // Future mtime clamps to "today" (delta floored at 0).
        assert_eq!(format_relative_time(Some(now + DAY_MS), now), "today");
    }

    #[test]
    fn is_noise_path_flags_tooling_and_dot_dirs_not_dotfiles() {
        // The Playwright MCP log spam that buries the first screen.
        assert!(is_noise_path(".playwright-mcp/console-1.log"));
        // Any dot-prefixed directory segment, at any depth.
        assert!(is_noise_path(".git/config"));
        assert!(is_noise_path("src/.cache/blob"));
        // A leaf dotfile is NOT noise — it is a real file under a real dir.
        assert!(!is_noise_path(".gitignore"));
        assert!(!is_noise_path("src/main.rs"));
        assert!(!is_noise_path("README.md"));
    }

    #[test]
    fn filter_files_narrows_by_query_noise_and_owner() {
        let nodes = vec![node("svc", "service", &["src"])];
        let idx = build_owner_index(&nodes);
        let files = vec![
            file("src/main.rs"),
            file("src/lib.rs"),
            file("docs/readme.md"),
            file(".playwright-mcp/console-1.log"),
        ];

        // Default (hide noise, no query, no owner) drops the log spam only.
        let out = filter_files(&files, "", None, true, &idx);
        let paths: Vec<&str> = out.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["src/main.rs", "src/lib.rs", "docs/readme.md"]);

        // Text query is a case-insensitive path substring.
        let out = filter_files(&files, "MAIN", None, true, &idx);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].path, "src/main.rs");

        // Owner filter keeps only files resolving to that node.
        let out = filter_files(&files, "", Some(0), true, &idx);
        let paths: Vec<&str> = out.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["src/main.rs", "src/lib.rs"]);

        // Disabling the noise filter surfaces the spam again.
        let out = filter_files(&files, "", None, false, &idx);
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn filtered_files_rebuild_to_matching_files_plus_ancestor_dirs() {
        // The filter narrows the flat list; build_repo_tree then recreates only
        // the ancestor dirs of the survivors — no orphan/empty dirs remain.
        let files = vec![file("src/main.rs"), file("src/data/fetch.rs"), file("docs/x.md")];
        let kept = filter_files(&files, "fetch", None, true, &[]);
        let tree = build_repo_tree(&kept);
        // Only `src` survives at root (docs had no match); inside it only `data`.
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].name, "src");
        assert_eq!(tree.children[0].children.len(), 1);
        assert_eq!(tree.children[0].children[0].name, "data");
        assert_eq!(tree.children[0].children[0].children[0].path, "src/data/fetch.rs");
    }
}
