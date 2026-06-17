//! The nav mode list: nine entries, one active at a time.
//!
//! Active state uses the dedicated state treatment (`.is-active` = --accent
//! ring/glow, DESIGN.md rule 1), never a --c4-* role hue. Selection is local
//! Leptos signal state for now — no data routing yet (V1 scaffold).
use leptos::*;

use crate::theme::Mode;

#[component]
pub fn ModeList(
    /// Current selection.
    active: ReadSignal<Mode>,
    /// Setter the nav buttons drive.
    set_active: WriteSignal<Mode>,
) -> impl IntoView {
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
                                on:click=move |_| set_active.set(mode)
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
