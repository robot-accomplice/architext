//! The empty state for the two enrichment modes (Code Graph, Slop Detection).
//!
//! These are the only modes whose data Architext does not produce itself, so
//! their empty state has a job no other mode's does: explain a DEPENDENCY.
//!
//! What it replaced said, in full: "This project has no code-graph.json
//! registered under manifest.files.codeGraph." That is a restatement of the
//! condition the user is already looking at. It names no tool, says nothing
//! about whether the tool is installed, gives no way to get it, and offers no
//! way to proceed — a dead end phrased as a diagnosis.
//!
//! This asks the server which tools are actually present and branches:
//!
//!   - **installed** → say so (with version) and offer to RUN it, because at
//!     that point the only thing between the user and data is a subprocess;
//!   - **missing** → name the binary, say what it produces IN THE USER'S TERMS,
//!     give the install command, and link the repository.
//!
//! Failures are reported with the tool's OWN stderr. These pipelines refuse for
//! good reasons — ferret rejects a dirty tree because "a dirty map reports
//! in-flight code as dead" — and that sentence is more useful than any wrapper
//! message could be.

use leptos::*;
use serde_json::{json, Value};

use crate::data::mutate::post_mutation;
use crate::state::use_app_state;

/// Which enrichment a panel is missing.
#[derive(Clone, Copy, PartialEq)]
pub enum Enrichment {
    CodeGraph,
    SlopDetection,
}

impl Enrichment {
    /// The `/api/tools` key whose presence gates this mode.
    fn tool_key(self) -> &'static str {
        match self {
            // Slop Detection needs BOTH, but ferret is the one that defines the
            // mode; magma missing is reported by the run itself, in its terms.
            Enrichment::CodeGraph => "magma",
            Enrichment::SlopDetection => "ferret",
        }
    }

    /// `tool` value for `POST /api/tools/run`.
    fn run_tool(self) -> &'static str {
        match self {
            Enrichment::CodeGraph => "magma",
            Enrichment::SlopDetection => "slop-ferret",
        }
    }

    /// How the TOOL is known, which is not the mode name. The mode is "Slop
    /// Detection"; the tool that produces its data is still Slop Ferret. Saying
    /// "ferret is installed" makes the reader map a binary name to a product
    /// themselves.
    fn tool_display(self) -> &'static str {
        match self {
            Enrichment::CodeGraph => "Magma",
            Enrichment::SlopDetection => "Slop Ferret",
        }
    }

    fn heading(self) -> &'static str {
        match self {
            Enrichment::CodeGraph => "No code graph yet",
            Enrichment::SlopDetection => "No sweep yet",
        }
    }

    /// What the mode WOULD show — stated before the dependency, so a reader
    /// deciding whether to install anything knows what they would get.
    fn what_it_gives(self) -> &'static str {
        match self {
            Enrichment::CodeGraph => {
                "Code Graph renders this repository's call graph: what calls what, which functions \
                 nothing reaches, and the call order from the entry points."
            }
            Enrichment::SlopDetection => {
                "Slop Detection renders a sweep for work that looks finished and is not: functions \
                 nothing calls, tests that cannot fail, guards that cannot fire. It shows coverage \
                 and what was NOT checked, not only what was found."
            }
        }
    }

    fn install_hint(self) -> &'static str {
        match self {
            Enrichment::CodeGraph => "go install github.com/robot-accomplice/magma@latest",
            Enrichment::SlopDetection => {
                "go install github.com/robot-accomplice/slop-ferret/cmd/ferret@latest"
            }
        }
    }
}

/// Shape of one tool in the `/api/tools` payload.
fn tool_field<'a>(tools: &'a Value, key: &str, field: &str) -> Option<&'a str> {
    tools.get(key)?.get(field)?.as_str()
}

fn tool_installed(tools: &Value, key: &str) -> bool {
    tools.get(key).and_then(|t| t.get("installed")).and_then(Value::as_bool).unwrap_or(false)
}

#[component]
pub fn EnrichmentEmptyState(kind: Enrichment) -> impl IntoView {
    let state = use_app_state();
    let tools = create_rw_signal::<Option<Value>>(None);
    let running = create_rw_signal(false);
    let outcome = create_rw_signal::<Option<(bool, String)>>(None);
    // Set when the user asks for a sweep before a code graph exists. Slop
    // detection reads the map magma builds, so a sweep without one is a weaker
    // sweep, not an invalid one. That makes it a CONFIRMATION rather than a
    // block: the user may have a reason, and the tool should not decide for
    // them. Declining offers the better order instead of leaving a dead end.
    let confirm_no_graph = create_rw_signal(false);
    let has_code_graph =
        move || state.data.get().code_graph.as_ref().and_then(|r| r.as_ref().ok()).is_some();

    // Ask the server what is actually installed. A discovery failure leaves
    // `tools` as None and the panel falls back to the instructions-only view —
    // never a spinner that never resolves.
    spawn_local(async move {
        if let Ok(resp) = gloo_net::http::Request::get("/api/tools").send().await {
            if let Ok(v) = resp.json::<Value>().await {
                tools.set(Some(v));
            }
        }
    });

    let start = move |tool: &'static str| {
        if running.get_untracked() {
            return;
        }
        confirm_no_graph.set(false);
        running.set(true);
        outcome.set(None);
        let token = state.mutation_token.get_untracked();
        spawn_local(async move {
            let body = json!({ "tool": tool });
            let res = post_mutation(token.as_deref(), "/api/tools/run", &body).await;
            match res {
                // The data file is written by the run; the SSE data-events
                // watcher repopulates the mode without a reload.
                Ok(_) => outcome.set(Some((true, "Done. The mode will populate shortly.".into()))),
                Err(e) => outcome.set(Some((false, e.to_string()))),
            }
            running.set(false);
        });
    };

    // Slop detection is the only mode with an ORDER preference, so the gate
    // lives here rather than in the shared run path.
    let run = move |_| {
        if kind == Enrichment::SlopDetection && !has_code_graph() {
            confirm_no_graph.set(true);
            return;
        }
        start(kind.run_tool());
    };

    view! {
        <div class="enrichment-empty">
            <h2>{kind.heading()}</h2>
            <p class="enrichment-empty__what">{kind.what_it_gives()}</p>
            {move || {
                let t = tools.get();
                let key = kind.tool_key();
                let installed = t.as_ref().map(|t| tool_installed(t, key)).unwrap_or(false);
                let repo = t.as_ref().and_then(|t| tool_field(t, key, "repo")).unwrap_or("").to_string();
                let provides =
                    t.as_ref().and_then(|t| tool_field(t, key, "provides")).unwrap_or("").to_string();
                let version = t.as_ref().and_then(|t| tool_field(t, key, "version")).unwrap_or("").to_string();

                if installed {
                    view! {
                        <div class="enrichment-empty__ready">
                            <p>
                                {kind.tool_display()}" ("
                                <code>{key}</code>
                                {if version.is_empty() { String::new() } else { format!(" v{version}") }}
                                ") located. Click the button below to run it now."
                            </p>
                            <button class="enrichment-empty__run" on:click=run disabled=move || running.get()>
                                {move || if running.get() { "Running…" } else { "Run now" }}
                            </button>
                            <p class="enrichment-empty__note">
                                "Runs against the current commit. A dirty working tree is refused on \
                                 purpose: a map of uncommitted code reports work-in-progress as dead."
                            </p>
                        </div>
                    }.into_view()
                } else {
                    view! {
                        <div class="enrichment-empty__missing">
                            <p>
                                "This mode is produced by "<code>{key}</code>", which is "
                                <strong>"not installed"</strong>
                                {(!provides.is_empty()).then(|| format!(". It provides {provides}."))}
                            </p>
                            <p class="enrichment-empty__label">"Install it:"</p>
                            <pre class="enrichment-empty__cmd">{kind.install_hint()}</pre>
                            {(!repo.is_empty()).then(|| view! {
                                <p>
                                    <a class="enrichment-empty__repo" href=repo.clone() target="_blank" rel="noreferrer">
                                        {repo}
                                    </a>
                                </p>
                            })}
                            <p class="enrichment-empty__note">
                                "Architext reads this data, it does not produce it. Once the tool is on \
                                 PATH, reload and this panel will offer to run it."
                            </p>
                        </div>
                    }.into_view()
                }
            }}
            {move || confirm_no_graph.get().then(|| view! {
                <div class="enrichment-empty__confirm">
                    <p class="enrichment-empty__confirm-head">"No code graph has been built for this project."</p>
                    <p>
                        "Slop detection reads the call map that Code Graph is built from. Without it, \
                         the sweep still runs, but reachability is weaker: fewer functions can be shown \
                         as unreached, so it will find less."
                    </p>
                    <p class="enrichment-empty__label">"Recommended order: build the code graph first, then sweep."</p>
                    <div class="enrichment-empty__confirm-actions">
                        <button class="enrichment-empty__run" on:click=move |_| start("magma")>
                            "Build the code graph now"
                        </button>
                        <button class="enrichment-empty__secondary" on:click=move |_| start(kind.run_tool())>
                            "Sweep anyway"
                        </button>
                        <button class="enrichment-empty__secondary" on:click=move |_| confirm_no_graph.set(false)>
                            "Cancel"
                        </button>
                    </div>
                </div>
            })}
            // The tool's own words, verbatim. A refusal from ferret or magma is
            // the most useful thing on this screen when it happens.
            {move || outcome.get().map(|(ok, msg)| view! {
                <pre class=move || if ok { "enrichment-empty__out" } else { "enrichment-empty__out enrichment-empty__out--err" }>
                    {msg}
                </pre>
            })}
        </div>
    }
}
