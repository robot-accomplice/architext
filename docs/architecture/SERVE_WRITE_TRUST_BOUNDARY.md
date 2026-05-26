# Serve Write Trust Boundary

Architext serve is a local viewer for repository-owned files. The server may
read package-owned assets and project-owned JSON for the browser UI, but write
operations have a stricter trust boundary.

## Architecture

The local HTTP server must treat every disk-writing request as untrusted until
it proves all of these facts:

- The request Host is loopback: `localhost`, `127.0.0.0/8`, or `::1`.
- A browser Origin, when present, is same-origin with that loopback Host.
- The request carries the per-server mutation token in
  `x-architext-mutation-token`.

The token is generated when the viewer server starts and is exposed only to the
same-origin Architext UI through a read-only session endpoint. Mutating requests
use a custom header so cross-origin browser submissions require a CORS preflight
that Architext does not authorize.

Remote serving is out of scope for the unauthenticated local viewer. `--host`
must stay loopback-only until a deliberately designed remote mode exists with an
explicit authentication contract.

## Protected Endpoints

These endpoints write project-owned files and require the mutation guard:

- `POST /api/doctor`
- `POST /api/sync-repair`
- `POST /api/release-plans`
- `POST /api/rules`

Read endpoints such as `/api/status`, `/api/data-events`, package assets, and
`/data/**` still require a loopback Host but do not require the mutation token.

## Verification

- Cross-origin or non-loopback Host requests to mutating endpoints are rejected.
- Mutating requests without the token are rejected.
- Same-origin mutating requests with the token still work.
- `--host 0.0.0.0` and LAN addresses fail before the server starts.
