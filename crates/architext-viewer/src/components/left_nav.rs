//! Fixed-width left nav: the shared wordmark, the nine-mode list, and the
//! view/flow selector region (data-driven, below the modes).
use leptos::*;

use crate::components::mode_icon::ModeIcon;
use crate::components::mode_list::ModeList;
use crate::components::selector_bar::SelectorBar;
use crate::components::wordmark::Wordmark;
use crate::state::use_app_state;
use crate::theme::Mode;

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
                <ModeRail/>
            </Show>
        </nav>
    }
}

/// Collapsed-rail mode switcher: the nine modes as icon-only buttons (vertical),
/// reusing the same `Mode::ALL` list and `set_mode` selection as `ModeList`.
/// Active mode carries the `--accent` state treatment (rule 1, never a role
/// hue); each button's `title`/`aria-label` is the mode label (hover tooltip).
/// Selecting a mode here leaves `nav_collapsed` untouched, so the rail stays
/// collapsed after switching.
#[component]
fn ModeRail() -> impl IntoView {
    let state = use_app_state();
    let active = state.mode;

    view! {
        <ul class="mode-rail">
            {Mode::ALL
                .into_iter()
                .map(|mode| {
                    view! {
                        <li>
                            <button
                                class="mode-rail__item"
                                class:is-active=move || active.get() == mode
                                title=mode.label()
                                aria-label=mode.label()
                                on:click=move |_| state.set_mode(mode)
                            >
                                <ModeIcon mode=mode/>
                            </button>
                        </li>
                    }
                })
                .collect_view()}
        </ul>
    }
}
