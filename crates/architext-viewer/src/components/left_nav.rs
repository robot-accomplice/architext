//! Fixed-width left nav: the shared wordmark, the nine-mode list, and the
//! view/flow selector region (data-driven, below the modes).
use leptos::*;

use crate::components::mode_list::ModeList;
use crate::components::selector_bar::SelectorBar;
use crate::components::wordmark::Wordmark;

#[component]
pub fn LeftNav() -> impl IntoView {
    view! {
        <nav class="left-nav">
            <Wordmark/>
            <div class="overline">"MODES"</div>
            <ModeList/>
            <SelectorBar/>
        </nav>
    }
}
