//! Fixed-width left nav: the shared wordmark, the nine-mode list, and the
//! view/flow selector region (data-driven, below the modes).
use leptos::*;

use crate::components::mode_list::ModeList;
use crate::components::selector_bar::SelectorBar;
use crate::components::wordmark::Wordmark;
use crate::state::use_app_state;

#[component]
pub fn LeftNav() -> impl IntoView {
    let state = use_app_state();
    let collapsed = state.nav_collapsed;
    let toggle = move |_| collapsed.update(|c| *c = !*c);

    let nav_class = move || {
        if collapsed.get() {
            "left-nav left-nav--collapsed"
        } else {
            "left-nav"
        }
    };

    view! {
        <nav class=nav_class>
            <Show
                when=move || collapsed.get()
                fallback=move || view! {
                    <div class="panel-collapse-header">
                        <Wordmark/>
                        <button
                            class="panel-collapse-toggle"
                            title="Collapse navigation"
                            on:click=toggle
                        >"‹"</button>
                    </div>
                    <div class="overline">"MODES"</div>
                    <ModeList/>
                    <SelectorBar/>
                }
            >
                <button
                    class="panel-collapse-toggle panel-collapse-toggle--rail"
                    title="Expand navigation"
                    on:click=toggle
                >"›"</button>
            </Show>
        </nav>
    }
}
