# Data Watch SSE Resource Limits

The data watch hub streams validation events to the local viewer through
server-sent events. SSE connections are long-lived, so the hub must treat each
client as a held server resource.

## Contract

- The hub caps concurrent SSE clients.
- Clients over the cap receive a controlled failure instead of being retained.
- Broadcasts and heartbeats check the return value of `write`.
- A client that applies back-pressure is closed instead of accumulating
  unbounded buffered data.
- Heartbeats keep healthy clients active and give the hub a periodic chance to
  evict dead peers.

This is intentionally conservative. The local viewer only needs a small number
of live browser tabs, not an unlimited pub/sub fanout.

## Verification

- Normal clients still receive validation refresh events.
- Non-JSON writes still do not schedule validation.
- The hub rejects clients over the configured cap.
- A client whose `write` reports back-pressure is removed from the active set.
