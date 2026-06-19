//! Top header strip — the full-width row above the console triad.
//!
//! Faithful to the old viewer `.topbar` (`viewer/src/main.tsx` ~1501) on the
//! Cyber-Tactical language: a mono eyebrow (`Architext / <cli> · Schema /
//! <schema>`), the project name (Hanken headline) + summary (muted, single-line
//! ellipsis), and a right-side `⚙ Config` button that opens the read-only config
//! drawer. The mode list and the ARCHITEXT wordmark are NOT duplicated here —
//! they live in the left nav.

use leptos::*;

use crate::components::config_panel::ConfigPanel;
use crate::state::use_app_state;

#[component]
pub fn Header() -> impl IntoView {
    let state = use_app_state();
    // Drawer open/close, owned here and threaded into the config panel.
    let config_open = create_rw_signal(false);

    // Eyebrow: `Architext / <cliVersion> · Schema / <schemaVersion>`. The CLI
    // version is a deferred fetch (None until loaded); the schema version is in
    // the manifest. Either missing degrades to a dash rather than a blank.
    let eyebrow = move || {
        let data = state.data.get();
        let schema = data
            .manifest
            .as_ref()
            .map(|m| m.schema_version.clone())
            .unwrap_or_else(|| "—".to_string());
        let cli = state.cli_version.get().unwrap_or_else(|| "—".to_string());
        format!("Architext / {cli} · Schema / {schema}")
    };

    let project_name = move || {
        state
            .data
            .get()
            .manifest
            .as_ref()
            .map(|m| m.project.name.clone())
            .unwrap_or_default()
    };
    let project_summary = move || {
        state
            .data
            .get()
            .manifest
            .as_ref()
            .and_then(|m| m.project.summary.clone())
            .unwrap_or_default()
    };

    view! {
        <header class="topbar">
            <div class="topbar__identity">
                <p class="overline topbar__eyebrow">{eyebrow}</p>
                <div class="topbar__title-line">
                    <h1 class="topbar__project">{project_name}</h1>
                    <p class="topbar__summary">{project_summary}</p>
                </div>
            </div>
            <div class="topbar__actions">
                <LiveIndicator/>
                <button
                    class="topbar__theme-btn"
                    title=move || format!("Switch to {} theme", state.theme.get().toggle_label())
                    aria-label=move || format!("Switch to {} theme", state.theme.get().toggle_label())
                    on:click=move |_| state.theme.update(|t| *t = t.toggled())
                >
                    <span aria-hidden="true">{move || state.theme.get().toggle_icon()}</span>
                    {move || state.theme.get().toggle_label()}
                </button>
                <button
                    class="topbar__config-btn"
                    title="View resolved diagram configuration"
                    on:click=move |_| config_open.set(true)
                >
                    <span aria-hidden="true">"⚙ "</span>"Config"
                </button>
            </div>
            <InvalidNotice/>
            <ConfigPanel open=config_open/>
        </header>
    }
}

/// A hairline "live" indicator: a small mono dot+label keyed to the live-reload
/// SSE connection. `--accent` when connected, muted when not. Display-only.
#[component]
fn LiveIndicator() -> impl IntoView {
    let state = use_app_state();
    let connected = move || state.live_connected.get();
    view! {
        <span
            class="topbar__live"
            class=("topbar__live--on", connected)
            title=move || if connected() { "Live reload connected" } else { "Live reload offline" }
        >
            <span class="topbar__live-dot" aria-hidden="true"></span>
            "LIVE"
        </span>
    }
}

/// Non-blocking notice strip shown when the on-disk data fails validation. The
/// last-good diagram keeps rendering; this only surfaces the validator summary.
/// Hairline + mono + `--sev-high` per DESIGN.md (a warning, not a hard error).
#[component]
fn InvalidNotice() -> impl IntoView {
    let state = use_app_state();
    view! {
        {move || state.invalid_notice.get().map(|summary| view! {
            <div class="topbar__notice" role="status">
                <span class="topbar__notice-tag">"DATA INVALID"</span>
                <span class="topbar__notice-msg">{summary}</span>
            </div>
        })}
    }
}
