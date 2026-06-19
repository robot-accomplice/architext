// Persists diagram config via POST /api/config. `fetcher` is mutationFetch
// (adds the server mutation token). Mirrors rulesClient's envelope handling.
/**
 * @param {(input: any, init?: any) => Promise<Response>} fetcher
 * @param {{ scope: "project" | "user", diagram: any }} payload
 * @returns {Promise<{ ok?: boolean, scope?: string, file?: string, written?: any, diagram?: any, warnings?: string[], error?: string }>}
 */
export async function postDiagramConfig(fetcher, payload) {
  let response;
  try {
    response = await fetcher("/api/config", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(payload)
    });
  } catch {
    throw new Error("Diagram config save failed. Confirm architext serve is running for this repository, then retry.");
  }

  const responseText = await response.text();
  let result = {};
  if (responseText) {
    try {
      result = JSON.parse(responseText);
    } catch {
      throw new Error("Diagram config save failed: invalid server response.");
    }
  }

  if (!response.ok || result.ok === false) {
    throw new Error(result.error ?? `Diagram config save failed: ${response.status}`);
  }
  return result;
}
