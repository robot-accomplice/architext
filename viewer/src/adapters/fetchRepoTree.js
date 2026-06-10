// Fetches the target repo's file list from `architext serve` (GET /api/repo-tree):
// { files: string[], source: "git" | "filesystem" }. Returns null when the
// endpoint is absent (e.g. a static build) or the request fails.
export async function fetchRepoTree(fetcher = fetch) {
  try {
    const response = await fetcher("/api/repo-tree");
    if (!response || !response.ok) return null;
    const body = await response.json();
    return body && Array.isArray(body.files) ? body : null;
  } catch {
    return null;
  }
}
