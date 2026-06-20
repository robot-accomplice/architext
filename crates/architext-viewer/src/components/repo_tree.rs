//! Repo Tree surface (DISPLAY only).
//!
//! Fetches the live file list from `/api/repo-tree`, folds it into a nested
//! directory tree (`repo_tree_model::build_repo_tree`), and renders expand/
//! collapse rows in the central canvas region. Each row carries a left accent
//! rail in its OWNING node's `--c4-{type}` role color — files via
//! `repo_tree_model::resolve_owner` (longest `sourcePaths` prefix), directories
//! via `repo_tree_model::dominant_owner` (majority owner; "mixed" when several)
//! — colored by the single-source `diagram::role_color_var`. Files also show a
//! type icon (tinted `DiagramIcon` glyph), a size column, and a relative-time
//! "modified" column; directories show a folder glyph. A column header labels
//! the columns and a summary line reports the file count + source. Clicking an
//! owned row selects its owning node (the inspector then shows that component).
//!
//! This restores parity with the React `RepoTreeWorkspace`'s data richness.
//! Editing/mutation is out of scope (V5); this is read-only.

use std::collections::HashSet;

use leptos::*;
use leptos::spawn_local;

use crate::data::{
    fetch_file, fetch_repo_tree, models::FilePreviewPayload, models::RepoFile, FetchError,
};
use crate::diagram::role_color_var;
use crate::repo_tree_model::{
    build_owner_index, build_repo_tree, dominant_owner, file_icon, filter_files,
    format_relative_time, format_size, resolve_owner, FileEntry, TreeKind, TreeNode,
};
use crate::state::use_app_state;

const INDENT_PX: f64 = 14.0; // per-depth indent (matches the JS workspace)

/// The repo-tree fetch outcome held in a plain signal (no nested `<Suspense>`,
/// which conflicts with the App-level data Suspense). `None` while loading.
type RepoState = Option<Result<crate::data::models::RepoTreePayload, FetchError>>;

/// The file-preview fetch outcome for the right pane. `None` while no file is
/// selected OR while the selected file's contents are still loading — the pane
/// distinguishes the two via `selected_file` being set (loading) vs unset
/// (empty hint).
type PreviewState = Option<Result<FilePreviewPayload, FetchError>>;

/// The SVG `path` `d` for a `DiagramIcon` glyph key used by the repo tree
/// (file-type glyphs + folder glyphs). Mirrors the React `DiagramIcon` paths
/// verbatim so the two viewers draw identical shapes. Unknown keys fall back to
/// the generic file glyph.
fn glyph_path(key: &str) -> &'static str {
    match key {
        "braces" => "M9 4c-2 0-2 2-2 3.5C7 9 6.5 11 5 12c1.5 1 2 3 2 4.5C7 18 7 20 9 20 M15 4c2 0 2 2 2 3.5 0 1.5.5 3.5 2 4.5-1.5 1-2 3-2 4.5 0 1.5 0 3.5-2 3.5",
        "code" => "M9 8l-4 4 4 4 M15 8l4 4-4 4",
        "gear" => "M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6 M12 3v2.5 M12 18.5V21 M21 12h-2.5 M5.5 12H3 M18.4 5.6l-1.8 1.8 M7.4 16.6l-1.8 1.8 M18.4 18.4l-1.8-1.8 M7.4 7.4 5.6 5.6",
        "hash" => "M6 9h12 M5 15h12 M10 5l-2 14 M17 5l-2 14",
        "image" => "M4 6h16v12H4z M4 15l4-4 3 3 4-4 5 5 M9 10a1.4 1.4 0 1 0 0 .01",
        "lock" => "M6 11h12v9H6z M9 11V8a3 3 0 0 1 6 0v3",
        "markdown" => "M3 8h18v8H3z M6 14v-4l2.5 2.5L11 10v4 M15 10v3 M13 12l2 2 2-2",
        "database" => "M6 6c0-2 12-2 12 0v12c0 2-12 2-12 0z M6 6c0 2 12 2 12 0 M6 12c0 2 12 2 12 0",
        "package" => "M4 8l8-4 8 4v8l-8 4-8-4z M4 8l8 4 8-4 M12 12v8",
        "folder" => "M4 18h16V8h-9l-2-2H4z",
        "folder-open" => "M3 8V6h6l2 2h8v2 M3 8h18l-2 10H5z",
        _ => "M7 3h7l4 4v14H7z M14 3v4h4",
    }
}

#[component]
pub fn RepoTree() -> impl IntoView {
    let state = use_app_state();

    // Fetch the file list once on mount via spawn_local into a plain signal.
    // The dataset (nodes) is already loaded in AppState.
    let repo = create_rw_signal::<RepoState>(None);
    spawn_local(async move {
        repo.set(Some(fetch_repo_tree().await));
    });

    // Collapsed directory paths (expanded by default).
    let collapsed = create_rw_signal::<HashSet<String>>(HashSet::new());

    // ── Signal-to-noise controls (UX review #7) ──────────────────────────────
    // Text filter over file paths; hide-noise (Playwright logs / dot-dirs) on by
    // default so the first screen shows real files; an optional owning-node id
    // filter driven by the owner chips.
    let query = create_rw_signal(String::new());
    let hide_noise = create_rw_signal(true);
    let owner_filter = create_rw_signal::<Option<String>>(None);

    // The currently-previewed file path (right pane). `None` = empty state.
    let selected_file = create_rw_signal::<Option<String>>(None);
    // The preview fetch outcome. `None` while loading the selected file.
    let preview = create_rw_signal::<PreviewState>(None);

    // When `selected_file` changes to a path, fetch its highlighted contents.
    // Each click clears the prior result first (loading state), then resolves.
    create_effect(move |_| {
        let Some(path) = selected_file.get() else { return };
        preview.set(None);
        spawn_local(async move {
            preview.set(Some(fetch_file(&path).await));
        });
    });

    view! {
        <div
            class="repo-tree"
            class=("repo-tree--previewing", move || selected_file.get().is_some())
        >
            <div class="repo-tree__main">
            <div class="repo-tree__header">
                <div class="overline">"REPO TREE"</div>
                {move || repo.get()
                    .and_then(|r| r.ok())
                    .map(|p| {
                        let count = p.files.len();
                        let source = p.source.clone();
                        view! {
                            <span class="repo-tree__summary">
                                <span class="repo-tree__count">{format!("{count} files")}</span>
                                {source.map(|s| view! {
                                    <span class="chip">{format!("source: {s}")}</span>
                                })}
                            </span>
                        }
                    })}
            </div>
            // ── Signal-to-noise controls ────────────────────────────────────
            {move || repo.get()
                .and_then(|r| r.ok())
                .filter(|p| !p.files.is_empty())
                .map(|_| view! {
                    <RepoTreeControls
                        query
                        hide_noise
                        owner_filter
                        collapsed
                        repo
                        state
                    />
                })}
            {move || match repo.get() {
                None => view! {
                    <p class="repo-tree__hint">"Loading repo file list…"</p>
                }.into_view(),
                Some(Err(err)) => view! {
                    <p class="repo-tree__hint repo-tree__hint--error">
                        {format!("Could not load repo tree: {err}")}
                    </p>
                }.into_view(),
                Some(Ok(payload)) => {
                    if payload.files.is_empty() {
                        return view! {
                            <p class="repo-tree__hint">
                                "No files found. Run "<code>"architext serve"</code>" inside a git repository."
                            </p>
                        }.into_view();
                    }
                    let files: Vec<FileEntry> = payload.files.iter()
                        .map(|f: &RepoFile| FileEntry {
                            path: f.path.clone(),
                            size: f.size,
                            mtime: f.mtime,
                        })
                        .collect();
                    // Apply the signal-to-noise filters BEFORE folding the tree,
                    // so build_repo_tree recreates only the ancestor dirs of the
                    // surviving files (matching files + ancestors, no orphans).
                    let data = state.data.get_untracked();
                    let owner_index = build_owner_index(&data.nodes);
                    let owner_idx = owner_filter.get().and_then(|id| {
                        data.nodes.iter().position(|n| n.id == id)
                    });
                    let kept = filter_files(
                        &files,
                        &query.get(),
                        owner_idx,
                        hide_noise.get(),
                        &owner_index,
                    );
                    if kept.is_empty() {
                        return view! {
                            <p class="repo-tree__hint">
                                "No files match the current filter."
                            </p>
                        }.into_view();
                    }
                    let kept_count = kept.len();
                    let total = files.len();
                    let tree = build_repo_tree(&kept);
                    // Single clock read for the whole render — matches the React
                    // `now = Date.now()` memo (stable across all rows).
                    let now = js_sys::Date::now() as i64;
                    view! {
                        <div class="repo-tree__body">
                            {(kept_count != total).then(|| view! {
                                <p class="repo-tree__filtered">
                                    {format!("Showing {kept_count} of {total} files")}
                                </p>
                            })}
                            <div class="repo-row repo-row--colhead" aria-hidden="true">
                                <div class="repo-row__lead">
                                    <span class="repo-row__caret"></span>
                                    <span class="repo-row__icon"></span>
                                    <span class="repo-row__name">"Name"</span>
                                </div>
                                <span class="repo-row__size">"Size"</span>
                                <span class="repo-row__time">"Modified"</span>
                                <span class="repo-row__owner">"Owner"</span>
                            </div>
                            {render_children(&tree, 0, state, collapsed, selected_file, now)}
                        </div>
                    }.into_view()
                }
            }}
            </div>
            <FilePreview selected_file preview/>
        </div>
    }
}

/// The controls bar: a path filter, collapse/expand-all, a "hide noise" toggle
/// (Playwright logs / dot-dirs, on by default), and owner filter chips. Owns
/// only the control signals; the body render reacts to them.
#[component]
fn RepoTreeControls(
    query: RwSignal<String>,
    hide_noise: RwSignal<bool>,
    owner_filter: RwSignal<Option<String>>,
    collapsed: RwSignal<HashSet<String>>,
    repo: RwSignal<RepoState>,
    state: crate::state::AppState,
) -> impl IntoView {
    // Collapse every directory present in the (filtered) repo; expand clears it.
    let collapse_all = move |_| {
        let Some(Ok(payload)) = repo.get_untracked() else { return };
        let data = state.data.get_untracked();
        let owner_index = build_owner_index(&data.nodes);
        let owner_idx = owner_filter
            .get_untracked()
            .and_then(|id| data.nodes.iter().position(|n| n.id == id));
        let files: Vec<FileEntry> = payload
            .files
            .iter()
            .map(|f| FileEntry { path: f.path.clone(), size: f.size, mtime: f.mtime })
            .collect();
        let kept = filter_files(
            &files,
            &query.get_untracked(),
            owner_idx,
            hide_noise.get_untracked(),
            &owner_index,
        );
        let tree = build_repo_tree(&kept);
        let mut dirs = HashSet::new();
        collect_dir_paths(&tree, &mut dirs);
        collapsed.set(dirs);
    };
    let expand_all = move |_| collapsed.set(HashSet::new());

    // Owner chips: the distinct nodes that own at least one repo file, so the
    // chip set is exactly the owners a user can filter by.
    let owner_chips = create_memo(move |_| {
        let Some(Ok(payload)) = repo.get() else { return Vec::new() };
        let data = state.data.get();
        let owner_index = build_owner_index(&data.nodes);
        let mut seen: HashSet<usize> = HashSet::new();
        let mut chips: Vec<(String, String, String)> = Vec::new();
        for f in &payload.files {
            if let Some(idx) = resolve_owner(&f.path, &owner_index) {
                if seen.insert(idx) {
                    if let Some(n) = data.nodes.get(idx) {
                        chips.push((n.id.clone(), n.name.clone(), n.node_type.clone()));
                    }
                }
            }
        }
        chips.sort_by(|a, b| a.1.cmp(&b.1));
        chips
    });

    view! {
        <div class="repo-tree__controls">
            <input
                class="blast-search repo-tree__filter"
                r#type="text"
                placeholder="Filter files…"
                prop:value=move || query.get()
                on:input=move |ev| query.set(event_target_value(&ev))
            />
            <div class="repo-tree__control-row">
                <button class="chip repo-tree__btn" on:click=collapse_all>"Collapse all"</button>
                <button class="chip repo-tree__btn" on:click=expand_all>"Expand all"</button>
                <button
                    class="chip repo-tree__btn"
                    class:is-active=move || hide_noise.get()
                    title="Hide .playwright-mcp logs and dot-directories"
                    on:click=move |_| hide_noise.update(|v| *v = !*v)
                >"Hide noise"</button>
            </div>
            {move || {
                let chips = owner_chips.get();
                (!chips.is_empty()).then(|| view! {
                    <div class="repo-tree__owners">
                        <span class="overline repo-tree__owners-label">"OWNER"</span>
                        {chips.into_iter().map(|(id, name, node_type)| {
                            let color = role_color_var(&node_type);
                            let chip_id = id.clone();
                            let is_active = create_memo(move |_| {
                                owner_filter.get().as_deref() == Some(chip_id.as_str())
                            });
                            let toggle_id = id.clone();
                            let on_click = move |_| {
                                owner_filter.update(|cur| {
                                    if cur.as_deref() == Some(toggle_id.as_str()) {
                                        *cur = None;
                                    } else {
                                        *cur = Some(toggle_id.clone());
                                    }
                                });
                            };
                            view! {
                                <button
                                    class="chip repo-tree__owner-chip"
                                    class:is-active=move || is_active.get()
                                    style=format!("--owner-color:{color}")
                                    on:click=on_click
                                >{name}</button>
                            }
                        }).collect_view()}
                    </div>
                })
            }}
        </div>
    }
}

/// Collect every directory path in a (sub)tree into `out` (for collapse-all).
fn collect_dir_paths(node: &TreeNode, out: &mut HashSet<String>) {
    for child in &node.children {
        if child.kind == TreeKind::Dir {
            out.insert(child.path.clone());
            collect_dir_paths(child, out);
        }
    }
}

/// The right pane: renders the selected file's syntax-highlighted contents, or
/// an empty hint when nothing is selected. Loading and error states are
/// explicit (FAIL LOUD — never a blank pane). Binary/truncated files carry a
/// clear notice.
#[component]
fn FilePreview(
    selected_file: RwSignal<Option<String>>,
    preview: RwSignal<PreviewState>,
) -> impl IntoView {
    view! {
        <div class="repo-file-preview">
            {move || match (selected_file.get(), preview.get()) {
                // Nothing selected yet → empty hint.
                (None, _) => view! {
                    <p class="repo-file-preview__hint">"Select a file to preview"</p>
                }.into_view(),
                // Selected but not yet resolved → loading.
                (Some(path), None) => view! {
                    <div class="repo-file-preview__header">
                        <span class="repo-file-preview__path">{path}</span>
                    </div>
                    <p class="repo-file-preview__hint">"Loading file…"</p>
                }.into_view(),
                // Fetch failed → explicit error (FAIL LOUD).
                (Some(path), Some(Err(err))) => view! {
                    <div class="repo-file-preview__header">
                        <span class="repo-file-preview__path">{path}</span>
                    </div>
                    <p class="repo-file-preview__hint repo-file-preview__hint--error">
                        {format!("Could not load file: {err}")}
                    </p>
                }.into_view(),
                // Resolved.
                (Some(_), Some(Ok(payload))) => {
                    let meta = preview_meta(&payload);
                    let notice = preview_notice(&payload);
                    let body = if payload.binary {
                        view! {
                            <p class="repo-file-preview__hint">
                                "Binary file — no text preview."
                            </p>
                        }.into_view()
                    } else {
                        // Server-rendered, inline-styled highlight HTML.
                        let html = payload.html.clone().unwrap_or_default();
                        view! {
                            <div class="repo-file-preview__code" inner_html=html></div>
                        }.into_view()
                    };
                    view! {
                        <div class="repo-file-preview__header">
                            <span class="repo-file-preview__path">{payload.path.clone()}</span>
                            <span class="repo-file-preview__meta">{meta}</span>
                        </div>
                        {notice}
                        {body}
                    }.into_view()
                }
            }}
        </div>
    }
}

/// The header metadata line: `size · language`.
fn preview_meta(p: &FilePreviewPayload) -> String {
    let size = if p.size.is_some() { format_size(p.size) } else { String::new() };
    match (&size, &p.language) {
        (s, Some(lang)) if !s.is_empty() => format!("{s} · {lang}"),
        (s, _) if !s.is_empty() => s.clone(),
        (_, Some(lang)) => lang.clone(),
        _ => String::new(),
    }
}

/// A truncation notice, shown above the code when the server only sent the head.
fn preview_notice(p: &FilePreviewPayload) -> View {
    if p.truncated && !p.binary {
        view! {
            <p class="repo-file-preview__notice">
                "Large file — showing the first 512 KiB."
            </p>
        }
        .into_view()
    } else {
        ().into_view()
    }
}

/// Render a directory's children at `depth`. Recursive; expanded dirs render
/// their children inline. Owner resolution reads `state.data` (nodes) untracked
/// — the dataset is immutable for the session.
fn render_children(
    parent: &TreeNode,
    depth: usize,
    state: crate::state::AppState,
    collapsed: RwSignal<HashSet<String>>,
    selected_file: RwSignal<Option<String>>,
    now: i64,
) -> View {
    parent
        .children
        .iter()
        .map(|child| render_node(child, depth, state, collapsed, selected_file, now))
        .collect_view()
}

fn render_node(
    node: &TreeNode,
    depth: usize,
    state: crate::state::AppState,
    collapsed: RwSignal<HashSet<String>>,
    selected_file: RwSignal<Option<String>>,
    now: i64,
) -> View {
    let indent = format!("padding-left:{}px", depth as f64 * INDENT_PX);
    let data = state.data.get_untracked();
    let owner_index = build_owner_index(&data.nodes);

    match node.kind {
        TreeKind::Dir => {
            let path = node.path.clone();
            let name = node.name.clone();
            let toggle_path = path.clone();
            let is_collapsed = create_memo(move |_| collapsed.get().contains(&toggle_path));
            let on_click = move |_| {
                collapsed.update(|set| {
                    if !set.remove(&path) {
                        set.insert(path.clone());
                    }
                });
            };

            // Dominant-owner color for the dir rail + label ("mixed" when the
            // subtree spans more than one owner). Port of JS `dominantOwner`.
            let (owner_idx, mixed) = dominant_owner(node, &owner_index);
            let owner = owner_idx.and_then(|i| data.nodes.get(i).cloned());
            let (rail, owner_view) = if mixed {
                (
                    "var(--line)".to_string(),
                    view! { <span class="repo-row__owner repo-row__owner--muted">"mixed"</span> }
                        .into_view(),
                )
            } else if let Some(n) = &owner {
                let color = role_color_var(&n.node_type);
                (
                    color.clone(),
                    view! {
                        <span class="repo-row__owner" style=format!("color:{color}")>
                            {n.name.clone()}
                        </span>
                    }
                    .into_view(),
                )
            } else {
                (
                    "var(--line)".to_string(),
                    view! { <span class="repo-row__owner"></span> }.into_view(),
                )
            };
            let style = format!("border-left-color:{rail}");

            // Children are rendered eagerly but hidden when collapsed, so the
            // expand toggle is instant and selection state is preserved.
            let children_view =
                render_children(node, depth + 1, state, collapsed, selected_file, now);
            view! {
                <div class="repo-row repo-row--dir" style=style on:click=on_click>
                    <div class="repo-row__lead" style=indent>
                        <span class="repo-row__caret">
                            {move || if is_collapsed.get() { "▸" } else { "▾" }}
                        </span>
                        <span class="repo-row__icon repo-row__icon--folder">
                            <svg class="repo-glyph" viewBox="0 0 24 24" aria-hidden="true">
                                <path d=move || if is_collapsed.get() {
                                    glyph_path("folder")
                                } else {
                                    glyph_path("folder-open")
                                }/>
                            </svg>
                        </span>
                        <span class="repo-row__name repo-row__name--dir">{name}</span>
                    </div>
                    <span class="repo-row__size"></span>
                    <span class="repo-row__time"></span>
                    {owner_view}
                </div>
                <div class="repo-dir-children" class=("repo-dir-children--hidden", move || is_collapsed.get())>
                    {children_view}
                </div>
            }
            .into_view()
        }
        TreeKind::File => {
            let name = node.name.clone();
            let path = node.path.clone();

            // Owner resolution + single-source role color for the rail.
            let owner = resolve_owner(&path, &owner_index).and_then(|i| data.nodes.get(i).cloned());

            let (rail, owner_label, owner_id) = match &owner {
                Some(n) => (role_color_var(&n.node_type), n.name.clone(), Some(n.id.clone())),
                // Unowned files get a neutral rail (the hairline), never a role hue.
                None => ("var(--line)".to_string(), String::new(), None),
            };
            let style = format!("border-left-color:{rail}");

            // Clicking a file row keeps the existing behaviour (select the
            // owning node, if any) AND sets the preview path so the right pane
            // fetches + renders the file's contents.
            let click_path = path.clone();
            let on_click = move |_| {
                if let Some(id) = owner_id.clone() {
                    state.set_selected_node(id);
                }
                selected_file.set(Some(click_path.clone()));
            };
            let owned = owner.is_some();

            let icon = file_icon(&name);
            let size_text = format_size(node.size);
            let time_text = format_relative_time(node.mtime, now);

            view! {
                <div
                    class="repo-row repo-row--file"
                    class=("repo-row--owned", owned)
                    style=style
                    on:click=on_click
                >
                    <div class="repo-row__lead" style=indent>
                        <span class="repo-row__caret"></span>
                        <span class="repo-row__icon" style=format!("color:{}", icon.color)>
                            <svg class="repo-glyph" viewBox="0 0 24 24" aria-hidden="true">
                                <path d=glyph_path(icon.glyph)/>
                            </svg>
                        </span>
                        <span class="repo-row__name">{name}</span>
                    </div>
                    <span class="repo-row__size">{size_text}</span>
                    <span class="repo-row__time">{time_text}</span>
                    {if owner_label.is_empty() {
                        view! { <span class="repo-row__owner"></span> }.into_view()
                    } else {
                        view! {
                            <span class="repo-row__owner" style=format!("color:{rail}")>
                                {owner_label}
                            </span>
                        }.into_view()
                    }}
                </div>
            }
            .into_view()
        }
    }
}
