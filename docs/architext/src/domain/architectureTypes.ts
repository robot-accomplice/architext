export type Id = string;

export type Manifest = {
  schemaVersion: string;
  project: {
    id: Id;
    name: string;
    summary: string;
  };
  generatedAt: string;
  defaultViewId: Id;
  files: {
    nodes: string;
    flows: string;
    views: string;
    dataClassification: string;
    decisions: string;
    risks: string;
    glossary: string;
    releases?: string;
  };
  notes: string[];
};

export type NodeType =
  | "actor"
  | "software-system"
  | "client"
  | "service"
  | "module"
  | "worker"
  | "queue"
  | "data-store"
  | "external-service"
  | "deployment-unit"
  | "trust-boundary";

export type ArchNode = {
  id: Id;
  type: NodeType;
  name: string;
  summary: string;
  responsibilities: string[];
  owner: string;
  sourcePaths: string[];
  runtime: string;
  interfaces: string[];
  dependencies: Id[];
  dataHandled: Id[];
  security: string[];
  observability: string[];
  relatedFlows: Id[];
  relatedDecisions: Id[];
  knownRisks: Id[];
  verification: string[];
};

export type FlowStep = {
  id: Id;
  from: Id;
  to: Id;
  action: Id;
  summary: string;
  data: Id[];
};

export type Flow = {
  id: Id;
  name: string;
  status: "planned" | "partial" | "implemented";
  summary: string;
  trigger: string;
  actors: Id[];
  steps: FlowStep[];
  guarantees: string[];
  failureBehavior: string[];
  observability: string[];
  verification: string[];
  knownGaps: string[];
};

export type View = {
  id: Id;
  name: string;
  type: string;
  summary: string;
  lanes: Array<{
    id: Id;
    name: string;
    nodeIds: Id[];
  }>;
};

export type DataClass = {
  id: Id;
  name: string;
  sensitivity: "low" | "medium" | "high" | "critical";
  handling: string;
};

export type Decision = {
  id: Id;
  status: string;
  title: string;
  context: string;
  decision: string;
  consequences: string[];
  relatedNodes: Id[];
  relatedFlows: Id[];
};

export type Risk = {
  id: Id;
  title: string;
  category: string;
  severity: "low" | "medium" | "high" | "critical";
  status: string;
  summary: string;
  mitigations: string[];
  relatedNodes: Id[];
  relatedFlows: Id[];
};

export type ReleaseStatus = "planning" | "active" | "blocked" | "candidate" | "released" | "deferred";
export type ReleaseItemStatus = "planned" | "in-progress" | "blocked" | "complete" | "deferred" | "stretch" | "cut";
export type ReleasePosture = "on-track" | "at-risk" | "blocked" | "release-candidate" | "shipped";
export type ReleaseItemKind = "feature" | "bug-fix" | "documentation" | "architecture" | "test" | "chore";

export type ReleaseCounts = {
  features: number;
  bugFixes: number;
  workstreams: number;
  blockers: number;
  complete: number;
  inProgress: number;
  planned: number;
  stretch: number;
};

export type ReleaseSummary = {
  id: Id;
  version: string;
  name: string;
  status: ReleaseStatus;
  posture: ReleasePosture;
  targetDate?: string;
  targetWindow?: string;
  releasedAt?: string;
  lastUpdated: string;
  summary: string;
  counts: ReleaseCounts;
  file: string;
};

export type ReleaseIndex = {
  currentReleaseId: Id;
  releases: ReleaseSummary[];
};

export type ReleaseItem = {
  id: Id;
  title: string;
  kind: ReleaseItemKind;
  status: ReleaseItemStatus;
  summary: string;
  owner?: string;
  priority?: "critical" | "high" | "medium" | "low";
  rationale?: string;
  decisionSource?: string;
  workstreamId?: Id;
  dependsOn?: Id[];
  evidence?: string[];
};

export type ReleaseWorkstream = {
  id: Id;
  name: string;
  owner: string;
  status: ReleaseItemStatus;
  posture: ReleasePosture;
  summary: string;
  progress?: number;
  itemIds: Id[];
  evidence: string[];
};

export type ReleaseBlocker = {
  id: Id;
  title: string;
  severity: "low" | "medium" | "high" | "critical";
  status: ReleaseItemStatus;
  owner: string;
  summary: string;
  dependency?: string;
  nextAction: string;
  itemIds: Id[];
  evidenceNeeded: string[];
};

export type ReleaseMilestone = {
  id: Id;
  label: string;
  status: ReleaseItemStatus;
  date?: string;
  targetWindow?: string;
  order: number;
  itemIds: Id[];
};

export type ReleaseDependency = {
  id: Id;
  from: Id;
  to: Id;
  summary: string;
};

export type ReleaseEvidence = {
  id: Id;
  label: string;
  kind: "test" | "build" | "manual-check" | "release-note" | "screenshot" | "document";
  status: ReleaseItemStatus;
  href?: string;
};

export type ReleaseDetail = {
  id: Id;
  version: string;
  name: string;
  status: ReleaseStatus;
  posture: ReleasePosture;
  summary: string;
  targetDate?: string;
  targetWindow?: string;
  releasedAt?: string;
  lastUpdated: string;
  updateSource?: string;
  scope: {
    required: ReleaseItem[];
    planned: ReleaseItem[];
    stretch: ReleaseItem[];
    deferred: ReleaseItem[];
    outOfScope: ReleaseItem[];
  };
  workstreams: ReleaseWorkstream[];
  blockers: ReleaseBlocker[];
  milestones: ReleaseMilestone[];
  dependencies: ReleaseDependency[];
  evidence: ReleaseEvidence[];
};

export type ReleaseModel = {
  index: ReleaseIndex;
  details: ReleaseDetail[];
  detailBasePath: string;
};

export type Model = {
  manifest: Manifest;
  nodes: ArchNode[];
  flows: Flow[];
  views: View[];
  dataClasses: DataClass[];
  decisions: Decision[];
  risks: Risk[];
  releases?: ReleaseModel;
};

export type Relationship = {
  id: Id;
  from: Id;
  to: Id;
  label: string;
  summary: string;
  relationshipType: "flow" | "structural";
  toType?: NodeType;
  stepId?: Id;
  flowId?: Id;
};

export type Selection =
  | { kind: "node"; id: Id }
  | { kind: "flow"; id: Id }
  | { kind: "step"; flowId: Id; stepId: Id }
  | { kind: "relationship"; from: Id; to: Id; label: string; relationshipType: Relationship["relationshipType"]; stepId?: Id; flowId?: Id }
  | { kind: "release-milestone"; milestoneId: Id }
  | { kind: "release-item"; itemId: Id };

export type Mode = "flows" | "sequence" | "c4" | "deployment" | "data-risks" | "release-truth";
export type RoutingStyle = "orthogonal" | "spline" | "straight";

export type DiagramTransform = {
  zoom: number;
  focused: boolean;
};

export type ViewportSize = {
  width: number;
  height: number;
};
