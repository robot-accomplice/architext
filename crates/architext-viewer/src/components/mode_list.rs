//! The nav mode list: nine entries, one active at a time.
//!
//! Active state uses the dedicated state treatment (`.is-active` = --accent
//! ring/glow, DESIGN.md rule 1), never a --c4-* role hue. Selecting a mode
//! drives `AppState::set_mode`, which re-seeds the view/flow selection via the
//! ported routing rules.
use leptos::*;

use crate::state::use_app_state;
use crate::theme::Mode;

#[component]
pub fn ModeList() -> impl IntoView {
    let state = use_app_state();
    let active = state.mode;

    view! {
        <ul class="mode-list">
            {Mode::ALL
                .into_iter()
                .map(|mode| {
                    view! {
                        <li>
                            <button
                                class="mode-list__item"
                                class:is-active=move || active.get() == mode
                                on:click=move |_| state.set_mode(mode)
                            >
                                {mode.label()}
                            </button>
                        </li>
                    }
                })
                .collect_view()}
        </ul>
    }
}
