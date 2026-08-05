# CLAUDE.md

Guidance for AI assistants working **on the Architext tool itself** (this
repository). For working on architecture data *inside another repo that uses
Architext*, the **Architext Architecture Documentation** section at the bottom
of this file is the project-consumer contract.

> [!IMPORTANT]
> Two things in this file are **managed by `architext` itself** — keep all
> hand-written contributor guidance written as **prose and tables, not bulleted
> lists**, and leave the managed sections alone:
>
> The **Architext Architecture Documentation** section (the `##` heading near the
> end of this file) is upserted by `architext sync`
> (`crates/architext-cli/src/commands/sync/instruction_files.rs`); it replaces
> everything from that `##` heading to the next `##` heading. Edit the `APPENDIX`
> const in that file and `viewer/AGENTS_APPENDIX.md` — never the rendered block.
>
> `architext doctor`/`sync` also runs an **instruction-rule migration**
> (`crates/architext-core/src/domain/instruction_rules.rs`): any Markdown bullet
> or numbered list item of 16+ characters in this file gets extracted into
> `docs/architext/data/rules.json` and a pointer section is appended. Tables and
> prose are left untouched. This guide is intentionally bullet-free so `doctor`
> leaves it alone and does not flood the curated `rules.json`. Keep it that way.
>
> `AGENTS.md` is kept byte-identical to this file.

## What Architext Is

Architext is a **local, project-owned architecture / release / rules / dataflow
viewer** generated from strict JSON. Target projects own only JSON data (under
`docs/architext/data/`); the viewer, schema, and tooling are owned by the
globally installed `architext` package. The machine-readable model is the source
of truth, and the rendered site is a projection for humans.

Distribution is **native-only** (the 1.7.0 cutover removed a previous JS CLI;
there is no Node runtime and no npm channel). Architext ships as a single
self-contained Rust binary with the WASM viewer embedded, delivered via
`install.sh`, GitHub-release binaries, and `architext update`. See `README.md`
for the product framing and `ROADMAP.md` for direction.

## Tech Stack

The entire CLI, serve, validation, and routing surface is **Rust** (edition
2021, resolver 2). The viewer is a **Leptos 0.6 CSR app compiled to WASM** and
built with **Trunk** — there is no Node, bundler, or JS toolchain in the build.
The local server is **axum + tokio**. Data validation uses the `jsonschema`
crate. **`just`** is the task runner and **GitHub Actions** runs CI and release.

## Workspace Layout

Five crates live in `crates/`. All carry `version = "0.0.0"` and
`publish = false`; the real product version lives in `VERSION` / `package.json`
and is stamped into the binary by `architext-cli/build.rs`.

| Crate | Role |
| --- | --- |
| `architext-routing` | Deterministic diagram/route layout engine, and **the single source of truth for routing**. Compiled both native (`features = ["native"]`, used by serve) and to WASM (`features = ["wasm"]`, used by the viewer). Holds the `plan()` boundary, route geometry, C4 layout, decision branches, and sequence framing. |
| `architext-core` | Schema + cross-file reference **validation**, domain logic (rules, notes, releases, C4 quality, doctor repairs, sync plan, schema migration, instruction-rule migration), and order-preserving JSON writing. Schemas are embedded with `include_dir` so an installed binary validates with no on-disk schema files. |
| `architext-serve` | axum HTTP serve adapter: static viewer plus `/data/*`, read endpoints (`/api/status`, `/api/config`, `/api/repo-tree`, `/api/file`, `/api/plan/{hash}`) and mutation endpoints (`/api/rules`, `/api/notes`, `/api/config`, `/api/release-plans`, `/api/doctor`, `/api/sync-repair`) behind loopback-only + mutation-token security, plus `/api/data-events` SSE live-reload. Embeds the Trunk viewer dist via `rust-embed`. |
| `architext-cli` | The `architext` binary. `main.rs` is pure argv routing; logic lives in `commands::*` (`sync`, `serve`, `validate`, `doctor`, `status`, `build`, `update`, `prompt`, `skill`, `explain`, `clean`). |
| `architext-viewer` | Leptos CSR WASM app. **wasm32-only**, so it is excluded from the workspace `default-members` to keep native `cargo build --workspace` green. Module split: `data` (serde + fetch), `state` (signals), `selection`, `diagram` (in-process plan + SVG), `components` (one per file), `theme`. |

The viewer dist must exist before `architext-serve` compiles — release builds
bake it in via `rust-embed`, while debug builds read the folder at runtime. CI
builds the viewer once and then the CLI.

## Repository Map

| Path | Contents |
| --- | --- |
| `crates/` | The five Rust crates described above. |
| `viewer/` | **Package-owned** schema and canonical policy docs (`schema/*.json`, `LLM_ARCHITEXT.md`, `AGENTS_APPENDIX.md`, `DESIGN.md`). The `APPENDIX` const in the sync code mirrors `viewer/AGENTS_APPENDIX.md`. |
| `docs/architecture/` | Design specs and rubrics (`LLM_ARCHITEXT.md`, `ROUTING_*`, `SERVE_*`, `RELEASE_*`, `C4_DOCUMENTATION_RUBRIC.md`, …) plus per-release success criteria. Background, not a build input — treat as unverified hints, not truth. |
| `docs/architext/data/` | **Architext dogfooding itself**: this repo's own architecture model (`nodes`, `flows`, `views`, `decisions`, `risks`, `rules`, `roadmap`, `releases/`, `manifest.json`). Edit per the managed contract below. |
| `skills/architext/` | The published Claude Code skill (`SKILL.md` plus agents). |
| `.claude-plugin/`, `.codex-plugin/` | Plugin / marketplace manifests. |
| `packaging/`, `install.sh`, `about.toml`, `about.hbs`, `THIRD_PARTY_NOTICES.md` | Distribution and license-attribution artifacts. |
| `test/fixtures/` and each crate's `tests/` | Fixtures (corpus, routing-corpus) and integration tests. |
| Root `*.png` | README screenshots, regenerated manually from Architext's own served data. |

## Build, Test & Dev Commands

Run the full local release gate before opening a PR with `just release-check`
(validates the Architext data and builds the WASM viewer; native-only). The core
checks CI runs are Rust only, with **no Node or npm**: `cargo test --workspace`;
`cargo test -p architext-routing --test corpus_fitness` (the routing fitness and
perf ratchet); `cargo run -p architext-cli -- validate .`; and
`trunk build --release --config crates/architext-viewer/Trunk.toml`.

Other `just` recipes worth knowing: bare `just` lists everything, `just ci` /
`just ci-for <sha>` show CI status, and `just github-release` /
`just release-dry-run <version>` drive releases. The viewer WASM crate does
**not** build on native targets — always target `wasm32-unknown-unknown` (or use
Trunk) for it, and never add it to a plain native `cargo build`.

Run the CLI from source with `cargo run -p architext-cli -- <command> [path]`
(for example `serve`, `validate .`, `doctor .`, or `status --json`). The serve
layer has smoke scripts at `scripts/rust-serve-smoke.sh` and
`scripts/rust-serve-embedded-smoke.sh`.

## CI & Release

`.github/workflows/ci.yml` runs on pull requests and on pushes to `main` and
`develop`; it runs the workspace tests plus the routing corpus fitness/perf
ratchet, and PRs must be green to merge. `release-binaries.yml` cross-compiles
the five platform binaries (viewer embedded) and stops at artifacts.
`publish.yml` (the Release workflow) fires when a GitHub release is published: it
builds and attaches the native binaries plus a `SHA256SUMS` manifest that
`install.sh` and `architext update` download. A `dry_run=true` dispatch builds
everything without attaching anything.

## Contribution Workflow

`CONTRIBUTING.md` is authoritative. Architext uses a modified Gitflow: `main` is
the release branch (a PR into `main` signals a release) and `develop` is the
integration branch for the next release. Normal work branches from `develop` as
`feature/<short-name>` or `fix/<short-name>`; urgent production fixes branch from
`main` as `hotfix/<short-name>` and **must** be backmerged into `develop`.
Architecture and LLM-instruction updates land **before** code when the system
shape or behavior changes. Keep diffs scoped to the branch purpose, include
tests or validation evidence, and run `just release-check` before opening the
PR. (For automated sessions, follow any branch the task assigns instead.)

## Key Conventions

**Code is the source of truth.** Existing prose docs (including
`docs/architecture/*` and prior Architext claims) may be stale; verify against
code, and when they disagree, the code wins. **Routing logic lives once**, in
`architext-routing`, serving both native and WASM consumers — do not fork layout
logic into the viewer. **Determinism matters**: keep `Math.random` and
wall-clock out of routing, and respect the corpus fitness ratchet
(`corpus_fitness.rs`) that gates routing-quality regressions. **The native
binary is self-contained**: schemas (`include_dir`) and the viewer dist
(`rust-embed`) are embedded, so avoid reintroducing repo-relative runtime file
dependencies, and prefer pure-Rust dependencies (rustls, fancy-regex) over
anything needing a C toolchain. **There is no Node or npm** anywhere in the
build, test, or distribution path. When this repo's own architecture changes,
update its Architext data under `docs/architext/data/` and re-validate, per the
managed contract below.

## Architext Architecture Documentation

This project uses `docs/architext/data/**/*.json` as the machine-readable
architecture and release source of truth.

Derive what you record in those JSON files from the **source code only**.
Existing architecture documentation — prose READMEs, design docs, diagrams,
comments, and even prior Architext claims — may be stale, aspirational, or
wrong; do not treat any of it as authoritative for what the system actually is.
Read the code to determine real responsibilities, flows, data movement,
dependencies, and trust boundaries. Treat existing documents as unverified
hints at most, verify every claim against the code, and when code and a
document disagree, the code wins. (This governs the architecture you record;
this contract and the schema still govern how you record it.)

`docs/architext/data/manifest.json` records the Architext data schema version.
That version tracks the JSON data contract, not the installed CLI/package
version. Additive schema changes may ship in minor releases; breaking schema
changes require a major semver release and an Architext-managed migration path.

When changing architecture, data flow, persistence, external integrations, trust
boundaries, deployment topology, observability paths, or major module
responsibilities, update the relevant Architext JSON files before completing the
task.

When release scope, blockers, milestones, posture, evidence, or target dates
change, update Release Truth data under `docs/architext/data/releases/`.
Release Truth is the reviewed release source of truth: completed work,
deferrals, reprioritization, blockers, dependencies, and next actions belong in
the release detail file, with `releases/index.json` refreshed from those facts.
Keep Release Path labels concise and put long context in the selected release
item's detail data.

When planning a future release, use `docs/architext/data/roadmap.json` as the
roadmap source and Release Planning as the approval boundary. Selected roadmap
items keep `source: "roadmap"`; manually entered scope uses `source:
"ad-hoc"` and should be promoted into `roadmap.json` when the plan is approved.
Do not represent unreviewed planning proposals as current Release Truth facts.

When project rules change, update `docs/architext/data/rules.json`.
Categories are maintainer-defined classifications such as Architecture,
Development, Design, Release, or any project-specific grouping. Respect
`protection.edit` and `protection.delete`; protected rules are not casual
cleanup targets. Rank rules by `criticality` and `order`, not alphabetical
order or creation time.

Element notes are human annotations on an architecture element (node, flow,
decision, risk, view, or data class), persisted in the optional
`docs/architext/data/notes.json` and registered as `manifest.files.notes`.
Each note records `target: { kind, id }`, a `category`
(`note` | `mitigation` | `caveat` | `todo`), a `body`, and timestamps; the
note's `target.id` must reference an existing element (validation enforces
this). Notes capture maintainer judgement — for example, that a high-risk
area is intentionally mitigated by the documented system — so treat them as
user-owned: preserve and update them, but do not fabricate notes or delete a
human's note as cleanup. They are edited in the viewer (the detail panel's
Notes section) and never replace validation or recorded architecture facts.

When ordered work or use-case paths deserve a dedicated Flows projection, add a
`workflow` view in `docs/architext/data/views.json`. Workflow views should reuse
existing nodes and ordered flows; do not duplicate flow facts or invent
workflow-specific routing rules.

Keep flow diagrams free of orphaned elements. Every rendered node, edge, marker,
and label must be traceable to the selected flow, a selected supporting
relationship, or an explicit context relationship shown in the projection.
Remove disconnected context, connect it with a labeled relationship, or split it
into a separate view; do not leave loose boxes, endpoints, markers, or labels
for the reader to interpret. Prefer semantic iconography over UML/code diagrams
or broad flowchart shape palettes for flow enrichment. Mark decision, start,
stop, async, persistence, artifact, return, and process semantics with
`step.kind` when the flow needs them. For decision branches, set `step.outcome`
to the concrete branch/result label that should be readable on the path. A
decision branch should have at least two outgoing outcome steps from the
decision node, and those branch lines should share the decision step number. Do
not add UML/code diagrams for now.
For sequence diagrams, create explicit return paths
for request/response, command/result, event/acknowledgement, and failure-return
interactions when the flow requires them. Mark return steps with `kind:
"return"` and `returnOf` when they answer a specific outbound step. Use
`sequenceFrames` for loops, retries, optional branches, and transaction or
consistency blocks so outbound and return messages are visibly grouped instead
of implied.

For source extraction work, produce a reviewable draft of proposed JSON changes
with source paths and confidence notes before editing data files. Never replace
validation with extracted claims.

For C4 views, keep Context, Container, and Component diagrams at their proper
abstraction level. Prefer splitting dense views over forcing tangled routing,
keep relationship labels visible, and treat duplicate node membership in one
C4 view as a documentation defect to repair in `docs/architext/data/views.json`.
Use explicit `scopeNodeId` metadata to make C4 drilldown navigable: a Context
node that represents the system should have a scoped Container view, a
decomposable Container node should have a scoped Component view, and a
decomposable Component node should have a scoped Code view when code-level
documentation exists. If a node is external or intentionally outside the
project boundary, leave it without a child view so the viewer can explain that
drilldown is unavailable.

Run the Architext validator after edits:

```sh
architext validate [path]
```

Use the local viewer for review:

```sh
architext serve [path]
```

The optional path defaults to the current directory. Target repositories should
not vendor or edit Architext viewer, schema, tool, package, or Vite files.
Those are owned by the globally installed `architext` package. Edit project
architecture, roadmap, and release data under `docs/architext/data/**/*.json`;
use `architext sync [path]` to install or migrate lifecycle metadata and
instructions.

Use `architext doctor [path]` to inspect installation health, including C4
document quality issues, and `architext doctor [path] --yes` to apply
deterministic repairs. `architext sync [path]` runs the same doctor diagnostics
before converging lifecycle state. Use `architext prompt [path]` to print the
current agent build-out or maintenance instructions.
Do not claim the architecture documentation is current if validation fails or
was skipped.
