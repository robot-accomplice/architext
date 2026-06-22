//! App state: the loaded dataset + current mode/view/flow selection, provided
//! to the component tree via Leptos context.
//!
//! Selections are stored as indices into the loaded `views`/`flows` vectors
//! (stable for the session — the data is loaded once and never mutated in V2).
//! `AppState` is `Clone` (it holds `RwSignal`s, which are `Copy`) so panels read
//! it out of context cheaply.

use leptos::*;
use std::rc::Rc;

use crate::data::ArchitectureData;
use crate::data::{models::RepoTreePayload, FetchError};
use crate::selection;
use crate::theme::{load_theme, Mode, Theme};

/// The reactive application state. Cheap to clone (signals are `Copy`).
#[derive(Clone, Copy)]
pub struct AppState {
    /// Loaded dataset, shared (immutable in V2). `Rc` keeps clones cheap.
    pub data: RwSignal<Rc<ArchitectureData>>,
    /// Active navigation mode.
    pub mode: RwSignal<Mode>,
    /// Selected view, as an index into `data.views` (None if the mode/data has none).
    pub view_idx: RwSignal<Option<usize>>,
    /// Selected flow, as an index into `data.flows` (only meaningful in flows mode).
    pub flow_idx: RwSignal<Option<usize>>,
    /// Node selected by clicking a diagram card (drives the inspector). Cleared
    /// when the view/flow changes so a stale node never inspects a node absent
    /// from the new projection.
    pub selected_node: RwSignal<Option<String>>,
    /// Flow step selected by clicking a steps-panel card (== the step id, which
    /// is also the matching diagram route id in flows mode → drives the
    /// `--accent` active edge treatment). Cleared on every flow/view/mode change
    /// so a stale step never highlights an edge absent from the new projection.
    pub selected_step: RwSignal<Option<String>>,
    /// Whether the footer steps panel is collapsed to its header line. Toggling
    /// it resizes the canvas, so the canvas re-fit effect tracks it.
    pub steps_collapsed: RwSignal<bool>,
    /// Running CLI version (from `/api/status`), for the header eyebrow.
    /// Display-only; `None` until fetched (or if the server omits it).
    pub cli_version: RwSignal<Option<String>>,
    /// Per-process mutation token (from `GET /api/session`) authorizing writes
    /// via the `x-architext-mutation-token` header. `None` until fetched (or if
    /// the server omits it), in which case editing affordances stay disabled —
    /// the single source of write authorization for every editing surface.
    pub mutation_token: RwSignal<Option<String>>,
    /// Whether the left nav is collapsed to its thin rail (DESIGN.md: auxiliary
    /// panels collapse to icons/drawers). Drives the shell grid + the canvas
    /// re-fit when the center track resizes.
    pub nav_collapsed: RwSignal<bool>,
    /// Whether the right inspector is collapsed to its thin rail.
    pub inspector_collapsed: RwSignal<bool>,
    /// Whether the live-reload SSE stream (`/api/data-events`) is connected.
    /// Drives the small "live" indicator; display-only.
    pub live_connected: RwSignal<bool>,
    /// A non-blocking notice shown when a data change failed validation. The
    /// last-good diagram keeps rendering; this surfaces the validator summary so
    /// the user knows the on-disk data is currently invalid. `None` when valid.
    pub invalid_notice: RwSignal<Option<String>>,
    /// The selected release id (Release Truth mode). Lifted out of the panel so
    /// the left-nav release selector and the center detail share ONE source of
    /// truth: selecting in the navbar updates the detail. Seeded lazily (from the
    /// index's currentReleaseId / newest release) by `ReleaseTruthPanel`; `None`
    /// until then or if no releases are recorded.
    pub selected_release: RwSignal<Option<String>>,
    /// The Release Path item selected by clicking its line — drives the
    /// inspector's item detail in Release Truth mode (the item id). Cleared when
    /// the selected release changes.
    pub selected_release_item: RwSignal<Option<String>>,

    /// Active color theme (Dark default / Light). Seeded from localStorage in
    /// `new`; an effect in `App` applies it as `data-theme` on <html> and
    /// persists changes.
    pub theme: RwSignal<Theme>,

    /// C4 drill-down breadcrumb trail: the chain of view indices from the C4
    /// root down to the currently shown view. The LAST entry is always the
    /// active view (`view_idx`); earlier entries are the ancestor views a node
    /// click drilled through. Only meaningful in C4 mode — entering C4 (or
    /// picking a C4 view from the selector) re-roots it to a single-entry trail,
    /// and leaving C4 clears it. A node click that resolves a scoped child view
    /// PUSHES onto it; clicking a breadcrumb crumb POPS back to that depth. This
    /// is the only navigation history the viewer keeps; modes that don't drill
    /// never read it.
    pub c4_trail: RwSignal<Vec<usize>>,

    /// Cached `/api/repo-tree` result, fetched ONCE and shared across RepoTree
    /// (re)mounts. The canvas center region remounts RepoTree several times as a
    /// mode switch settles; an unguarded per-mount fetch fired ~5 concurrent
    /// requests (5× the `git ls-files` cost on a real repo — the "slow initial
    /// load"). `repo_tree_loading` guards same-tick mounts down to one fetch.
    pub repo_tree: RwSignal<Option<Result<RepoTreePayload, FetchError>>>,
    pub repo_tree_loading: RwSignal<bool>,
}

impl AppState {
    /// Construct state from a loaded dataset and seed the initial selection
    /// (flows mode → its default flow + the default view for that flow).
    pub fn new(data: ArchitectureData) -> Self {
        let mode = Mode::Flows;
        let data = Rc::new(data);

        // Seed selection using the routing-backed rules.
        let flow_idx = if mode.projects_flows() && !data.flows.is_empty() { Some(0) } else { None };
        let view_idx = match flow_idx {
            Some(f) => selection::default_view_for_flow(&data.views, &data.flows, mode, None, f),
            None => selection::default_view_for_mode(&data.views, mode),
        };

        Self {
            data: create_rw_signal(data),
            mode: create_rw_signal(mode),
            view_idx: create_rw_signal(view_idx),
            flow_idx: create_rw_signal(flow_idx),
            selected_node: create_rw_signal(None),
            selected_step: create_rw_signal(None),
            steps_collapsed: create_rw_signal(false),
            cli_version: create_rw_signal(None),
            mutation_token: create_rw_signal(None),
            nav_collapsed: create_rw_signal(false),
            inspector_collapsed: create_rw_signal(false),
            live_connected: create_rw_signal(false),
            invalid_notice: create_rw_signal(None),
            selected_release: create_rw_signal(None),
            selected_release_item: create_rw_signal(None),
            theme: create_rw_signal(load_theme()),
            // Initial mode is Flows, which doesn't drill — start with no trail.
            c4_trail: create_rw_signal(Vec::new()),
            repo_tree: create_rw_signal(None),
            repo_tree_loading: create_rw_signal(false),
        }
    }

    /// Re-root the C4 breadcrumb trail at `view_idx` if it is a C4 view, else
    /// clear it. Used whenever the C4 root changes (entering C4 mode, picking a
    /// C4 view from the selector, or a reload that re-resolves the view): the
    /// trail collapses to a single crumb (the new root) so it never carries a
    /// stale ancestor chain from a different root. A non-C4 view yields an empty
    /// trail (no breadcrumb renders outside C4 mode).
    fn reroot_c4_trail(&self, view: Option<usize>) {
        let data = self.data.get_untracked();
        let trail = match view {
            Some(i) if data.views.get(i).is_some_and(|v| v.view_type.starts_with("c4-")) => {
                vec![i]
            }
            _ => Vec::new(),
        };
        self.c4_trail.set(trail);
    }

    /// Replace the loaded dataset after a live-reload, PRESERVING the user's
    /// current mode/view/flow selection. The selection is stored as indices into
    /// the `views`/`flows` vectors; on reload those vectors may have changed
    /// length, so we clamp each index to the new bounds (an index that points
    /// past the end of the new data falls back to the mode/flow default rather
    /// than dangling). A successful reload also clears any invalid notice.
    pub fn reload_data(&self, next: ArchitectureData) {
        let next = Rc::new(next);
        let mode = self.mode.get_untracked();

        // Clamp the view index to the new views vector; fall back to the
        // mode-appropriate default if the old index is now out of range.
        let view = match self.view_idx.get_untracked() {
            Some(v) if v < next.views.len() => Some(v),
            _ => selection::default_view_for_mode(&next.views, mode),
        };
        // Clamp the flow index likewise (only meaningful when the mode projects
        // flows; otherwise it stays None).
        let flow = if mode.projects_flows() {
            match self.flow_idx.get_untracked() {
                Some(f) if f < next.flows.len() => Some(f),
                _ if !next.flows.is_empty() => Some(0),
                _ => None,
            }
        } else {
            None
        };

        self.data.set(next);
        self.view_idx.set(view);
        self.flow_idx.set(flow);
        self.invalid_notice.set(None);
        // A stale node/step selection may not exist in the reloaded projection;
        // clear both so nothing dangles (mirrors set_view/set_flow behavior).
        self.selected_node.set(None);
        self.selected_step.set(None);
        // The reloaded views vector may have shifted; the old trail's indices
        // can't be trusted, so re-root at the (clamped) active view.
        self.reroot_c4_trail(view);
    }

    /// Replace ONLY the resolved diagram config after a `POST /api/config`
    /// write, preserving the loaded architecture data and the current
    /// mode/view/flow/selection.
    ///
    /// Config lives outside the watched data dir, so a write does NOT fire the
    /// data-events SSE; the editor must push the re-resolved config back through
    /// here. Replacing the `data` signal re-runs the canvas's plan compute (whose
    /// selection key folds in the config layout identity), so the diagram reflows
    /// with the new layout — the just-refreshed plan farm serves the new key, or
    /// the in-process fallback computes it.
    pub fn set_config(&self, config: crate::data::models::ConfigPayload) {
        let current = self.data.get_untracked();
        let mut next = (*current).clone();
        next.config = Some(config);
        self.data.set(Rc::new(next));
    }

    /// Select a node by id (diagram click → inspector).
    pub fn set_selected_node(&self, node_id: String) {
        self.selected_node.set(Some(node_id));
    }

    /// Select a flow step by id (steps-panel click → highlight the matching
    /// diagram edge). The step id equals the flow route id, so the diagram keys
    /// its `--accent` active edge on this.
    pub fn set_selected_step(&self, step_id: String) {
        self.selected_step.set(Some(step_id));
    }

    /// Switch modes and re-seed the view/flow selection per the routing rules.
    pub fn set_mode(&self, mode: Mode) {
        let data = self.data.get_untracked();
        self.selected_node.set(None);
        self.selected_step.set(None);
        self.mode.set(mode);
        if mode.renders_routed_flow() {
            // Flows / Data-Risks: the flow drives; resolve the view to a
            // compatible flow-projection.
            let flow = if data.flows.is_empty() { None } else { Some(0) };
            self.flow_idx.set(flow);
            let view = match flow {
                Some(f) => selection::default_view_for_flow(&data.views, &data.flows, mode, None, f),
                None => selection::default_view_for_mode(&data.views, mode),
            };
            self.view_idx.set(view);
        } else if mode.projects_flows() {
            // Sequence: the view is fixed by the mode (the `sequence` view); keep the
            // CURRENT flow if it's compatible (so switching into Sequence mode doesn't
            // silently jump back to the first flow), else fall back to the first.
            let current_flow = self.flow_idx.get_untracked();
            let view = selection::default_view_for_mode(&data.views, mode);
            self.view_idx.set(view);
            let flow = view.and_then(|v| {
                selection::default_flow_for_view(&data.views, &data.flows, v, current_flow)
            });
            self.flow_idx.set(flow);
        } else {
            self.flow_idx.set(None);
            self.view_idx.set(selection::default_view_for_mode(&data.views, mode));
        }
        // Re-root the breadcrumb trail at the new mode's view: a single crumb
        // when entering C4 (the Context root), empty for every non-C4 mode.
        self.reroot_c4_trail(self.view_idx.get_untracked());
    }

    /// Select a flow (flows mode); re-resolve the view to a compatible one.
    pub fn set_flow(&self, flow_idx: usize) {
        let data = self.data.get_untracked();
        self.selected_node.set(None);
        self.selected_step.set(None);
        self.flow_idx.set(Some(flow_idx));
        let current = self.view_idx.get_untracked();
        let view = selection::default_view_for_flow(
            &data.views,
            &data.flows,
            self.mode.get_untracked(),
            current,
            flow_idx,
        );
        self.view_idx.set(view);
    }

    /// Select a view directly (the VIEW selector, Blast Radius cross-links). In
    /// flows mode re-resolves the flow to a compatible one. Because this is a
    /// "jump straight to this view" action — not a drill — it RE-ROOTS the C4
    /// breadcrumb trail: picking a C4 view from the selector starts a fresh trail
    /// at that view rather than appending to an unrelated drill path.
    pub fn set_view(&self, view_idx: usize) {
        let data = self.data.get_untracked();
        self.selected_node.set(None);
        self.selected_step.set(None);
        self.view_idx.set(Some(view_idx));
        if self.mode.get_untracked().projects_flows() {
            let current = self.flow_idx.get_untracked();
            let flow = selection::default_flow_for_view(&data.views, &data.flows, view_idx, current);
            self.flow_idx.set(flow);
        }
        self.reroot_c4_trail(Some(view_idx));
    }

    /// Drill DOWN into a scoped C4 child view (a node click in C4 mode resolved
    /// `child_view_idx` via `child_c4_view_for_node`). Unlike [`Self::set_view`],
    /// this PUSHES the child onto the breadcrumb trail so the ancestor chain is
    /// preserved and the breadcrumb can navigate back up. Clears the node/step
    /// selection like every other view change. C4 mode only — the caller has
    /// already established we are in C4 and a scoped child exists.
    pub fn drill_to_c4_view(&self, child_view_idx: usize) {
        self.selected_node.set(None);
        self.selected_step.set(None);
        self.view_idx.set(Some(child_view_idx));
        self.c4_trail.update(|t| t.push(child_view_idx));
    }

    /// Navigate to breadcrumb `depth` (0 == the C4 root). Truncates the trail to
    /// that depth + 1 (dropping the descendants the user is climbing out of) and
    /// makes that view active. A no-op if `depth` is already the last crumb or
    /// out of range. Clears the node/step selection like any view change.
    pub fn navigate_to_c4_crumb(&self, depth: usize) {
        let trail = self.c4_trail.get_untracked();
        let Some(&view_idx) = trail.get(depth) else { return };
        if depth + 1 == trail.len() {
            return; // already here
        }
        self.selected_node.set(None);
        self.selected_step.set(None);
        self.view_idx.set(Some(view_idx));
        self.c4_trail.update(|t| t.truncate(depth + 1));
    }
}

/// Read the `AppState` out of context. Panics if not provided (a programming
/// error — the shell always provides it before rendering panels).
pub fn use_app_state() -> AppState {
    use_context::<AppState>().expect("AppState must be provided by the shell")
}
