#!/usr/bin/env node
// Build a sanitized, relabeled Architext review corpus from the (read-only)
// roboticus dataset. Deterministic + re-runnable: reads SRC, applies one
// consistent rename/scrub map, writes DEST.
//
// Domain: "FlowForge" — a generic SaaS workflow-automation platform. The
// roboticus runtime structure (CLI/TUI/dashboard/API surfaces, a central
// execution pipeline, scheduler, memory/context store, tool/connector
// execution, model/automation routing, release harness) maps onto an
// automation platform with the least distortion, so node/flow/view structure
// is preserved 1:1 and only identity-bearing labels + prose are changed.
//
// Strategy:
//  - Structure is preserved exactly (graph, edges, steps, frames, lanes, C4
//    levels, scope, release scope shapes, rule order/protection).
//  - A small set of identity-bearing IDS is remapped (bijective); every other
//    id is generic already and is kept verbatim so cross-refs stay valid by
//    construction. The remap is applied to BOTH definitions and every
//    reference field the validators check.
//  - Node NAMES get a curated use-case-descriptive map; all other names + all
//    PROSE strings run through an ordered token scrubber that removes
//    roboticus-specific proper nouns, file paths, URLs, hashes, and product
//    names, replacing them with generic FlowForge-domain equivalents.
//  - IDs, enum-constrained fields, dates, and numeric counts are never run
//    through the prose scrubber.

import fs from "node:fs";
import path from "node:path";

const SRC = "/Users/jmachen/code/roboticus/docs/architext/data";
const DEST = path.resolve(
  path.dirname(new URL(import.meta.url).pathname),
  "corpus"
);

// ---------------------------------------------------------------------------
// 1. ID remap. Most roboticus ids are already generic slugs and are kept
//    verbatim so cross-refs stay valid by construction. Identity-bearing ids
//    are rewritten via (a) explicit overrides, then (b) a deterministic
//    slug-token rewrite for any id that still contains a scrub token (e.g.
//    release-local ids like "practical-deepseek-media-gate"). The SAME
//    remapId() is applied everywhere an id appears (definitions + refs), so a
//    rewritten id stays consistent across the whole corpus.
// ---------------------------------------------------------------------------
const ID_OVERRIDES = {
  "roboticus-system": "flowforge-platform",
  "product-knowledge-service": "knowledge-base-service",
  "mechanic-subsystem": "self-healing-subsystem",
  "sqlite-store": "sql-store",
  "c4-context-roboticus": "c4-context-flowforge",
  "product-knowledge-retrieval": "knowledge-base-retrieval",
  "source-backed-product-knowledge": "source-backed-knowledge-base",
  "stale-product-knowledge": "stale-knowledge-base"
};

// Slug-safe token substitutions for ids (ordered, longest first). Distinct
// source tokens map to distinct targets so the rewrite stays injective.
const ID_SLUG_RULES = [
  [/product-knowledge/g, "knowledge-base"],
  [/roboticus/g, "flowforge"],
  [/deepseek/g, "cloud-provider"],
  [/ollama/g, "embedded-runtime"],
  [/telegram/g, "chat-connector"],
  [/whatsapp/g, "second-connector"],
  [/mechanic/g, "self-healing"],
  [/sqlite/g, "sql"],
  [/\bfts5\b/g, "fulltext"],
  [/\bhnsw\b/g, "vector-index"],
  [/x402/g, "metered-billing"],
  [/aave/g, "billing-plan"],
  [/openai/g, "provider-a"],
  [/anthropic/g, "provider-b"],
  [/\bgoogle\b/g, "provider-c"],
  [/claude/g, "ide"],
  [/reddit/g, "forum"],
  [/elevenlabs/g, "voice-vendor"],
  [/hipporag/g, "graph-rag"],
  [/huggingface/g, "model-hub"]
];

function remapId(id) {
  if (typeof id !== "string") return id;
  if (ID_OVERRIDES[id]) return ID_OVERRIDES[id];
  let out = id;
  for (const [re, rep] of ID_SLUG_RULES) out = out.replace(re, rep);
  return out;
}
const remapIdList = (list) =>
  Array.isArray(list) ? list.map(remapId) : list;

// ---------------------------------------------------------------------------
// 2. Curated node-name map (use-case descriptive, distinct & meaningful).
//    Only names that carry product identity or read awkwardly in the new
//    domain are remapped; the rest are already generic and kept verbatim
//    (still passed through the scrubber for token safety).
// ---------------------------------------------------------------------------
const NODE_NAME_MAP = {
  "roboticus-system": "FlowForge Platform",
  "external-channel-adapters": "Integration Connectors",
  "unified-pipeline": "Workflow Execution Engine",
  "llm-service": "Automation Engine and Rule Router",
  "external-model-providers": "External Automation Providers",
  "local-model-provider": "Embedded Automation Runtime",
  "memory-system": "Context Store",
  "agency-service": "Autonomy Orchestrator",
  "skill-plugin-system": "Plugin Marketplace",
  "mcp-system": "Tool Connector Hub",
  "media-services": "Document and Media Services",
  "mail-service": "Email Integration Service",
  "wallet-service": "Billing and Credits Service",
  "product-knowledge-service": "Knowledge Base Service",
  "test-release-harness": "Test and Release Harness",
  "sqlite-store": "Embedded SQL Store",
  "unified-pipeline-entry": "Workflow Execution Engine",
  "turn-policy-engine": "Run Policy and Trigger Engine",
  "context-builder": "Execution Context Builder",
  "tool-evidence-engine": "Tool Execution and Evidence Engine",
  "guard-verifier-stage": "Validation and Verification Stage",
  "post-turn-stage": "Post-Run Persistence and Telemetry Stage",
  "agency-policy-engine": "Autonomy Policy Engine",
  "agency-executor-component": "Autonomy Executor",
  "agency-audit-log": "Autonomy Audit Records",
  "media-device-inventory": "Document Source Inventory",
  "stt-service": "Document Ingestion Service",
  "tts-service": "Document Rendering Service",
  "camera-capture-service": "Attachment Capture Service",
  "vision-preprocessor": "Document Preprocessor",
  "llm-route-selector": "Automation Route Selector",
  "llm-request-sanitizer": "Automation Request Sanitizer",
  "llm-provider-circuit": "Provider Circuit and Fallback",
  "mechanic-subsystem": "Self-Healing Maintenance Subsystem",
  "pipeline-ingress": "Engine Ingress and Session Resolver"
};

// ---------------------------------------------------------------------------
// 3. Ordered token scrubber. Order matters (longest / most-specific first).
//    Case-aware: each entry rewrites the listed source spelling -> target.
//    Applied ONLY to free-text strings (names, summaries, prose), never to
//    ids / enums / dates.
// ---------------------------------------------------------------------------
const TOKEN_RULES = [
  // CamelCase identifiers embedding the product name (test names etc.)
  [/NoRoboticusAPI/g, "NoPlatformAPI"],
  [/Roboticus([A-Z])/g, "FlowForge$1"],
  // multi-word / specific phrases first
  [/Roboticus product knowledge/g, "FlowForge knowledge base"],
  [/Roboticus REST loopback/g, "platform REST loopback"],
  [/Roboticus setup/g, "platform setup"],
  [/the Roboticus runtime/g, "the platform runtime"],
  [/Roboticus runtime/g, "platform runtime"],
  [/Roboticus-specific/g, "platform-specific"],
  [/Roboticus product/g, "FlowForge product"],
  [/Roboticus/g, "FlowForge"],
  [/roboticus-dev/g, "flowforge-dev"],
  [/roboticus\.toml/g, "flowforge.toml"],
  [/roboticus/g, "flowforge"],
  // self-healing maintenance subsystem. Match the exact CamelCase token
  // "Mechanic" wherever it is a whole sub-word (followed by uppercase, "(",
  // end, or non-letter) so RunMechanic/MechanicReport are caught, while the
  // English words mechanical/mechanically/mechanics (lowercase continuation)
  // are intentionally left alone.
  [/Mechanic(?![a-z])/g, "SelfHealing"],
  [/\bmechanic\b/g, "self-healing"],
  // third-party model hubs / research systems
  [/HuggingFace/g, "ModelHub"],
  [/Hugging Face/g, "Model Hub"],
  [/huggingface/g, "model-hub"],
  [/HippoRAG/g, "GraphRAG"],
  [/hipporag/g, "graph-rag"],

  // model providers / runtimes -> generic automation-provider language
  [/DeepSeek/g, "the cloud provider"],
  [/deepseek/g, "cloud-provider"],
  [/\bOpenAI\b/g, "Provider A"],
  [/\bopenai\b/g, "provider-a"],
  [/\bAnthropic\b/g, "Provider B"],
  [/\banthropic\b/g, "provider-b"],
  [/\bClaude\b/g, "Provider B model"],
  [/\bGemini\b/g, "Provider C"],
  [/\bGPT\b/g, "the cloud model"],
  [/FormatOllama/g, "FormatEmbedded"],
  [/FormatOpenAI/g, "FormatProviderA"],
  [/FormatDeepSeek/g, "FormatCloud"],
  [/\bOllama\b/g, "Embedded Runtime"],
  [/\bollama\b/g, "embedded-runtime"],
  [/nomic-embed-text/g, "embed-model"],
  [/\bNomic\b/g, "Embed Model"],
  [/\bnomic\b/g, "embed-model"],
  // local model names -> generic local-model placeholders
  [/qwen[0-9.]*:[0-9a-z.-]+/gi, "local-model-a"],
  [/\bqwen[0-9.]*\b/gi, "local-model-a"],
  [/phi4[a-z0-9:.-]*/gi, "local-model-b"],
  [/\bphi4\b/gi, "local-model-b"],
  [/\bsglang\b/gi, "alt-runtime-1"],
  [/\bvllm\b/gi, "alt-runtime-2"],
  [/\bllama-cpp\b/gi, "alt-runtime-3"],
  [/docker-model-runner/gi, "container-runtime"],
  [/AI21-Jamba/g, "a strict-tooling model"],
  // env-var prefix
  [/ROBOTICUS_/g, "FLOWFORGE_"],
  // storage internals
  [/\bFTS5\b/g, "full-text search"],
  [/\bfts5\b/g, "full-text-search"],
  [/\bHNSW\b/g, "approximate vector index"],
  [/\bhnsw\b/g, "vector-index"],
  // logging / web libs
  [/\bzerolog\b/g, "the structured logger"],
  [/\bchi\b/g, "the HTTP router"],

  // channels / protocols -> generic connectors
  [/\bTelegram\b/g, "Chat Connector"],
  [/\btelegram\b/g, "chat-connector"],
  [/\bWhatsApp\b/g, "Second Connector"],
  [/\bwhatsapp\b/g, "second-connector"],
  [/\bIMAP\b/g, "inbound mail"],
  [/\bPOP3\b/g, "inbound mail"],
  [/\bSMTP\b/g, "outbound mail"],
  [/\bsmtp\b/g, "outbound-mail"],

  // wallet / payments -> billing & credits
  [/\bx402\b/gi, "metered-billing"],
  [/\bAave V3\b/g, "Billing Plan V3"],
  [/\bAave\b/gi, "billing-plan"],
  [/\bUSDC\b/g, "credit units"],
  [/\baToken\b/g, "credit token"],
  [/\bIPool\b/g, "IPlan"],
  [/BYO-wallet/g, "bring-your-own-billing"],
  [/\bwallet\b/g, "billing account"],
  [/\bWallet\b/g, "Billing account"],

  // chains -> generic regions/tenants (keep generic)
  [/\bEthereum\b/gi, "Region East"],
  [/\bPolygon\b/gi, "Region West"],
  [/\bCompound\b/g, "Billing Plan B"],
  [/\bSolana\b/gi, "Region North"],
  [/\bBase chain\b/gi, "default region"],

  // stacks / stores -> generic
  [/\bSQLite\b/g, "embedded SQL store"],
  [/\bsqlite\b/g, "embedded-sql-store"],
  [/\bcobra\b/gi, "the CLI framework"],
  [/golangci-lint and go test/g, "the linter and unit-test suite"],
  [/golangci-lint/g, "the linter"],
  [/\bgo test\b/g, "the unit-test suite"],
  [/\bGo process\b/g, "service process"],
  [/\bGo service[s]?\b/g, "platform service"],
  [/\bGo build-tagged\b/g, "build-tagged"],
  [/\bGo\b/g, "the platform"],
  [/\bIPC\b/g, "IPC"],

  // place names that slipped into prose
  [/\bGeneva\b/g, "the requested city"],

  // people / orgs
  [/robot@accomplice\.ch/g, "owner@example.com"],
  [/robot-accomplice/g, "example-org"],

  // module path roots (roboticus Go layout) -> generic module hints
  [/internal\/pipeline/g, "src/engine"],
  [/internal\/wallet/g, "src/billing"],
  [/internal\/core/g, "src/core"],
  [/internal\/configstate/g, "src/configstate"],
  [/internal\/update/g, "src/update"],
  [/pipeline\.RunPipeline/g, "engine.RunWorkflow"],
  [/RunPipeline/g, "RunWorkflow"],
  [/grounding_judge\.go/g, "validation_judge.ts"],
  [/yield\.go/g, "billing_plan.ts"],
  [/config_enums\.go/g, "config_enums.ts"],
  [/config_validation_financial\.go/g, "config_validation_billing.ts"],
  [/schema\.go/g, "schema.ts"],
  [/exercise\.go/g, "exercise.ts"],
  [/adversarial_soak_test\.go/g, "adversarial_soak_test.ts"],

  // external URLs/CDNs that became "evidence" in prose
  [/apis\.google\.com/g, "third-party-api.example.com"],
  [/Cross-Origin-Opener-Policy/g, "a transport header"],
  [/Report-To/g, "a transport header"],
  [/\bNEL\b/g, "a transport header"],
  [/weather icon CDN URL/g, "static asset URL"],

  // compound / camelCase / snake_case source symbols carrying proper nouns.
  // These run last among token rules (after the clean word-boundary forms) and
  // target the identifier spellings the boundary rules cannot reach.
  [/ollama[A-Za-z]*/g, "embeddedRuntimeAdapter"],
  [/openAICompatible/g, "providerACompatible"],
  [/[Ff]ormatter_telegram/g, "formatter_chat_connector"],
  [/TestWebhookTelegram[A-Za-z_]*/g, "TestWebhookChatConnector"],
  [/Test[A-Za-z]*Telegram[A-Za-z_]*/g, "TestChatConnector"],
  [/TestWebhookWhatsApp[A-Za-z_]*/g, "TestWebhookSecondConnector"],
  [/x402_payments/gi, "metered_billing_payments"],
  [/X402Handler/g, "MeteredBillingHandler"],
  [/activateX402Payments/g, "activateMeteredBillingPayments"],
  [/[xX]402[A-Za-z_]*/g, "meteredBilling"],
  [/copy_sqlite_snapshot/g, "copy_store_snapshot"],
  [/[A-Za-z_]*sqlite[A-Za-z_]*/gi, "store_snapshot"],
  [/ElevenLabs ElevenAgents/g, "Voice Vendor RealtimeAgents"],
  [/ElevenLabs/g, "Voice Vendor"],
  [/ElevenAgents/g, "RealtimeAgents"],
  [/elevenlabs/g, "voice-vendor"],
  // remaining third-party provider / search names -> consistent placeholders
  [/\bGoogle\b/g, "Provider C"],
  [/\bDuckDuckGo\b/g, "Search Engine B"],
  [/\bVenice\b/g, "Provider D"],
  // npm scope / owner identity
  [/@robotaccomplice/g, "@example-org"],
  [/robotaccomplice/g, "example-org"],
  [/arXiv:[0-9.]+/g, "a reference paper"],
  [/arxiv/gi, "reference-archive"],
  [/github\.com\/[A-Za-z0-9._\/-]+/g, "code-host.example.com/repo"],
  [/CLAUDE\.md/g, "AGENT_GUIDELINES.md"],
  [/Gortex/g, "an open-source library"],
  [/usdc_address/g, "credit_account"],
  [/BundledNomicGGUF/g, "BundledEmbedModel"],
  [/Nomic[A-Za-z]*/g, "EmbedModel"],
  [/HNSW[A-Za-z]*/g, "VectorIndex"],
  [/aave[A-Za-z]*/g, "billingPlan"],
  [/[A-Za-z_]*fts5[A-Za-z_]*/g, "fulltext_index"],
  [/sqlite_master/g, "sql_catalog"],
  [/sqlite3/g, "sqlcli"],
  [/SQLITE_BUSY/g, "STORE_BUSY"],
  [/ClaudeAI/g, "ForumAI"],
  [/claude-plugin/g, "ide-plugin"],
  [/claude_code/g, "ide_assistant"],
  [/claude\.ai/g, "assistant.example.com"],

  // version-control / vendor doc filenames -> generic
  [/v1\.0\.0-footprint\.md/g, "release-1.0.0-notes.md"],
  [/\.codex\//g, "docs/release-status/"],
  [/architecture_rules\.md/g, "ARCHITECTURE_RULES.md"],
  [/ARCHITECTURE\.md/g, "ARCHITECTURE.md"],
];

// Neutralize obviously-identifying dynamic tokens in prose:
//  - absolute /tmp/... and ~/.foo paths with embedded identifiers
//  - 32/40/64-char hex hashes (session / turn / sha)
function neutralizeDynamic(s) {
  let out = s;
  // sha256:<hex> -> sha256:<redacted>
  out = out.replace(/sha256:[0-9a-f]{40,64}/gi, "sha256:<redacted>");
  // bare long hex run-ids
  out = out.replace(/\b[0-9a-f]{32,64}\b/g, "<run-id>");
  // external URLs in prose -> generic placeholder (keep our own example.com refs)
  out = out.replace(/https?:\/\/[^\s")]+/g, (m) =>
    /example\.com/.test(m) ? m : "https://reference.example.com"
  );
  // absolute user paths (Desktop / Users / home) with embedded artifact names
  out = out.replace(/\/Users\/[^\s",]+/g, "/home/user/artifact");
  // /tmp/...timestamped artifact paths -> generic artifact path
  out = out.replace(/\/tmp\/[^\s",]+/g, "/tmp/artifact");
  // home-dir dev-state paths
  out = out.replace(/~?\/\.[A-Za-z0-9_.-]*flowforge[^\s",]*/g, "~/.flowforge-dev");
  // any remaining flowforge-prefixed artifact filenames in prose
  out = out.replace(/flowforge-v?[0-9][^\s",]*/g, "flowforge-artifact");
  // :18789/:18790 dev ports -> generic dev port
  out = out.replace(/:1878\d|:1879\d/g, ":8080");
  return out;
}

// Collapse artifacts of token expansion. These are self-inflicted adjacent
// duplications introduced when a scrubbed token's replacement ends in a word
// that the surrounding source text repeats (e.g. "SQLite store" -> "embedded
// SQL store" + "store"). Listed explicitly so legitimate source phrasings like
// "multi-turn turn" or "handled-error ERROR" are left intact.
function tidy(s) {
  return s
    .replace(/\bthe the\b/g, "the")
    .replace(/\bThe the\b/g, "The")
    .replace(/\ba a\b/g, "a")
    .replace(/\bA a\b/g, "A")
    .replace(/\bruntime runtime\b/g, "runtime")
    .replace(/\bRuntime runtime\b/g, "Runtime")
    .replace(/\bstore store\b/g, "store");
}

function scrub(value) {
  if (typeof value !== "string") return value;
  let out = value;
  for (const [re, rep] of TOKEN_RULES) out = out.replace(re, rep);
  out = neutralizeDynamic(out);
  out = tidy(out);
  return out;
}

// scrub every string in an arbitrary JSON value (arrays / nested objects),
// used for prose-only structures. Keys are preserved.
function scrubDeep(value) {
  if (typeof value === "string") return scrub(value);
  if (Array.isArray(value)) return value.map(scrubDeep);
  if (value && typeof value === "object") {
    const out = {};
    for (const [k, v] of Object.entries(value)) out[k] = scrubDeep(v);
    return out;
  }
  return value;
}

const readJson = (rel) =>
  JSON.parse(fs.readFileSync(path.join(SRC, rel), "utf8"));
const writeJson = (rel, obj) => {
  const p = path.join(DEST, rel);
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, JSON.stringify(obj, null, 2) + "\n");
};

// ---------------------------------------------------------------------------
// Field-aware transformers. Enum/date/id fields are listed explicitly so the
// prose scrubber never touches them; ref fields get remapId; prose fields get
// scrub.
// ---------------------------------------------------------------------------

function transformNodes(data) {
  const nodes = data.nodes.map((n) => ({
    ...n,
    id: remapId(n.id),
    // type: enum, keep
    name: NODE_NAME_MAP[n.id] ?? scrub(n.name),
    summary: scrub(n.summary),
    responsibilities: (n.responsibilities ?? []).map(scrub),
    owner: scrub(n.owner),
    sourcePaths: (n.sourcePaths ?? []).map(scrub),
    runtime: scrub(n.runtime),
    interfaces: (n.interfaces ?? []).map(scrub),
    dependencies: remapIdList(n.dependencies ?? []),
    dataHandled: remapIdList(n.dataHandled ?? []), // data-class ids (none remapped)
    security: (n.security ?? []).map(scrub),
    observability: (n.observability ?? []).map(scrub),
    relatedFlows: remapIdList(n.relatedFlows ?? []),
    relatedDecisions: remapIdList(n.relatedDecisions ?? []),
    knownRisks: remapIdList(n.knownRisks ?? []),
    verification: (n.verification ?? []).map(scrub)
  }));
  return { nodes };
}

function transformFlows(data) {
  const flows = data.flows.map((f) => ({
    ...f,
    id: remapId(f.id),
    name: scrub(f.name),
    // status: enum keep
    summary: scrub(f.summary),
    trigger: scrub(f.trigger),
    actors: remapIdList(f.actors ?? []),
    steps: (f.steps ?? []).map((s) => ({
      ...s,
      // id: step id (local), keep verbatim
      from: remapId(s.from),
      to: remapId(s.to),
      // kind: enum keep; returnOf: step id local keep
      action: scrub(s.action),
      summary: scrub(s.summary),
      data: remapIdList(s.data ?? []),
      ...(s.outcome !== undefined ? { outcome: scrub(s.outcome) } : {})
    })),
    sequenceFrames: (f.sequenceFrames ?? []).map((fr) => ({
      ...fr,
      // id/type/stepIds: keep (type enum, stepIds local)
      label: scrub(fr.label)
    })),
    guarantees: (f.guarantees ?? []).map(scrub),
    failureBehavior: (f.failureBehavior ?? []).map(scrub),
    observability: (f.observability ?? []).map(scrub),
    verification: (f.verification ?? []).map(scrub),
    knownGaps: (f.knownGaps ?? []).map(scrub)
  }));
  return { flows };
}

function transformViews(data) {
  const views = data.views.map((v) => ({
    ...v,
    id: remapId(v.id),
    name: scrub(v.name),
    // type: enum keep
    summary: scrub(v.summary),
    ...(v.scopeNodeId ? { scopeNodeId: remapId(v.scopeNodeId) } : {}),
    lanes: (v.lanes ?? []).map((l) => ({
      ...l,
      // lane id local keep
      name: scrub(l.name),
      nodeIds: remapIdList(l.nodeIds ?? [])
    }))
  }));
  return { views };
}

function transformDecisions(data) {
  const decisions = data.decisions.map((d) => ({
    ...d,
    id: remapId(d.id),
    // status enum keep
    title: scrub(d.title),
    context: scrub(d.context),
    decision: scrub(d.decision),
    consequences: (d.consequences ?? []).map(scrub),
    relatedNodes: remapIdList(d.relatedNodes ?? []),
    relatedFlows: remapIdList(d.relatedFlows ?? [])
  }));
  return { decisions };
}

function transformRisks(data) {
  const risks = data.risks.map((r) => ({
    ...r,
    id: remapId(r.id),
    title: scrub(r.title),
    // category/severity/status enum keep
    summary: scrub(r.summary),
    mitigations: (r.mitigations ?? []).map(scrub),
    relatedNodes: remapIdList(r.relatedNodes ?? []),
    relatedFlows: remapIdList(r.relatedFlows ?? [])
  }));
  return { risks };
}

function transformRules(data) {
  const rules = data.rules.map((r) => ({
    ...r,
    // id keep (no rule ids carry identity); category free-text -> scrub safe
    title: scrub(r.title),
    summary: scrub(r.summary),
    category: scrub(r.category),
    // criticality/source enum keep; order/protection keep
    rationale: scrub(r.rationale),
    appliesTo: (r.appliesTo ?? []).map(scrub)
  }));
  return { rules };
}

function transformGlossary(data) {
  const terms = data.terms.map((t) => ({
    term: scrub(t.term),
    definition: scrub(t.definition)
  }));
  return { terms };
}

function transformDataClassification(data) {
  const classes = data.classes.map((c) => ({
    ...c,
    // id keep (none remapped); sensitivity enum keep
    name: scrub(c.name),
    handling: scrub(c.handling)
  }));
  return { classes };
}

function transformRoadmap(data) {
  const items = data.items.map((it) => ({
    ...it,
    id: remapId(it.id),
    // kind/status/priority enum keep; dateAdded date keep;
    // targetReleaseId is a release id (vX-Y-Z) -> no scrub token, kept
    targetReleaseId: it.targetReleaseId ? remapId(it.targetReleaseId) : it.targetReleaseId,
    title: scrub(it.title),
    summary: scrub(it.summary),
    section: scrub(it.section),
    evidence: (it.evidence ?? []).map(scrub)
  }));
  return { items };
}

// Release scope/workstream/blocker/milestone/dependency ids are release-LOCAL
// opaque slugs; preserving them keeps every internal ref valid. We scrub prose
// fields and leave ids/enums/dates/counts untouched. evidence may be strings
// or {id,label,kind,status,href} objects.
function scrubEvidenceEntry(e) {
  if (typeof e === "string") return scrub(e);
  return {
    ...e,
    id: remapId(e.id), // evidence ids are local but some embed scrub tokens
    // kind/status keep
    label: scrub(e.label),
    ...(e.href !== undefined ? { href: scrub(e.href) } : {})
  };
}

function transformReleaseDetail(d) {
  const mapScope = (arr) =>
    (arr ?? []).map((item) => ({
      ...item,
      id: remapId(item.id),
      // kind/status/priority/source/owner/dateAdded keep
      workstreamId: item.workstreamId ? remapId(item.workstreamId) : item.workstreamId,
      dependsOn: remapIdList(item.dependsOn ?? []),
      title: scrub(item.title),
      summary: scrub(item.summary),
      rationale: scrub(item.rationale),
      decisionSource: scrub(item.decisionSource),
      evidence: (item.evidence ?? []).map(scrubEvidenceEntry),
      ...(item.deferredToVersion !== undefined
        ? { deferredToVersion: scrub(item.deferredToVersion) }
        : {})
    }));
  const scope = {};
  for (const [bucket, arr] of Object.entries(d.scope ?? {})) {
    scope[bucket] = mapScope(arr);
  }
  return {
    ...d,
    // id/version/status/posture/dates keep; scrub all prose fields
    name: scrub(d.name),
    summary: scrub(d.summary),
    updateSource: d.updateSource !== undefined ? scrub(d.updateSource) : d.updateSource,
    ...(d.targetWindow !== undefined ? { targetWindow: scrub(d.targetWindow) } : {}),
    scope,
    workstreams: (d.workstreams ?? []).map((w) => ({
      ...w,
      id: remapId(w.id),
      // status/posture/progress keep
      itemIds: remapIdList(w.itemIds ?? []),
      name: scrub(w.name),
      summary: scrub(w.summary),
      owner: scrub(w.owner),
      evidence: (w.evidence ?? []).map(scrubEvidenceEntry)
    })),
    blockers: (d.blockers ?? []).map((b) => ({
      ...b,
      id: remapId(b.id),
      // severity/status/owner keep
      itemIds: remapIdList(b.itemIds ?? []),
      title: scrub(b.title),
      summary: scrub(b.summary),
      // dependency/nextAction/owner are scalar strings; evidenceNeeded is an
      // array — scrubDeep handles either shape safely.
      dependency: scrubDeep(b.dependency),
      evidenceNeeded: scrubDeep(b.evidenceNeeded),
      nextAction: scrubDeep(b.nextAction),
      owner: scrubDeep(b.owner)
    })),
    dependencies: (d.dependencies ?? []).map((dep) => ({
      ...dep,
      id: remapId(dep.id),
      from: remapId(dep.from),
      to: remapId(dep.to),
      summary: scrub(dep.summary)
    })),
    milestones: (d.milestones ?? []).map((m) => ({
      ...m,
      id: remapId(m.id),
      // date/order/status/targetWindow keep
      itemIds: remapIdList(m.itemIds ?? []),
      label: scrub(m.label)
    })),
    evidence: (d.evidence ?? []).map(scrubEvidenceEntry)
  };
}

function transformReleaseIndex(idx) {
  return {
    ...idx,
    // currentReleaseId is a release id (vX-Y-Z), not remapped, keep
    releases: idx.releases.map((r) => ({
      ...r,
      // id/version/status/posture/dates/counts/file keep; scrub name/summary/
      // targetWindow identically to detail so the generated-summary equality
      // check (references.mjs) still passes
      name: scrub(r.name),
      summary: scrub(r.summary),
      ...(r.targetWindow !== undefined ? { targetWindow: scrub(r.targetWindow) } : {})
    }))
  };
}

function transformManifest(m) {
  return {
    schemaVersion: m.schemaVersion,
    project: {
      id: "flowforge",
      name: "FlowForge",
      summary:
        "Workflow automation platform with a CLI, TUI, browser dashboard, daemon runtime, scheduler, integration connectors, context store, tool execution, automation-route selection, and a centralized workflow execution engine."
    },
    generatedAt: m.generatedAt,
    defaultViewId: remapId(m.defaultViewId),
    files: { ...m.files },
    notes: [
      "Sanitized, relabeled Architext review corpus derived from a real multi-surface automation runtime.",
      "Structure (graph topology, flows, C4 levels, release scope) is preserved 1:1; only identity-bearing labels and prose are generic.",
      "Built deterministically by test/fixtures/corpus-build.mjs for rendering review; not a live project's architecture."
    ]
  };
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------
function main() {
  fs.rmSync(DEST, { recursive: true, force: true });
  fs.mkdirSync(path.join(DEST, "releases"), { recursive: true });

  const manifest = readJson("manifest.json");

  writeJson("nodes.json", transformNodes(readJson("nodes.json")));
  writeJson("flows.json", transformFlows(readJson("flows.json")));
  writeJson("views.json", transformViews(readJson("views.json")));
  writeJson(
    "data-classification.json",
    transformDataClassification(readJson("data-classification.json"))
  );
  writeJson("decisions.json", transformDecisions(readJson("decisions.json")));
  writeJson("risks.json", transformRisks(readJson("risks.json")));
  writeJson("glossary.json", transformGlossary(readJson("glossary.json")));
  writeJson("rules.json", transformRules(readJson("rules.json")));
  writeJson("roadmap.json", transformRoadmap(readJson("roadmap.json")));

  // releases: index + every detail file
  const index = readJson("releases/index.json");
  writeJson("releases/index.json", transformReleaseIndex(index));
  for (const entry of index.releases) {
    const detail = readJson(path.join("releases", entry.file));
    writeJson(path.join("releases", entry.file), transformReleaseDetail(detail));
  }

  writeJson("manifest.json", transformManifest(manifest));

  console.log(
    `Built corpus at ${DEST}\n  releases: ${index.releases.length} detail files + index`
  );
}

main();
