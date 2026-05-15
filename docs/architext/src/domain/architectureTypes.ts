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

export type Model = {
  manifest: Manifest;
  nodes: ArchNode[];
  flows: Flow[];
  views: View[];
  dataClasses: DataClass[];
  decisions: Decision[];
  risks: Risk[];
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
  | { kind: "relationship"; from: Id; to: Id; label: string; relationshipType: Relationship["relationshipType"]; stepId?: Id; flowId?: Id };

export type Mode = "flows" | "sequence" | "c4" | "deployment" | "data-risks";
export type RoutingStyle = "orthogonal" | "spline" | "straight";

export type DiagramTransform = {
  zoom: number;
  focused: boolean;
};

export type ViewportSize = {
  width: number;
  height: number;
};
