# Architext Template

This directory is the package-owned Architext viewer, schema, and starter data.
Target repositories should not copy or edit this implementation. They own only
their `docs/architext/data/*.json` files, lifecycle metadata, and optional
repository-level agent instructions.

## Commands

From a target project root, use the global Architext CLI:

```sh
architext serve
architext validate
architext build
architext doctor
architext prompt
architext version
```

Install the CLI as a native binary (no Node runtime required):

```sh
curl -fsSL https://raw.githubusercontent.com/robot-accomplice/architext/main/install.sh | sh
```

Keep it current with `architext update`. See the repository README for manual
download and the off-ramp for older npm-bridge installs.

Each command also accepts an optional target path:

```sh
architext serve /path/to/project
architext validate /path/to/project
```

`architext serve` runs in the foreground by default for script compatibility.
Use `architext serve --background` to start a detached local viewer server,
`architext serve --open` to launch the system browser, `architext serve
--status` to inspect a recorded serve process, and `architext serve --stop`
to stop it.

Additional serve controls are `--foreground`, `--no-open`, `--host <host>`, and
`--port <port>`. The port is a preferred starting port; startup advances to the
next available loopback port when it is occupied. Serve process state is local
runtime state and is not part of target-owned Architext JSON data.

If you are developing Architext itself, the toolchain is Rust only — Cargo for
the CLI/engine and Trunk for the WASM viewer. No Node or npm.

Validate architecture data:

```sh
cargo run -p architext-cli -- validate .
```

Build the WASM viewer (embedded into the binary):

```sh
trunk build --release --config crates/architext-viewer/Trunk.toml
```

Serve a project locally (after building the viewer, or from an installed
binary that has it embedded):

```sh
cargo run -p architext-cli -- serve .
```

Run the workspace tests:

```sh
cargo test --workspace
```

## Upgrades

Target repositories are data-only in Architext 1.0+. From the target project
root, use the package CLI:

```sh
architext sync
```

The script detects whether Architext data is absent, current, or from an older
copied-template install. It prompts before writing files and can create a git
branch before making changes. Migration preserves `docs/architext/data/*.json`
by default, removes copied implementation files, updates lifecycle metadata,
and corrects Architext sections in model-specific instruction files.

The script can also maintain the target repository `.gitignore`. Generated
local artifacts, especially `docs/architext/dist/`, should be ignored.

The CLI writes lifecycle metadata to `.architext.json`. Keep that file with the
project so automation can report CLI version, update time, managed instruction
files, copied-install migration state, saved sync choices, and last validation
state. Later interactive syncs offer to reuse saved choices; pass `--prompt` to
ask the normal prompts again or `--quiet` to select the default choices without
asking.

## Management

Useful project-root commands:

```sh
architext doctor
architext status --json
architext prompt --mode architecture-change
architext skill
architext clean
architext clean --node-modules
architext explain nodes
```

`doctor` diagnoses stale or broken installs. Without `--yes`, it reports
available deterministic repairs and prompts before applying them. With `--yes`,
it applies those repairs directly. Repairs include converging model-specific
rule files such as `AGENTS.md`, `CLAUDE.md`, Cursor rule files, and
`.cursorrules` into the model-agnostic `data/rules.json` source of truth when
the rules can be migrated deterministically.

`skill` prints Architext's package-owned `SKILL.md` content so maintainers can
paste it into an LLM chat session when creating a model-specific skill.

## Viewer Data

The package-owned viewer can render and edit selected repository-owned data:

- diagrams from nodes, flows, views, data classes, decisions, and risks
- Release Truth and Release Planning data under `data/releases/`
- ranked project Rules under `data/rules.json`
- roadmap items under `data/roadmap.json`

Browser editors write JSON through the local CLI server and validate before
committing changes to disk. Target repositories still own the data; they do not
own the viewer implementation.

## Data Entry Point

The viewer loads:

```text
data/manifest.json
```

The manifest points to the remaining JSON files. Keep paths local to this
directory. Do not load runtime assets, schemas, scripts, styles, or fonts from
remote URLs.

## Validation

`architext validate` (or `cargo run -p architext-cli -- validate .`) performs
two checks:

- JSON Schema validation for each data file
- cross-reference validation for IDs shared across nodes, flows, views, risks,
  decisions, and data classifications

If validation fails, the architecture model is not trustworthy.
