// Fetches the resolved diagram config from `architext serve` (GET /api/config).
// Returns the `diagram` config object, or null when the endpoint is absent
// (e.g. a static build) or the request fails — callers then use hardcoded
// defaults, so a missing config never blocks rendering.
export async function fetchDiagramConfig(fetcher = fetch) {
  try {
    const response = await fetcher("/api/config");
    if (!response || !response.ok) return null;
    const body = await response.json();
    return body && typeof body === "object" ? body.diagram ?? null : null;
  } catch {
    return null;
  }
}
