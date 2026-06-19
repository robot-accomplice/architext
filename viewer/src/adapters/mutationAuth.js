let sessionPromise = null;

async function mutationToken() {
  sessionPromise ??= fetch("/api/session")
    .then(async (response) => {
      if (!response.ok) throw new Error(`Architext session request failed: ${response.status}`);
      const session = await response.json();
      if (!session.mutationToken) throw new Error("Architext session did not include a mutation token.");
      return session.mutationToken;
    });
  return sessionPromise;
}

export async function mutationFetch(input, init = {}) {
  const headers = new Headers(init.headers ?? {});
  headers.set("x-architext-mutation-token", await mutationToken());
  return fetch(input, {
    ...init,
    headers
  });
}
