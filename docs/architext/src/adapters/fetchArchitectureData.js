import { validateArchitectureReferences } from "../../../../src/domain/architecture-model/references.mjs";

async function fetchJson(fetcher, path) {
  const response = await fetcher(path);
  if (!response.ok) {
    throw new Error(`Failed to load ${path}: ${response.status} ${response.statusText}`);
  }
  const text = await response.text();
  try {
    return JSON.parse(text);
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    const snippet = text.length > 1200 ? `${text.slice(0, 1200)}\n...` : text;
    throw new Error(`Failed to parse ${path}: ${reason}\n\nOffending JSON:\n${snippet}`);
  }
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

  const releases = manifest.files.releases
    ? await loadReleaseModel(fetcher, base, manifest.files.releases)
    : undefined;
  const roadmap = manifest.files.roadmap
    ? await fetchJson(fetcher, base + manifest.files.roadmap)
    : undefined;
  const rules = manifest.files.rules
    ? await fetchJson(fetcher, base + manifest.files.rules)
    : undefined;

  const model = {
    manifest,
    nodes: nodes.nodes,
    flows: flows.flows,
    views: views.views,
    dataClasses: dataClassification.classes,
    decisions: decisions.decisions,
    risks: risks.risks,
    ...(rules ? { rules: rules.rules } : {}),
    ...(roadmap ? { roadmap: roadmap.items } : {}),
    ...(releases ? { releases } : {})
  };
  const errors = validateArchitectureReferences(model);
  if (errors.length > 0) {
    throw new Error(`Architext data failed viewer validation:\n${errors.join("\n")}`);
  }
  return model;
}

export async function loadReleaseDetail(fetcher = fetch, releaseModel, releaseId) {
  const summary = releaseModel.index.releases.find((release) => release.id === releaseId);
  if (!summary) {
    throw new Error(`Release "${releaseId}" is not listed in the release index`);
  }
  return fetchJson(fetcher, `/data/${releaseModel.detailBasePath}${summary.file}`);
}

async function loadReleaseModel(fetcher, base, indexPath) {
  const index = await fetchJson(fetcher, base + indexPath);
  const current = index.releases.find((release) => release.id === index.currentReleaseId);
  const detailBasePath = indexPath.includes("/") ? indexPath.slice(0, indexPath.lastIndexOf("/") + 1) : "";
  const details = current ? [await fetchJson(fetcher, `${base}${detailBasePath}${current.file}`)] : [];
  return { index, details, detailBasePath };
}
