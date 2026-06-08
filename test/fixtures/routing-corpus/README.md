# Routing corpus (sanitized)

A repo-local, **sanitized** copy of a real, complex architecture (the `roboticus`
project) used as the high-complexity test bed for the routing engine. Toy fixtures
(3–10 nodes) don't reproduce the routing challenges that real diagrams hit; this corpus
does, while leaking none of the source project's actual architecture.

## What was done

- **Only routing-relevant data is kept**: `nodes.json` (id + type), `views.json`
  (lanes), `flows.json` (steps: `from`/`to`/`kind`/`returnOf`/`outcome`). All domain
  prose (summaries, responsibilities, security, data handled, …) was dropped — it is
  irrelevant to routing and is the only thing that would leak the source design.
- **Identifiers are neutralized but ORDER-PRESERVING**: nodes become `n##`, steps become
  `s###`, lanes/views become `view-*`/`lane-*`. The rank in each id is the position of the
  original identifier in sorted order, so the neutral ids sort into the **exact same order**
  as the originals. Structure — lane membership and order, step order, edge direction,
  `kind`, `returnOf`, `outcome` — is preserved exactly.
- **Flows are relabeled by routing objective** (see below), so a failing test names the
  routing case that broke.

## Fidelity

Routing geometry comes only from view lanes + flow steps (`planDiagram` never reads
`nodes.json`). The router also uses **identifier order as a deterministic tiebreak**, and the
current router is sensitive enough to mount position that a tiebreak flip can change bends and
crossings — not just sub-lane coordinates. So the ids here are **order-preserving** (see above):
neutral ids sort identically to the originals, which makes every flow reproduce the source's
routing signature **exactly** — identical route count, bend count, AND crossing count, verified
15/15 against the source on the current router.

> An earlier revision neutralized ids to `<type>-NN`, which reordered the tiebreak and silently
> diverged from the source (5/15 flows, e.g. `bundle-return-gutter` rendered 5 crossings where
> the source has 0). Preserving sort order is what keeps the bed honest. If you re-sanitize,
> keep the order-preserving mapping or re-verify 15/15 parity.

The roboticus-derived flows are a frozen fixture. Do not hand-edit them; if the source corpus
changes, regenerate with an order-preserving id map and re-verify parity.

## Feature coverage (synthetic showcase)

This corpus is also the **live-UI review fixture** (review diagrams here, never a real
project). The faithful roboticus flows don't exercise every renderable diagram feature, so a
small amount of clearly-marked **synthetic** content is added purely for feature coverage. It
does not claim source fidelity and is gated by the same fitness baseline as everything else.

- **`feature-showcase` flow** + **`view-30` (flow-explorer)** + nodes **`n90`–`n96`**: exercises a
  **decision diamond** (`kind:"decision"` with two `outcome` branches), the `start` / `async` /
  `persistence` / `artifact` / `stop` step kinds, and the **`queue`** node type (`n95`) — the
  features absent from the roboticus flows. Self-contained: these nodes/view are referenced by
  nothing else, so existing flows' geometry is unchanged (baseline diff = this one flow added).
- **`sequenceFrames` on `hub-fan-dense`**: `loop` / `optional` / `retry` / `transaction` frames
  over its existing request/return pairs, to cover sequence-frame rendering. Frames are
  sequence-diagram-only metadata; `planDiagram`/metrics never read them, so `hub-fan-dense`'s
  routing signature is unchanged.

When adding feature coverage, prefer new self-contained flows/views (then re-run
`regen-baseline.mjs`) over editing the faithful roboticus flows.

## Flow taxonomy (objective → what it guards)

| flow id | routing objective it guards |
|---|---|
| `pair-minimal` | one reciprocal pair, 3 nodes |
| `pair-chain-linear` | reciprocal chain, no axis alignment |
| `pair-chain-short` | short reciprocal chain |
| `pair-fan-compact` | 4-way reciprocal fan |
| `pair-fan-basic` | reciprocal fan |
| `pair-single-multiedge` | one pair + extra edges on a node pair |
| `fan-coaligned-selfcross` | provider fan + co-aligned self-crossing pair |
| `bundle-coaligned` | 3-edge bundle on a single node pair |
| `bundle-return-gutter` | reciprocal bundle whose return takes a lower gutter |
| `fan-quad-reciprocal` | four reciprocal pairs |
| `fan-parallel-bundle` | reciprocal parallel bundles, fan-in 5 |
| `bracket-intermediates` | reciprocal bracket around stacked intermediate nodes |
| `fan-max-systemmap` | five reciprocal pairs on a system-map |
| `coaligned-multibundle-surfaceselect` | **two reciprocal pairs on one co-columnar node pair — surface selection + ordering (the curate-pair case)** |
| `hub-fan-dense` | 9-node hub, 7-way fan, 8 reciprocal pairs (+ synthetic `sequenceFrames`) |
| `feature-showcase` | **synthetic** — decision diamond, start/async/persistence/artifact/stop kinds, queue node |
