import { isIP } from "node:net";

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
  "skill",
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
  --check-updates            Check npm for a newer Architext package.
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
    serveList: false,
    serveRestart: false,
    serveInstance: "",
    checkUpdates: false,
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
    } else if (arg === "--list") {
      options.command = "serve";
      options.serveList = true;
    } else if (arg === "--instance") {
      assertServeCommand(options.command, arg);
      const value = rest[++index];
      if (!value) throw new Error("--instance requires a value");
      options.serveInstance = value;
    } else if (arg === "--restart" || arg === "--refresh" || arg === "--update") {
      assertServeCommand(options.command, arg);
      options.serveRestart = true;
    } else if (arg === "--check-updates") {
      options.command = "version";
      options.checkUpdates = true;
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
  const lifecycleOptions = [options.serveStatus, options.serveStop, options.serveList, options.serveRestart].filter(Boolean).length;
  if (lifecycleOptions > 1) throw new Error("--status, --stop, --list, and --restart cannot be used together");
  if (lifecycleOptions && (options.foreground || options.background || options.open || options.noOpen)) {
    throw new Error("--status, --stop, --list, and --restart cannot be combined with serve startup options");
  }
  if (options.serveInstance && !(options.serveStatus || options.serveStop || options.serveList || options.serveRestart)) {
    throw new Error("--instance requires --status, --stop, --list, or --restart");
  }
  if (!options.host) throw new Error("--host requires a value");
  if (!isLoopbackHost(options.host)) {
    throw new Error("--host must be a loopback address: localhost, 127.0.0.1, or ::1");
  }
  if (!Number.isInteger(options.port) || options.port < 0 || options.port > 65535) {
    throw new Error("--port must be an integer between 0 and 65535");
  }
}

export function isLoopbackHost(host) {
  const normalized = host.toLowerCase().replace(/^\[(.*)\]$/, "$1");
  if (normalized === "localhost" || normalized === "::1") return true;
  if (isIP(normalized) === 4) return normalized.startsWith("127.");
  return false;
}

function assertServeCommand(command, arg) {
  if (command !== "serve") throw new Error(`${arg} is only valid for architext serve`);
}
