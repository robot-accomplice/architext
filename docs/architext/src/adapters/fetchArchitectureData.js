import { validateArchitectureReferences } from "../../../../src/domain/architecture-model/references.mjs";

async function fetchJson(fetcher, path) {
  const response = await fetcher(path);
  if (!response.ok) {
    throw new Error(`Failed to load ${path}: ${response.status} ${response.statusText}`);
  }
  return response.json();
}

export async function loadArchitectureModel(fetcher = fetch) {
  const manifest = await fetchJson(fetcher, "/data/manifest.json");
  const base = "/data/";
  const [nodes, flows, views, dataClassification, decisions, risks] = await Promise.all([
    fetchJson(fetcher, base + manifest.files.nodes),
    fetchJson(fetcher, base + manifest.files.flows),
    fetchJson(fetcher, base + manifest.files.views),
    fetchJson(fetcher, base + manifest.files.dataClassification),
    fetchJson(fetcher, base + manifest.files.decisions),
    fetchJson(fetcher, base + manifest.files.risks)
  ]);

  const model = {
    manifest,
    nodes: nodes.nodes,
    flows: flows.flows,
    views: views.views,
    dataClasses: dataClassification.classes,
    decisions: decisions.decisions,
    risks: risks.risks
  };
  const errors = validateArchitectureReferences(model);
  if (errors.length > 0) {
    throw new Error(`Architext data failed viewer validation:\n${errors.join("\n")}`);
  }
  return model;
}
