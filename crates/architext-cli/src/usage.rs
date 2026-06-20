//! `usage()` text for the CLI. (Originally a byte-identical port of the JS
//! command-line help; the JS CLI was removed at the 1.7.0 cutover, so this is
//! now the sole source of truth and can evolve with the Rust CLI.)

pub fn usage() -> &'static str {
    "Usage:
  architext <command> [path] [options]

Path:
  [path] is optional and defaults to the current directory.
  Use it to manage another repository, for example:
    architext serve /path/to/repo

Commands:
  sync | install             Install data-only Architext or migrate old copied installs.
  migrate                    Alias for sync, intended for old copied installs.
  update | upgrade           Update the architext binary to the latest release.
  doctor                     Diagnose installation health and optionally repair drift.
  status                     Print installation status. Use --json for machine output.
  serve                      Run the package-owned local viewer for a target repo.
  validate                   Validate target Architext JSON with package-owned schemas.
  build                      Build a static viewer into docs/architext/dist by default.
  prompt                     Print an LLM maintenance prompt.
  skill                      Print the Architext SKILL.md content for LLM skill creation.
  clean                      Remove generated local artifacts.
  explain [topic]            Explain schemas and data contracts.
  version                    Print the Architext package version.

Options:
  --target <repo>            Target repository. Positional [path] is preferred.
  --yes, -y                  Accept default prompts.
  --quiet                    Accept default sync prompts without interactive questions.
  --prompt                   Force sync prompts instead of offering saved answers.
  --foreground               Run serve in the current terminal until interrupted.
  --background               Run serve detached and return control after startup.
  --list                     List all recorded live serve instances.
  --instance <id>            Target a listed serve instance.
  --restart                  Sync and restart a recorded background serve instance.
  --refresh                  Alias for --restart.
  --update                   Alias for --restart. Use --check-updates for package updates.
  --check-updates            Report whether a newer Architext release is available.
  --open                     Open the local viewer in the system browser.
  --no-open                  Do not open the system browser.
  --host <host>              Serve bind host. Defaults to 127.0.0.1.
                              Must be localhost, 127.0.0.0/8, or ::1.
  --port <port>              Preferred serve port. Defaults to 4317; use 0 for an OS-assigned port.
  --status                   Show the recorded serve process.
  --stop                     Stop the recorded serve process.
  --json                     Machine-readable status/doctor output.
  --dry-run                  Show intended changes without writing files.
  --force                    Rerun lifecycle management even when current.
  --overwrite-data           Replace docs/architext/data/*.json with neutral starter data.
  --append-agents            Append or replace Architext sections in AGENTS.md and CLAUDE.md.
  --no-agents                Do not manage AGENTS.md or CLAUDE.md.
  --root-scripts             Add root package.json Architext convenience scripts.
  --no-root-scripts          Do not manage root package.json scripts.
  --update-gitignore         Add generated artifact ignores without prompting.
  --no-gitignore             Do not manage .gitignore.
  --mode <name>              Prompt mode: initial-buildout, architecture-change, repair-validation, source-extraction.
  --out <path>               Build output path. Defaults to docs/architext/dist.
  --skip-validate            Do not run validation after sync/migration.
  --branch current|new|none  Branch handling for mutating sync.
  --branch-name <name>       Branch name to use with --branch new.
  --version, -v              Print the Architext package version.

Examples:
  architext sync
  architext serve
  architext serve --foreground
  architext serve --open
  architext serve --background
  architext serve --background --open
  architext --list
  architext serve --list
  architext serve --status
  architext serve --stop
  architext serve --restart --instance <id>
  architext --check-updates
  architext update
  architext serve --host 127.0.0.1 --port 4517
  architext --version
  architext validate .
  architext doctor .
  architext status . --json
  architext sync . --dry-run
  architext sync . --yes --branch current
  architext build . --out docs/architext/dist
  architext prompt . --mode architecture-change
  architext skill

Target repository ownership:
  Target repos should commit only project-owned Architext state:
    docs/architext/data/*.json
    docs/architext/.architext.json
    optional AGENTS.md, CLAUDE.md, Cursor rule, or .cursorrules pointers
  Do not copy or edit package-owned viewer, schema, tool, package, Vite,
  TypeScript, public asset, README, or generated dependency files inside target
  repositories.
  doctor/sync can migrate deterministic AGENTS, CLAUDE, Cursor, and .cursorrules
  project rules into docs/architext/data/rules.json."
}
