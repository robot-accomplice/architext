// Fetches the diagram config payload from `architext serve` (GET /api/config):
// { diagram, warnings, fields, sections }. Returns null when the endpoint is
// absent (e.g. a static build) or the request fails — callers then use
// hardcoded defaults, so a missing config never blocks rendering.
export async function fetchDiagramConfig(fetcher = fetch) {
  try {
    const response = await fetcher("/api/config");
    if (!response || !response.ok) return null;
    const body = await response.json();
    return body && typeof body === "object" && body.diagram ? body : null;
  } catch {
    return null;
  }
}
