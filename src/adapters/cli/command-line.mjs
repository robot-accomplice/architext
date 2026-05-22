const knownCommands = new Set([
  "install",
  "upgrade",
  "sync",
  "migrate",
  "doctor",
  "status",
  "serve",
  "validate",
  "build",
  "prompt",
  "clean",
  "explain",
  "help",
  "version"
]);

export function usage() {
  return `Usage:
  architext <command> [path] [options]

Path:
  [path] is optional and defaults to the current directory.
  Use it to manage another repository, for example:
    architext serve /path/to/repo

Commands:
  sync | install | upgrade   Install data-only Architext or migrate old copied installs.
  migrate                    Alias for sync, intended for old copied installs.
  doctor                     Diagnose installation health and optionally repair drift.
  status                     Print installation status. Use --json for machine output.
  serve                      Run the package-owned local viewer for a target repo.
  validate                   Validate target Architext JSON with package-owned schemas.
  build                      Build a static viewer into docs/architext/dist by default.
  prompt                     Print an LLM maintenance prompt.
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
  --open                     Open the local viewer in the system browser.
  --no-open                  Do not open the system browser.
  --host <host>              Serve bind host. Defaults to 127.0.0.1.
  --port <port>              Serve bind port. Defaults to 4317.
  --status                   Show the recorded background serve process.
  --stop                     Stop the recorded background serve process.
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
  architext serve --status
  architext serve --stop
  architext serve --host 127.0.0.1 --port 4517
  architext --version
  architext validate .
  architext doctor .
  architext status . --json
  architext sync . --dry-run
  architext sync . --yes --branch current
  architext build . --out docs/architext/dist
  architext prompt . --mode architecture-change

Target repository ownership:
  Target repos should commit docs/architext/data/*.json,
  docs/architext/.architext.json, and optional AGENTS.md or CLAUDE.md guidance.
  Do not copy or edit package-owned viewer, schema, tool, package, or Vite files
  inside target repositories.
  doctor/sync can migrate deterministic AGENTS, CLAUDE, Cursor, and .cursorrules
  project rules into docs/architext/data/rules.json.`;
}

export function parseArgs(argv) {
  const first = argv[0];
  const hasCommand = first && !first.startsWith("--") && knownCommands.has(first);
  const command = hasCommand ? first : "sync";
  const rest = hasCommand ? argv.slice(1) : argv;
  const options = {
    command,
    target: "",
    topic: "",
    yes: false,
    quiet: false,
    prompt: false,
    foreground: false,
    background: false,
    open: false,
    noOpen: false,
    host: "127.0.0.1",
    port: 4317,
    serveStatus: false,
    serveStop: false,
    json: false,
    dryRun: false,
    force: false,
    overwriteData: false,
    appendAgents: false,
    noAgents: false,
    rootScripts: false,
    noRootScripts: false,
    updateGitignore: false,
    noGitignore: false,
    mode: "initial-buildout",
    out: "",
    skipValidate: false,
    nodeModules: false,
    branch: "",
    branchName: ""
  };

  for (let index = 0; index < rest.length; index += 1) {
    const arg = rest[index];
    if (arg === "--target") options.target = rest[++index] ?? "";
    else if (arg === "--yes" || arg === "-y") options.yes = true;
    else if (arg === "--quiet") options.quiet = true;
    else if (arg === "--prompt") options.prompt = true;
    else if (arg === "--foreground") {
      assertServeCommand(options.command, arg);
      options.foreground = true;
    } else if (arg === "--background") {
      assertServeCommand(options.command, arg);
      options.background = true;
    } else if (arg === "--open") {
      assertServeCommand(options.command, arg);
      options.open = true;
    } else if (arg === "--no-open") {
      assertServeCommand(options.command, arg);
      options.noOpen = true;
    } else if (arg === "--host") {
      assertServeCommand(options.command, arg);
      options.host = rest[++index] ?? "";
    } else if (arg === "--port") {
      assertServeCommand(options.command, arg);
      options.port = Number(rest[++index] ?? "");
    } else if (arg === "--status") {
      assertServeCommand(options.command, arg);
      options.serveStatus = true;
    } else if (arg === "--stop") {
      assertServeCommand(options.command, arg);
      options.serveStop = true;
    } else if (arg === "--json") options.json = true;
    else if (arg === "--dry-run") options.dryRun = true;
    else if (arg === "--force") options.force = true;
    else if (arg === "--overwrite-data") options.overwriteData = true;
    else if (arg === "--append-agents") options.appendAgents = true;
    else if (arg === "--no-agents") options.noAgents = true;
    else if (arg === "--root-scripts") options.rootScripts = true;
    else if (arg === "--no-root-scripts") options.noRootScripts = true;
    else if (arg === "--update-gitignore") options.updateGitignore = true;
    else if (arg === "--no-gitignore") options.noGitignore = true;
    else if (arg === "--mode") options.mode = rest[++index] ?? "";
    else if (arg === "--out") options.out = rest[++index] ?? "";
    else if (arg === "--skip-validate") options.skipValidate = true;
    else if (arg === "--node-modules") options.nodeModules = true;
    else if (arg === "--branch") options.branch = rest[++index] ?? "";
    else if (arg === "--branch-name") options.branchName = rest[++index] ?? "";
    else if (arg === "--help" || arg === "-h") options.command = "help";
    else if (arg === "--version" || arg === "-v") options.command = "version";
    else if (options.command === "explain" && !options.topic) options.topic = arg;
    else if (!options.target) options.target = arg;
    else throw new Error(`Unknown argument: ${arg}`);
  }

  validateOptions(options);
  return options;
}

function validateOptions(options) {
  if (options.command !== "serve") return;
  if (options.foreground && options.background) throw new Error("--foreground and --background cannot be used together");
  if (options.open && options.noOpen) throw new Error("--open and --no-open cannot be used together");
  if (options.serveStatus && options.serveStop) throw new Error("--status and --stop cannot be used together");
  if ((options.serveStatus || options.serveStop) && (options.foreground || options.background || options.open || options.noOpen)) {
    throw new Error("--status and --stop cannot be combined with serve startup options");
  }
  if (!options.host) throw new Error("--host requires a value");
  if (!Number.isInteger(options.port) || options.port < 1 || options.port > 65535) {
    throw new Error("--port must be an integer between 1 and 65535");
  }
}

function assertServeCommand(command, arg) {
  if (command !== "serve") throw new Error(`${arg} is only valid for architext serve`);
}
