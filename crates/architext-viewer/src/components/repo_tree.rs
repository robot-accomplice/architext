//! Repo Tree surface (DISPLAY only).
//!
//! Fetches the live file list from `/api/repo-tree`, folds it into a nested
//! directory tree (`repo_tree_model::build_repo_tree`), and renders expand/
//! collapse rows in the central canvas region. Each file row carries a left
//! accent rail in its OWNING component's `--c4-{type}` role color — resolved by
//! `repo_tree_model::resolve_owner` (longest `sourcePaths` prefix) and colored
//! by the single-source `diagram::role_color_var`. Clicking an owned file
//! selects its owning node (the inspector then shows that component).
//!
//! Editing/mutation is out of scope (V5); this is read-only.

use std::collections::HashSet;

use leptos::*;
use leptos::spawn_local;

use crate::data::{fetch_repo_tree, models::RepoFile, FetchError};
use crate::diagram::role_color_var;
use crate::repo_tree_model::{
    build_owner_index, build_repo_tree, resolve_owner, FileEntry, TreeKind, TreeNode,
};
use crate::state::use_app_state;

const INDENT_PX: f64 = 14.0; // per-depth indent (matches the JS workspace)

/// The repo-tree fetch outcome held in a plain signal (no nested `<Suspense>`,
/// which conflicts with the App-level data Suspense). `None` while loading.
type RepoState = Option<Result<crate::data::models::RepoTreePayload, FetchError>>;

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

    view! {
        <div class="repo-tree">
            <div class="repo-tree__header">
                <div class="overline">"REPO TREE"</div>
                {move || repo.get()
                    .and_then(|r| r.ok())
                    .and_then(|p| p.source)
                    .map(|s| view! { <span class="chip">{format!("source: {s}")}</span> })}
            </div>
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
                    let files: Vec<FileEntry> = payload.files.iter()
                        .map(|f: &RepoFile| FileEntry {
                            path: f.path.clone(),
                            size: f.size,
                            mtime: f.mtime,
                        })
                        .collect();
                    let tree = build_repo_tree(&files);
                    view! {
                        <div class="repo-tree__body">
                            {render_children(&tree, 0, state, collapsed)}
                        </div>
                    }.into_view()
                }
            }}
        </div>
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
) -> View {
    parent
        .children
        .iter()
        .map(|child| render_node(child, depth, state, collapsed))
        .collect_view()
}

fn render_node(
    node: &TreeNode,
    depth: usize,
    state: crate::state::AppState,
    collapsed: RwSignal<HashSet<String>>,
) -> View {
    let indent = format!("padding-left:{}px", depth as f64 * INDENT_PX);

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
            // Children are rendered eagerly but hidden when collapsed, so the
            // expand toggle is instant and selection state is preserved.
            let children_view = render_children(node, depth + 1, state, collapsed);
            view! {
                <div class="repo-row repo-row--dir" style=indent on:click=on_click>
                    <span class="repo-row__caret">
                        {move || if is_collapsed.get() { "▸" } else { "▾" }}
                    </span>
                    <span class="repo-row__name repo-row__name--dir">{name}</span>
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
            let data = state.data.get_untracked();
            let owner_index = build_owner_index(&data.nodes);
            let owner = resolve_owner(&path, &owner_index).and_then(|i| data.nodes.get(i).cloned());

            let (rail, owner_label, owner_id) = match &owner {
                Some(n) => (role_color_var(&n.node_type), n.name.clone(), Some(n.id.clone())),
                // Unowned files get a neutral rail (the hairline), never a role hue.
                None => ("var(--line)".to_string(), String::new(), None),
            };
            let style = format!("{indent};border-left-color:{rail}");

            let on_click = move |_| {
                if let Some(id) = owner_id.clone() {
                    state.set_selected_node(id);
                }
            };
            let owned = owner.is_some();

            view! {
                <div
                    class="repo-row repo-row--file"
                    class=("repo-row--owned", owned)
                    style=style
                    on:click=on_click
                >
                    <span class="repo-row__name">{name}</span>
                    {(!owner_label.is_empty()).then(|| view! {
                        <span class="repo-row__owner" style=format!("color:{rail}")>{owner_label}</span>
                    })}
                </div>
            }
            .into_view()
        }
    }
}
