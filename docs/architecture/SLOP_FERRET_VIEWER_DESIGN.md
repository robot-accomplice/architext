# Slop Ferret viewer integration — design

**Date:** 2026-08-04 · **Status:** approved · **Scope:** Architext viewer and CLI support for displaying a slop-ferret sweep record alongside the existing architecture data.

## 1. What this is

[Slop Ferret](https://github.com/robot-accomplice/slop-ferret) audits a codebase for work that looks finished and is not. It emits several artifacts; the one a human reads is the self-contained HTML report, and the durable record it writes is a small JSON file in `~/.slop-ferret/records/`. Architext already visualizes Magma's code graph as an optional enrichment. This design adds the same treatment for slop-ferret: a project-owned JSON snapshot that Architext loads, validates, and renders as a first-class viewer mode.

The integration is deliberately read-only in the viewer. Judgement — which findings are verified, what refuted the near-misses — stays in the skill and the agent that ran the sweep. Architext only visualizes the artefact.

## 2. Decisions

| # | Decision | Rationale |
|---|---|---|
| D1 | **Project-owned snapshot, like `code-graph.json`.** The file lives under `docs/architext/data/slop-ferret.json` and is registered in `manifest.json` under the logical key `slopFerret`. | Keeps Architext self-contained and reproducible; matches the existing Code Graph pattern; does not tie the viewer to slop-ferret's local record-store layout. |
| D2 | **The snapshot combines the sweep Record and the findings list.** The slop-ferret Record carries coverage fractions and provenance but not the actual findings. The authored `findings.json` carries the findings but not the computed accounting. The Architext document merges both so the viewer has one fetch and one schema. | A single file is simpler to validate, review, and commit. |
| D3 | **Refresh is a CLI command, not a viewer mutation.** `architext slop-ferret [path] --plan plan.json --discharge discharge.json --findings findings.json` runs `ferret enumerate` and writes the snapshot. | Avoids running an external binary from the axum server, keeps mutation-token scope unchanged, and fits the existing CLI-first pattern for data generation (`architext build`, `architext sync`). |
| D4 | **Initial slice is Approach A: coverage banner + findings table + work queue summary.** Filtering, master/detail, and cross-links are deferred to a follow-up slice (Approach B). | Establishes the data contract and mode first; the file format does not change when the UI gets richer. |
| D5 | **Update slop-ferret documentation to mention the integration.** The slop-ferret README and architecture docs already describe the three-tool seam with magma and architext; this design extends that prose to say the sweep output can be viewed inside Architext. | Keeps the two repos honest about the contract. |

## 3. Scope

In scope for this design:
- A new `Mode::SlopFerret` in the Architext viewer.
- A JSON schema for `slop-ferret.json` under `viewer/schema/`.
- Rust serde models and manifest-driven loading in `architext-viewer`.
- A new CLI command in `architext-cli` that bundles slop-ferret outputs into the snapshot.
- A read-only viewer panel showing coverage, findings, and work-queue state.
- Documentation updates in the slop-ferret repo.

Out of scope for this slice (future work):
- Viewer-side filtering, sorting, or master/detail (Approach B).
- Cross-linking finding file paths to Repo Tree.
- Running `ferret plan` or the sweep itself from Architext.
- Serving slop-ferret data from `~/.slop-ferret/records/` directly.

## 4. Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  agent runs sweep → plan.json + discharge.json + findings.json │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│  architext slop-ferret [path]                               │
│    ├─ runs ferret enumerate plan.json discharge.json        │
│    ├─ builds slop-ferret.json (Record + Findings)           │
│    ├─ writes docs/architext/data/slop-ferret.json           │
│    └─ registers slopFerret in manifest.json                 │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│  architext serve / architext-serve                          │
│    ├─ serves /data/slop-ferret.json                         │
│    └─ serves manifest.json                                  │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│  architext-viewer                                           │
│    ├─ loads slop-ferret.json from manifest                  │
│    └─ renders Mode::SlopFerret panel                        │
└─────────────────────────────────────────────────────────────┘
```

## 5. Data contract

### 5.1 `docs/architext/data/manifest.json`

The manifest gains an optional logical file entry:

```json
{
  "files": {
    "slopFerret": "slop-ferret.json"
  }
}
```

The path is project-owned; `architext slop-ferret` uses the default `docs/architext/data/slop-ferret.json` and updates the manifest only when the key is missing.

### 5.2 `slop-ferret.json`

The document is a strict superset of the slop-ferret Record with an added `findings` array. Field names mirror the slop-ferret wire format (snake_case) because the producer is a Go tool and reusing its names keeps the CLI transform mechanical.

Top-level shape:

```json
{
  "schema": 1,
  "origin": "github.com/robot-accomplice/slop-ferret",
  "root_commit": "abc1234...",
  "identity_method": "root-commit",
  "sha": "abc1234",
  "date": "2026-08-04",
  "attested_repo": "17/25",
  "attested_plan": "10/10",
  "denominator": 25,
  "waived": 2,
  "worklist_size": 10,
  "unmatched_size": 15,
  "accounting": "complete",
  "vocab_provenance": {
    "lexicon": "~/.claude/skills/slop-ferret/references/ai-slop-lexicon.md",
    "lexicon_version": "2026-08-04.1",
    "signals_total": "12",
    "signals_from_lexicon": "9",
    "signals_from_repo": "3"
  },
  "tier": "1-2",
  "families_not_run": ["E"],
  "checked_clean": [
    { "class": "H · latent defect", "method": "read every h_required path" }
  ],
  "near_misses": [
    "internal/bridge/bridge.go — looked like unreachable HTTP client, but is called via reflection in tests"
  ],
  "findings_verified": 3,
  "findings_suspected": 1,
  "report_path": "/path/to/report.html",
  "findings": [
    {
      "title": "Unauthenticated localhost HTTP server",
      "file": "internal/bridge/bridge.go",
      "class": "H · latent defect",
      "severity": "blocking",
      "status": "VERIFIED",
      "claim": "The bridge accepts arbitrary outbound fetches from localhost with no auth.",
      "refutation": "A test calling it from a non-localhost address would fail; no such test exists.",
      "bar": "reproduce RED: drive the real function at its real call-site shape",
      "evidence": "net/http.ListenAndServe on :0, handler makes http.Get(url) where url comes from the request body.",
      "remediation": "Validate caller identity or bind to a unix socket owned by the parent process.",
      "occurrences": 1
    }
  ]
}
```

Required fields: `schema`, `sha`, `date`, `attested_repo`, `attested_plan`, `denominator`, `accounting`, `findings`.

`findings` may be empty; a clean sweep is a valid document.

## 6. Components

### 6.1 `architext-cli`

New command module: `crates/architext-cli/src/commands/slop_ferret.rs`.

Responsibilities:
1. Resolve the target path (default `.`).
2. Locate `docs/architext/data/manifest.json`.
3. Read the three input files from explicit CLI flags (`--plan`, `--discharge`, `--findings`).
4. Validate that `ferret` is on `PATH`.
5. Run `ferret enumerate <plan> <discharge> <repo>` to compute the `Result`.
6. Construct the snapshot JSON by merging the Record fields with `findings`.
7. Write `docs/architext/data/slop-ferret.json`.
8. Add or update the `slopFerret` entry in `manifest.json`.

The command exits with the same code as `ferret enumerate` so scripts can still gate on open items, but it writes the file before exiting so partial data is available in the viewer.

### 6.2 `architext-viewer`

New / modified files:

- `crates/architext-viewer/src/data/models.rs` — add `SlopFerret` and `SlopFerretFinding` structs.
- `crates/architext-viewer/src/data/fetch.rs` — load `slopFerret` from manifest, non-fatal like `codeGraph`.
- `crates/architext-viewer/src/theme.rs` — add `Mode::SlopFerret` with id `"slop-ferret"` and label `"Slop Ferret"`.
- `crates/architext-viewer/src/components/mode_icon.rs` — add a ferret-hunt line icon.
- `crates/architext-viewer/src/components/canvas_panel.rs` — render `SlopFerretPanel` when mode is active.
- `crates/architext-viewer/src/components/slop_ferret_panel.rs` — new panel component.

### 6.3 `viewer/schema/slop-ferret.json`

A JSON Schema describing the document. The Architext validator will use it when `slopFerret` is registered in the manifest.

### 6.4 `slop-ferret` documentation

Update:
- `README.md` — in the "Symbiosis: magma, architext, slop-ferret" section, note that the sweep output can be viewed in Architext and link to the Architext docs.
- `docs/architecture/dataflow.md` — extend the dataflow diagram to show the Architext consume path.

## 7. Data flow

1. The user runs the sweep and produces `plan.json`, `discharge.json`, and `findings.json`.
2. The user runs `architext slop-ferret . --plan plan.json --discharge discharge.json --findings findings.json`.
3. The CLI resolves the repository root and the Architext data directory.
4. The CLI invokes `ferret enumerate plan.json discharge.json <repo>` as a subprocess.
5. The CLI reads the plan and discharge itself to build the Record half of the snapshot (or, if slop-ferret later exposes a machine-readable record writer, uses that).
6. The CLI reads `findings.json` and appends the findings array.
7. The CLI writes `docs/architext/data/slop-ferret.json`.
8. The CLI updates `manifest.json` to include `"slopFerret": "slop-ferret.json"` if absent.
9. On `architext serve`, the viewer fetches `/data/slop-ferret.json` and renders the mode.

## 8. Error handling

| Scenario | Behaviour |
|---|---|
| `ferret` binary not found | Print an error naming the binary and how to install it; do not write a partial file. |
| Input files missing or unreadable | Print the path and the OS error; exit 2. |
| `ferret enumerate` exits 3 (items open) | Write the snapshot anyway, but print the remaining-items summary and exit 3. The viewer will show `accounting: incomplete`. |
| `ferret enumerate` exits 4 (refused) | Do not write the snapshot; print the refusal reason; exit 4. |
| `findings.json` has unknown fields | Refuse, matching slop-ferret's own strict parsing. |
| `slop-ferret.json` is malformed in the viewer | Render a non-fatal error panel, identical to the Code Graph refusal panel. |

## 9. Testing

- **CLI tests** in `crates/architext-cli/tests/`: run the command against committed fixtures (`plan.json`, `discharge.json`, `findings.json`) and assert the produced `slop-ferret.json` matches a golden file.
- **Schema test**: validate the golden file against `viewer/schema/slop-ferret.json`.
- **Viewer tests**:
  - `Mode::SlopFerret` has a label, id, and icon.
  - `SlopFerretPanel` renders findings severity-first and shows coverage fractions.
  - Malformed data renders an error surface, not a blank panel.
- **End-to-end**: run `architext validate .` on a fixture project that includes `slop-ferret.json`.
- **Doc check**: ensure the slop-ferret README references Architext after the change.

## 10. Future work

The following are explicitly deferred but designed for:

- **Filtering and detail (Approach B):** severity/status/family filters, master/detail layout, near-misses and checked-clean sections.
- **Cross-links:** finding file paths hyperlink to Repo Tree file preview.
- **Auto-discovery:** optionally read the newest local record from `~/.slop-ferret/records/` instead of requiring explicit input files.
- **Historical trend:** if multiple sweep records are committed over time, plot finding counts and coverage fractions like the Release Truth trend chart.

None of these require changing the `slop-ferret.json` contract introduced here.

## 11. Branching and release flow

This work touches two repositories:

- **`architext`**: branch `feature/slop-ferret-viewer` from `develop`. Open a PR into `develop`.
- **`slop-ferret`**: branch `feature/architext-docs` from `develop`. Open a PR into `develop`.

Both follow the modified Gitflow described in `CONTRIBUTING.md`: feature branches merge to `develop`; `develop` merges to `main` only when cutting a release.
