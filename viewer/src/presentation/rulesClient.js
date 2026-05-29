export async function postRulesAction(fetcher, payload) {
  let response;
  try {
    response = await fetcher("/api/rules", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(payload)
    });
  } catch {
    throw new Error("Rules update request failed. Confirm architext serve is running for this repository, then retry.");
  }

  const responseText = await response.text();
  let result = {};
  if (responseText) {
    try {
      result = JSON.parse(responseText);
    } catch {
      throw new Error("Rules update failed: invalid server response.");
    }
  }

  if (!response.ok || result.ok === false) {
    throw new Error(result.error ?? `Rules update failed: ${response.status}`);
  }
  return result;
}
