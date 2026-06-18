//! Rules surface (DISPLAY only).
//!
//! A left list of rules ordered by criticality → order → title
//! (`rule_order::ordered_rule_indices`, the faithful port of the JS
//! `orderedRules` comparator), with an "All + per-category" chip filter. Each
//! card shows order#, title, a criticality badge (its OWN `--sev-*` scale, never
//! a `--c4-*` role hue), a category badge, an optional protection badge, and the
//! summary. Selecting a card opens a read-only detail pane (id / title /
//! criticality / category / source / summary / protection).
//!
//! Editing — the upsert form, reorder, POST — is V5; this slice is read-only.

use leptos::*;

use crate::data::models::Rule;
use crate::rule_order::{
    criticality_color_var, ordered_categories, ordered_rule_indices, protection_label,
};
use crate::state::use_app_state;

const ALL_CATEGORIES: &str = "__all__";

#[component]
pub fn RulesPanel() -> impl IntoView {
    let state = use_app_state();

    // Local UI state: the active category filter + the selected rule id.
    let active_category = create_rw_signal(ALL_CATEGORIES.to_string());
    let selected_rule = create_rw_signal::<Option<String>>(None);

    // Ordered rule indices (stable for the session — data is immutable).
    let ordered = move || {
        let data = state.data.get();
        ordered_rule_indices(&data.rules)
    };

    let categories = move || {
        let data = state.data.get();
        ordered_categories(&data.rules)
    };

    view! {
        <div class="rules-panel">
            <div class="rules-panel__list">
                <div class="overline rules-panel__section">"RULES"</div>
                // Category filter: All + per-category chips.
                <div class="rules-panel__filter">
                    {move || {
                        let active = active_category.get();
                        let mut chips = vec![category_chip(
                            "All".to_string(), ALL_CATEGORIES.to_string(),
                            active == ALL_CATEGORIES, active_category,
                        )];
                        for cat in categories() {
                            let is_active = active == cat;
                            chips.push(category_chip(cat.clone(), cat, is_active, active_category));
                        }
                        chips.into_iter().collect_view()
                    }}
                </div>
                // Ordered, filtered cards.
                <div class="rules-panel__cards">
                    {move || {
                        let data = state.data.get();
                        let active = active_category.get();
                        ordered()
                            .into_iter()
                            .filter_map(|i| data.rules.get(i).map(|r| (i, r.clone())))
                            .filter(|(_, r)| {
                                active == ALL_CATEGORIES
                                    || r.category.as_deref() == Some(active.as_str())
                            })
                            .map(|(_, rule)| rule_card(rule, selected_rule))
                            .collect_view()
                    }}
                </div>
            </div>
            // Read-only detail pane for the selected rule.
            <div class="rules-panel__detail">
                {move || {
                    let data = state.data.get();
                    match selected_rule.get()
                        .and_then(|id| data.rules.iter().find(|r| r.id == id).cloned())
                    {
                        Some(rule) => rule_detail(rule).into_view(),
                        None => view! {
                            <p class="rules-panel__hint">"Select a rule to see its detail."</p>
                        }.into_view(),
                    }
                }}
            </div>
        </div>
    }
}

/// A category filter chip. `--accent` STATE treatment when active (not a hue).
fn category_chip(
    label: String,
    value: String,
    active: bool,
    active_category: RwSignal<String>,
) -> View {
    let on_click = move |_| active_category.set(value.clone());
    view! {
        <button class="chip rules-chip" class:is-active=active on:click=on_click>{label}</button>
    }
    .into_view()
}

/// One rule card: order#, title, criticality badge (severity scale), category
/// badge, optional protection badge, summary. The left rail encodes criticality
/// on its own `--sev-*` scale.
fn rule_card(rule: Rule, selected_rule: RwSignal<Option<String>>) -> View {
    let rail = criticality_color_var(rule.criticality.as_deref());
    let id = rule.id.clone();
    let select_id = id.clone();
    let on_click = move |_| selected_rule.set(Some(select_id.clone()));
    let is_selected = create_memo({
        let id = id.clone();
        move |_| selected_rule.get().as_deref() == Some(id.as_str())
    });

    let order_label = rule.order.map(|o| format!("{o:02}")).unwrap_or_else(|| "—".to_string());
    let criticality = rule.criticality.clone().unwrap_or_else(|| "unranked".to_string());
    let protection = protection_label(&rule);

    view! {
        <div
            class="accent-surface rule-card"
            class:is-active=move || is_selected.get()
            style=format!("--accent:{rail}")
            on:click=on_click
        >
            <div class="rule-card__head">
                <span class="rule-card__order">{order_label}</span>
                <h3 class="rule-card__title">{rule.title.clone()}</h3>
            </div>
            <div class="chip-row rule-card__badges">
                <span class="chip rule-card__crit" style=format!("color:{rail}")>{criticality}</span>
                {rule.category.clone().map(|c| view! { <span class="chip">{c}</span> })}
                {protection.map(|p| view! { <span class="chip rule-card__protected">{p}</span> })}
            </div>
            {rule.summary.clone().map(|s| view! {
                <p class="rule-card__summary">{s}</p>
            })}
        </div>
    }
    .into_view()
}

/// Read-only detail for the selected rule.
fn rule_detail(rule: Rule) -> impl IntoView {
    let rail = criticality_color_var(rule.criticality.as_deref());
    let criticality = rule.criticality.clone().unwrap_or_else(|| "unranked".to_string());
    let protection = protection_label(&rule).unwrap_or("editable");

    view! {
        <div class="accent-surface rule-detail" style=format!("--accent:{rail}")>
            <div class="overline">"RULE"</div>
            <h2 class="rule-detail__title">{rule.title.clone()}</h2>
            <dl class="rule-detail__meta">
                <dt>"id"</dt><dd class="mono">{rule.id.clone()}</dd>
                <dt>"criticality"</dt>
                <dd><span class="chip" style=format!("color:{rail}")>{criticality}</span></dd>
                {rule.category.clone().map(|c| view! {
                    <dt>"category"</dt><dd>{c}</dd>
                })}
                {rule.source.clone().map(|s| view! {
                    <dt>"source"</dt><dd>{s}</dd>
                })}
                <dt>"protection"</dt><dd>{protection}</dd>
            </dl>
            {rule.summary.clone().map(|s| view! {
                <p class="rule-detail__summary">{s}</p>
            })}
        </div>
    }
}
