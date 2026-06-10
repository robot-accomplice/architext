// Persists element notes via POST /api/notes. `fetcher` is mutationFetch so the
// request carries the serve session's mutation token. Mirrors rulesClient.
export async function postNotesAction(fetcher, payload) {
  let response;
  try {
    response = await fetcher("/api/notes", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(payload)
    });
  } catch {
    throw new Error("Notes update request failed. Confirm architext serve is running for this repository, then retry.");
  }

  const responseText = await response.text();
  let result = {};
  if (responseText) {
    try {
      result = JSON.parse(responseText);
    } catch {
      throw new Error("Notes update failed: invalid server response.");
    }
  }

  if (!response.ok || result.ok === false) {
    throw new Error(result.error ?? `Notes update failed: ${response.status}`);
  }
  return result;
}
