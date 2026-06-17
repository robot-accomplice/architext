//! Fixed-width left nav: the shared wordmark + the nine-mode list.
use leptos::*;

use crate::components::mode_list::ModeList;
use crate::components::wordmark::Wordmark;
use crate::theme::Mode;

#[component]
pub fn LeftNav(active: ReadSignal<Mode>, set_active: WriteSignal<Mode>) -> impl IntoView {
    view! {
        <nav class="left-nav">
            <Wordmark/>
            <div class="overline">"MODES"</div>
            <ModeList active=active set_active=set_active/>
        </nav>
    }
}
