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
use crate::selection;
use crate::theme::Mode;

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
    /// Whether the left nav is collapsed to its thin rail (DESIGN.md: auxiliary
    /// panels collapse to icons/drawers). Drives the shell grid + the canvas
    /// re-fit when the center track resizes.
    pub nav_collapsed: RwSignal<bool>,
    /// Whether the right inspector is collapsed to its thin rail.
    pub inspector_collapsed: RwSignal<bool>,
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
            nav_collapsed: create_rw_signal(false),
            inspector_collapsed: create_rw_signal(false),
        }
    }

    /// Select a node by id (diagram click → inspector).
    pub fn set_selected_node(&self, node_id: String) {
        self.selected_node.set(Some(node_id));
    }

    /// Switch modes and re-seed the view/flow selection per the routing rules.
    pub fn set_mode(&self, mode: Mode) {
        let data = self.data.get_untracked();
        self.selected_node.set(None);
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
            // Sequence: the view is fixed by the mode (the `sequence` view);
            // resolve the flow to the first one compatible with it.
            let view = selection::default_view_for_mode(&data.views, mode);
            self.view_idx.set(view);
            let flow = view.and_then(|v| {
                selection::default_flow_for_view(&data.views, &data.flows, v, None)
            });
            self.flow_idx.set(flow);
        } else {
            self.flow_idx.set(None);
            self.view_idx.set(selection::default_view_for_mode(&data.views, mode));
        }
    }

    /// Select a flow (flows mode); re-resolve the view to a compatible one.
    pub fn set_flow(&self, flow_idx: usize) {
        let data = self.data.get_untracked();
        self.selected_node.set(None);
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

    /// Select a view; in flows mode re-resolve the flow to a compatible one.
    pub fn set_view(&self, view_idx: usize) {
        let data = self.data.get_untracked();
        self.selected_node.set(None);
        self.view_idx.set(Some(view_idx));
        if self.mode.get_untracked().projects_flows() {
            let current = self.flow_idx.get_untracked();
            let flow = selection::default_flow_for_view(&data.views, &data.flows, view_idx, current);
            self.flow_idx.set(flow);
        }
    }
}

/// Read the `AppState` out of context. Panics if not provided (a programming
/// error — the shell always provides it before rendering panels).
pub fn use_app_state() -> AppState {
    use_context::<AppState>().expect("AppState must be provided by the shell")
}
