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

use crate::code_graph_provenance::{parse_version, MIN_MAGMA_VERSION};
use crate::data::mutate::post_mutation;
use crate::state::use_app_state;

/// Where one pipeline stage has got to.
#[derive(Clone, Copy, PartialEq)]
enum StageState {
    Pending,
    Running,
    /// Finished, but the tool reported a warning. Distinct from `Done` because
    /// a clean tick over a real warning is the kind of quiet dishonesty this
    /// whole surface exists to avoid.
    Partial,
    Done,
    Failed,
}

impl StageState {
    fn css(self) -> &'static str {
        match self {
            StageState::Pending => "pending",
            StageState::Running => "running",
            StageState::Partial => "partial",
            StageState::Done => "done",
            StageState::Failed => "failed",
        }
    }
    /// Mark for a stage that is NOT running. A running stage renders an
    /// indeterminate bar instead (see the view), because the app already speaks
    /// that language: the layout-settle panel and the release cards both use a
    /// track-and-fill. A rotating glyph was neither in that vocabulary nor
    /// legible at this size.
    fn mark(self) -> &'static str {
        match self {
            StageState::Pending => "\u{00b7}",
            StageState::Running => "",
            StageState::Partial => "!",
            StageState::Done => "\u{2713}",
            StageState::Failed => "\u{00d7}",
        }
    }
    fn suffix(self) -> &'static str {
        match self {
            StageState::Pending => " (queued)",
            StageState::Running => "\u{2026}",
            StageState::Partial => " (finished with a warning)",
            StageState::Done => " (done)",
            StageState::Failed => " (failed)",
        }
    }
}

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
    // What is happening RIGHT NOW, in the user's terms. A bare spinner during a
    // two-tool pipeline that takes minutes tells the user nothing: not which
    // tool is working, not that a second one follows, not whether it has hung.
    // Each stage names its tool and stays visible after it finishes, so the
    // finished list is also the record of what ran.
    let stages = create_rw_signal::<Vec<(String, StageState)>>(Vec::new());
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

    // Run a sequence of stages, each a (label, tool) pair, stopping at the
    // first failure. A sequence rather than one call because the user asked
    // what is happening: "Invoking Magma" then "Invoking Slop Ferret" is only
    // possible if the client knows where it is in the pipeline.
    let start_stages = move |steps: Vec<(&'static str, &'static str)>| {
        if running.get_untracked() {
            return;
        }
        confirm_no_graph.set(false);
        running.set(true);
        outcome.set(None);
        stages.set(steps.iter().map(|(l, _)| ((*l).to_string(), StageState::Pending)).collect());
        let token = state.mutation_token.get_untracked();
        spawn_local(async move {
            for (i, (_, tool)) in steps.iter().enumerate() {
                stages.update(|s| s[i].1 = StageState::Running);
                let res = post_mutation(token.as_deref(), "/api/tools/run", &json!({ "tool": tool })).await;
                match res {
                    Ok(v) => {
                        // A stage can succeed PARTIALLY: magma emits a complete
                        // graph while failing to write some map notes. Say so
                        // rather than showing a clean tick over a warning.
                        let warn = v.get("warning").and_then(Value::as_str).map(str::to_string);
                        stages.update(|s| s[i].1 = match &warn {
                            Some(_) => StageState::Partial,
                            None => StageState::Done,
                        });
                        if let Some(w) = warn {
                            outcome.set(Some((true, w)));
                        }
                    }
                    Err(e) => {
                        stages.update(|s| s[i].1 = StageState::Failed);
                        outcome.set(Some((false, e.to_string())));
                        running.set(false);
                        return;
                    }
                }
            }
            running.set(false);
        });
    };

    let start = move |tool: &'static str| {
        let label = if tool == "magma" { "Invoking Magma in the background" } else { "Invoking Slop Ferret" };
        start_stages(vec![(label, tool)]);
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

                // The same judgement applied to the INSTALLED binary, so a
                // stale tool is caught before it produces a stale artifact
                // rather than after someone has drawn conclusions from one.
                let binary_stale = kind == Enrichment::CodeGraph
                    && parse_version(&version).is_some_and(|v| v < MIN_MAGMA_VERSION);
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
                            {binary_stale.then(|| {
                                let (a, b, c) = MIN_MAGMA_VERSION;
                                view! {
                                    <p class="enrichment-empty__stale">
                                        {format!(
                                            "This magma is older than the {a}.{b}.{c} this build expects. It \
                                             predates the disclosure fields, so the graph it produces cannot \
                                             report known analysis limitations. Update before running: "
                                        )}
                                        <code>{kind.install_hint()}</code>
                                    </p>
                                }
                            })}
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
                        <button class="enrichment-empty__run" on:click=move |_| start_stages(vec![
                            ("Invoking Magma in the background", "magma"),
                            ("Invoking Slop Ferret", "ferret"),
                        ])>
                            "Build the code graph, then sweep"
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
            {move || (!stages.get().is_empty()).then(|| view! {
                <ul class="enrichment-empty__stages">
                    {stages.get().into_iter().map(|(label, st)| view! {
                        <li class=format!("enrichment-empty__stage enrichment-empty__stage--{}", st.css())>
                            <span class="enrichment-empty__stage-mark" aria-hidden="true">{st.mark()}</span>
                            <span class="enrichment-empty__stage-label">{label}{st.suffix()}</span>
                            // Indeterminate because these tools give no
                            // fraction to report: magma walks a whole tree and
                            // says nothing until it is done. A bar that faked a
                            // percentage would be inventing progress.
                            {(st == StageState::Running).then(|| view! {
                                <span class="enrichment-empty__scan" aria-hidden="true">
                                    <span class="enrichment-empty__scan-fill"></span>
                                </span>
                            })}
                        </li>
                    }).collect_view()}
                </ul>
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
