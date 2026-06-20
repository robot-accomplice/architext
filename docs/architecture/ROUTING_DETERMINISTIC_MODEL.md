# Orthogonal Routing — Problem Statement & Deterministic Model

- **Date:** 2026-06-20
- **Status:** Foundation — **locked 2026-06-20** (problem, math, and cost order
  confirmed by the maintainer). Fixes *what* must be solved and *how* it is solved
  deterministically. Implementation spec / code-mapping come next, downstream of
  this.
- **Supersedes (as starting point):** the earlier `ROUTING_ORTHOGONAL_REDESIGN.md`
  draft, which jumped to implementation. Its vetting/mapping fold back in only
  after this foundation is locked.

This document is deliberately two layers and nothing else:
**§1 the problem** (requirements) and **§2 the mathematics**, then **§3 the
deterministic procedure** (pseudocode) and **§4 why it is deterministic.**

---

## 0. Ubiquitous language — the shape taxonomy (READ FIRST, NON-NEGOTIABLE)

Confirmed by the maintainer 2026-06-20 ("exactly right"). **Line shape — not bend
count — classifies a route.** Get this wrong and every downstream cost is wrong.

**Anatomy of a two-bend route.** Three segments: two **end segments**, each hung
off one end of a **middle (conjoining) segment**. The middle segment is the
shape's **axis** — the single straight reference line that orients both end
segments. An *axis* requires a shared reference point/line; the two end segments
**share no point with each other**, so there is no axis *between* them. Only the
middle segment is an axis. (Therefore the two end segments pointing opposite ways
is **not** a "reversal on an axis" — there is no shared axis to reverse on.)

**The ladder.** Cost is a property of shape:

| shape | segments | structure | `β` |
|---|---|---|---|
| **straight** | 1 | — | **0** |
| **L** | 2 | two perpendicular segments, one bend | **1** |
| **C** | 3 | two end segments on the **SAME side** of the middle segment: `[` `]` `∩` `∪` | **2** |
| **Z / staircase** | 3+ | end segments on **OPPOSITE sides** of the middle segment: `_|ˉ` | **99** = `Z_PENALTY` |
| **dogleg** | — | the line **doubles back over itself** (retraces / folds onto its own path) | **1e9** = `DOGLEG_PENALTY` |

**The rules that keep getting violated:**

1. **A C connects two *like-facing* surfaces** — two easterly, two westerly, two
   northerly (`∩`), or two southerly (`∪`). Its two end segments are parallel and
   non-contiguous, both on the same side of the middle segment. A C may span the
   same plane or different planes.
2. **A C between two *facing* surfaces is impossible.** Two facing surfaces,
   offset, produce a **Z** (`_|ˉ`) — the two end segments land on opposite sides of
   the conjoining segment. Aligned facing surfaces produce a straight or an L.
   A "jog" between facing surfaces is therefore an **unjustifiable Z (99)**, never
   a C.
3. **Bend count does not classify a shape.** A C and a Z both have two bends. The
   discriminator is *which side of the middle segment the end segments sit on*
   (same → C, opposite → Z).
4. **A clean C is never a dogleg**, in any of its four orientations, even though
   its two end segments point opposite — they share no axis, and the line never
   folds over itself. A **dogleg** is the line physically doubling back over
   itself; it is **not** defined by coordinate-axis sign reversal.
5. **Minimum stem.** Every route travels straight off a surface for at least
   `MIN_SURFACE_STEM` before its first bend — no bending right at the wall.
6. **Straighten beats even distribution.** Mount slots are spread across a surface
   for legibility, but spreading two *facing* mounts to different offsets turns a
   route that *could* be a straight into an avoidable jog (a Z). A **straightening
   pass runs after distribution**: if pulling a route's two mounts to a common
   coordinate makes it straight (or removes a bend), do it — **even if that
   violates even distribution.** An avoidable jog is never an acceptable price for
   tidy slot spacing.

---

## 1. Problem statement

### Given
- A finite set of nodes `V`. Each `v ∈ V` is an axis-aligned rectangle
  `R_v ⊂ ℝ²` (screen coordinates, `y` increasing downward).
  `O = { R_v : v ∈ V }` is the **obstacle field**.
- Each node exposes four **surfaces** `s ∈ {L, R, T, B}` with outward unit
  normals `n_L=(−1,0)`, `n_R=(+1,0)`, `n_T=(0,−1)`, `n_B=(0,+1)`. Each surface
  carries an ordered, finite set of **mount slots** on its face.
- A finite set of **connections** `E`. Each `e = (A, B)` is directed,
  `A, B ∈ V`, `A ≠ B`.

### Produce
- For each `e`, a route `r(e)`: a rectilinear polyline from a mount on a surface
  of `A` to a mount on a surface of `B`.

### Hard constraints (H) — hold for every route, always, no exception
- **H1 — Orthogonality.** Every segment is axis-aligned; consecutive segments
  alternate axis.
- **H2 — No doubling back (no dogleg).** The route never doubles back **over
  itself**: no segment retraces or overlaps a part of the route already drawn. This
  is absolute. *(Supersedes the earlier "monotonicity" formulation — see §0.
  Monotonicity is too strict: a clean **C**, e.g. the `∩` arch, reverses heading on
  the axis where its endpoints don't move. That is clearance, not doubling back,
  and is allowed. The constraint is self-overlap, not axis-sign monotonicity.)*
- **H3 — Clearance.** No segment crosses the interior of any node in
  `O \ {A, B}`; the first/last segment does not re-enter `A`/`B`.
- **H4 — Face correctness.** The route leaves `A` outward through its chosen
  surface and enters `B` inward through its chosen surface
  (`first_dir · n_{sA} > 0`, `last_dir · n_{sB} < 0`).

### Objectives (Q) — minimize, strict lexicographic priority
1. **Q1 — bend score** `β(r)`: a **shape-weighted** score, *not* a raw turn count
   (maintainer, 2026-06-20). Two routes with the same number of turns are not
   equal — a clean turn and a doubling-back turn score very differently:

   The discriminator is the §0 shape, **not** the turn count (a C and a Z both have
   two turns):

   | shape | turns | end segments vs middle | `β` |
   |---|---|---|---|
   | straight | 0 | — | **0** |
   | L | 1 | — | **1** |
   | C | 2 | **same side** of the middle segment (`[ ] ∩ ∪`) | **2** |
   | Z / staircase | 2+ | **opposite sides** of the middle segment (`_|ˉ`) | **99** = `Z_PENALTY` |
   | dogleg | — | the line overlaps / folds over **itself** | **1e9** = `DOGLEG_PENALTY` |

   General rule (the §0 ladder — classify by shape):
   ```
   β(r) = DOGLEG_PENALTY (1e9)  if doubles_back(r)        # folds over itself
        = 0 | 1                  if straight | L
        = 2                      if two-bend ∧ end segments SAME side of middle  # C
        = Z_PENALTY (99)         otherwise (opposite-side two-bend, or ≥3 bends) # Z
   ```
   A **C** is the most a route may bend cleanly. A **Z / staircase** — a two-bend
   jog whose ends fall on opposite sides of the middle segment (the jog between two
   *facing* surfaces), or any ≥3-bend route — is never acceptable and has *always*
   cost 99. A **dogleg** (the line doubles back over itself, [`doubles_back`]) is
   catastrophic at 1e9, orders of magnitude past a Z, so no trade of crossings or
   length can ever buy one. Both are named constants, never literals (rule 3).
   See §0 for the full vocabulary.
2. **Q2 — crossings** `crossings(r)`: intersections with other routes.
3. **Q3 — length** `length(r)`: Manhattan length.

**LAW REVISION (maintainer, 2026-06-20): crossings can outweigh a bend.** The
strict bends-first order below held until the fan-out evidence: under it, fan
crossings are *irreducible* — removing them needs a C's extra bend, which the
order forbids, so a crossed 1-bend L always beat a clean 2-bend C (measured:
forcing Cs scored β 138→243, crossings 18→98, worse on both — strict bends-first
made the L-router law-optimal at 138/18). The maintainer ruled that in dense fans
a clean C reads better than a crossed L. The clean-shape cost is therefore now
**weighted, not lexicographic**: `cost = W_b·bends + W_x·crossings + W_len·length`
(a Z/staircase still scores 99; a dogleg/reversal is a hard exclusion at
`DOGLEG_PENALTY`=1e9). A C beats an L when `W_b < Δcrossings · W_x`. The text below
is the **superseded** strict order, kept for provenance.

**Superseded strict priority law:** `Q1 ≻ Q2 ≻ Q3`, lexicographic, no slack. **The
bend score was the highest-cost element — an extra clean bend always loses**,
even to crossings and always to length; a *reversal* bend loses
catastrophically (β jumps to 99). Consequences (of the superseded order):
- a straight/L is taken **even when it crosses**, over any extra-bend route with
  fewer or zero crossings;
- more distance is *always* accepted to remove a bend;
- staircasing merely to shorten, when a cleaner (fewer-bend) circumnavigation
  exists, is a failure — including *inside* the forced regime, where a low-bend
  circumnavigation beats a shorter high-bend wiggle.

This folds the old separate `doglegs` term into `β`: a route that doubles back over
itself is simply one whose `β = 1e9` (`DOGLEG_PENALTY`). The model never *emits* one
(H2); the score still *represents* one, so a baselined dogleg loses by a billion.

The full order is therefore fixed: `β`, then `crossings`, then `length`,
then `(displayIndex, relationshipId)` for determinism.

### Determinism (D)
`r : (V, O, E, mounts) ↦ routes` is a **pure function.** Identical input ⇒
identical output. No randomness; every ordering used is a fixed total order.
(Required by the plan-farm / parity contract.) Determinism is required;
*global optimality is not* — the procedure is greedy under a fixed order
(see §4).

### Shape taxonomy (a consequence of §0 + Q1, not a separate rule)
| Shape | bends | end segments vs middle | status |
|---|---|---|---|
| straight | 0 | — | preferred |
| L | 1 | — | preferred |
| C | 2 | **same side** of the middle segment | preferred (the most a clean route may bend) |
| Z / staircase | 2+ | **opposite sides**, or ≥3 bends | β = 99; never acceptable, avoided |
| **dogleg** | — | line folds over **itself** | **β = 1e9; excluded by H2 — never produced** |

A dogleg is not a high-cost option; it fails H2 (it doubles back over itself), so it
is never a candidate. A Z/staircase satisfies H2 (it never overlaps itself) but is
priced at 99, so the search avoids it wherever a straight / L / **C arch** exists.

---

## 2. Mathematical model

### 2.1 Dogleg ≡ the line folds over itself (the formal core)
Let `P = (q₀, q₁, …, q_k)` be the reduced (corner) form; its segments are
`e_i = (q_i, q_{i+1})`.

```
overlap(e_i, e_j)  ⟺  e_i ∥ e_j  ∧  collinear(e_i, e_j)  ∧  their ranges share > 1 point
dogleg(P)          ⟺  ∃ i ≠ j : overlap(e_i, e_j)        # the line is drawn over itself
H2                 ≡  ¬dogleg(P)
```
This is the §0 definition: a dogleg is the route physically **doubling back over
itself**, not an axis-sign reversal. Note the consequence that overturned the old
formulation: a **C arch** (e.g. `∩`) is *not* monotone — it reverses heading on the
axis where its endpoints coincide and so **leaves the endpoints' bounding box** —
yet it never overlaps itself, so it satisfies H2 and is a clean shape. Bounding-box
containment is therefore **not** a property of dogleg-free routes; only
self-non-overlap is.

### 2.2 Bends and shape
For a reduced orthogonal path, `bends(P) = k − 1`. `straight ⟺ k=1`; `L ⟺ k=2`. A
two-bend path (`k=3`, segments `e₀, e₁ (middle), e₂`) is a **C** iff `q₀` and `q₃`
lie on the **same side** of the line through the middle segment `e₁`, else a **Z**.
`staircase ⟺ k ≥ 4` (≥3 bends) — always the Z tier.

### 2.3 Feasibility of a 0/1-bend route (free space)
For surfaces `sA, sB`, mounts `p_a, p_b`, with `d = p_b − p_a`, `α = d · n_{sA}`:

```
free_space(sA, p_a, sB, p_b):
  if α ≤ 0:                 false      # leaving A's face away from B (breaks H4/H2)
  if n_{sB} = −n_{sA}:      d × n_{sA} = 0     # facing: straight iff collinear (0 bends)
  if n_{sB} ⟂ n_{sA}:       d · n_{sB} < 0     # clean L into B's face (1 bend)
  else:                     false
```
True feasibility lifts this with clearance:
```
feasible(sA,p_a,sB,p_b)  ⟺  free_space(…) ∧ clears(O, path₀₁(p_a,p_b))
```
where `path₀₁` is the concrete straight or single-L polyline and `clears(O, ·)`
tests H3 (including A/B's own bodies). Offset-facing, back-to-back, same-side,
and blocked-L all make `feasible = false` for that surface pair.

### 2.4 Crossings
- **Intra-bundle** (edges sharing one `A→B` surface pair): order sources by mount
  as `σ`, their targets by mount as `τ`; `crossings = inversions(τ ∘ σ⁻¹)`.
  Zero ⟺ the two mount orderings are co-monotone.
- **Inter-route** (vs already-placed routes): geometric segment-intersection count.
- `crossings(r) = intra(r) + inter(r, placed)`. The inversion formula optimizes
  the intra term only; it is never a substitute for the inter term.

### 2.5 Cost and order (WEIGHTED — LAW REVISION 2026-06-20)
Per-route **shape cost** β (the §0 shape ladder, classified by shape not bend count):
```
β(r) = DOGLEG_PENALTY (1e9)  if doubles_back(r)              # line folds over itself
       0 | 1                 if straight | L
       2                     if two-bend ∧ ends SAME side of middle segment  # C
       Z_PENALTY (99)        otherwise (opposite-side two-bend, or ≥3 bends) # Z
```
**Weighted cost** (crossings can outweigh a bend):
```
cost(r)  = W_BEND·β(r) + W_CROSS·crossings(r) + W_LEN·length(r)
cost(D)  = W_BEND·Σβ  + W_CROSS·crossings(D)  + W_LEN·Σlength      (diagram total)
```
- Current weights (UNDER CALIBRATION on FlowForge): **`W_BEND=1`, `W_CROSS=3`,
  `W_LEN≈0`** (`place.rs`). Keep this block in sync with the constants.
- Consequence: a `C` (β=2) beats an `L` (β=1) when it removes more than
  `W_BEND/W_CROSS = 1/3` crossing per fan edge — i.e. a `k`-edge fan→C wins when
  it removes `> k/3` crossings. A Z/staircase (β=99) and a dogleg (β=1e9) both
  lose outright — no crossings/length trade reaches them.
- Selection = `argmin` weighted cost; ties `(displayIndex, relationshipId)`.

> **Superseded:** the strict lexicographic order `(β ≻ crossings ≻ length)`. It
> made the all-L router law-optimal at FlowForge β 138 / crossings 18, but the
> fan crossings were then irreducible (removing them needs a C's extra bend the
> order forbids). The maintainer revised the law to weighted; see §1.

---

## 3. Deterministic procedure

Two deterministic components, joined by exhaustive eviction, then a straightening
pass. Component 2 is reached only as a **proof** that no clean shape (straight / L /
**C arch**) clears `O` for that connection.

```
route_all(V, E):
    O      = { rect(v) : v ∈ V }
    placed = {}                                  # edge ↦ route
    for e in order(E):                           # most-constrained first,
                                                 # then (displayIndex, relationshipId)
        r = component1(e, O, placed)             # clean straight / L / C arch
        if r is None:
            r = component2(e, O, placed)         # last resort: a forced staircase
                                                 # (Z tier, β=99) only if no clean
                                                 # shape clears the field
        require ¬doubles_back(r) ∧ clears(r, O)  # H2, H3 — always, asserted
        placed[e] = r
    distribute_mounts(placed)                    # spread slots on shared surfaces
    straighten(placed)                           # §0 rule 6: undo any distribution
                                                 # that turned a straight into a jog,
                                                 # even at the cost of even spacing
    return placed


# ── Component 1 — clean straight / L / C-arch, exhaustive over surfaces × mounts ──
component1(e=(A,B), O, placed):
    C = [ cand
          for cand in surface_mount_product(A, B)        # all 4×4 surface pairs ×
          if feasible(cand, O) ]                          #   all mount slots, clearing O
    # a LIKE-facing surface pair (both N/S/E/W) yields a C arch (∩∪[]) — never a
    # flat straight/L grazing the plane; a FACING pair yields straight/L (or is
    # infeasible when offset — an offset facing jog would be a Z and is NOT emitted).
    if C is empty: return None
    return argmin_lex( c(route(cand)) for cand in C )     # (β, crossings, length),
                                                          # tie (displayIndex, id)

# ── Eviction (the multi-step, unbounded part) ──
# feasible() is evaluated against `placed`, so placing e may need a mount held by
# an already-placed edge f. Eviction re-resolves f to another feasible mount; that
# may displace g, and so on. The cascade is run to a fixpoint:
#
#   while ∃ placed edge that can lower the global lexicographic cost by moving to a
#         free feasible mount:  move it (in fixed edge order)
#
# It is unbounded in steps but finite in state (surfaces × mount slots × |E| is
# finite) and the global lex-cost strictly decreases on each accepted move, so it
# terminates. There is no give-up and no depth cap.

# ── Component 2 — forced staircase (last resort, Z tier) ──
component2(e=(A,B), O, placed):
    # No straight/L/C-arch clears O. Thread the route around O with the fewest
    # bends (Q1), then shortest (Q3). It never overlaps itself (¬doubles_back), so
    # it is not a dogleg — but ≥3 bends make it a staircase (β=99), a last resort.
    # (Realizable as a self-non-overlapping A* on the obstacle-induced grid,
    # lex-ordered (bends, length), fixed tie-break.)
    return min_bend_path(A, B, O)
```

---

## 4. Why this is deterministic (and what it is *not*)

- **Finite candidate space.** Per edge, Component 1 ranges over
  `4 × 4` surface pairs × a finite number of mount slots. Component 2 searches a
  finite obstacle-induced grid.
- **Total order on outcomes.** `c(·)` is lexicographic over `ℤ × ℤ × ℝ`; ties
  break on `(displayIndex, relationshipId)`, which is total on distinct edges.
  ⇒ every `argmin` is **unique**.
- **Fixed processing order.** `order(E)` is a fixed total order; the eviction
  cascade accepts moves in a fixed edge order and strictly decreases a bounded
  cost, so it reaches a **unique fixpoint** and terminates.
- **Component 2 is deterministic.** Monotone A* with reversal moves removed and a
  fixed lexicographic tie-break yields one path.

Therefore `route_all` is a pure function of `(V, E, mounts)`. ∎ (sketch)

**Honesty about scope.** This is **deterministic and greedy**, *not* globally
optimal. A fixed processing order makes the output reproducible (the parity
contract) but order-dependent; a globally minimal assignment would require
search/backtracking across edges, which is explicitly out of scope here. H1–H4
are guaranteed regardless; Q1–Q3 are minimized greedily, not globally.

---

## 5. Validation methodology (score first, visual last)

The score is the judge. Validation is a deterministic A/B **by number**: score
the current engine, score the new model on the same scenarios, and the model must
win — with the eyes used only to confirm the verdict, never to discover it.

### Diagram score
The aggregate of the per-route cost (§2.5):

```
S(D) = W_BEND·Σ β(r) + W_CROSS·crossings(D) + W_LEN·Σ length(r)   (weighted, §2.5)
     reported as the tuple (Σβ, crossings, Σlength) so each term stays visible.
```
- `β` encodes the dogleg as a 1e9 penalty and the Z/staircase as 99, so any baseline
  with reversals or jogs has its `Σβ` blown past any realistic clean total.
- the model's hard constraints (H2 + §0) mean its output is only straight / L / C —
  no Z, no staircase, no dogleg — so its `Σβ` is small; only a *baseline* (current
  engine) pays the 99s and 1e9s.
- **Measured (FlowForge, §0 law + arch builder, agent-turn-flow and corpus-wide):**
  the model emits **only clean shapes** — verified on the live `agent-turn-flow`:
  `{4 C, 4 straight, 10 L}`, **zero Z, zero staircase, zero dogleg**, including the
  `∩` arch `M 458 104 → 458 88 → 878 88 → 878 104` (WEE → Automation over Context
  Store). Corpus-wide `(β 182, crossings 22)` vs engine baseline
  `(β 1064, crossings 28, doglegs 9)`.
- **The §0 law raised crossings 10 → 22 on purpose.** The earlier `(β 149,
  crossings 10)` was bought by the coordinated router toggling fans to the facing
  `_|ˉ` **Z jog** (then mispriced as a "C" = 2). Under §0 those score 99 and are
  rejected, so the count now tells the truth (22, still < engine 28). **Shape
  legitimacy first; crossings are the next optimization** (no unjustifiable Z is an
  acceptable price for a lower crossing count).
- **Open (next):** (1) the §0-rule-6 **straightening pass** — undo slot distribution
  that turned a facing straight into a jog; (2) crossing reduction that stays within
  the {straight, L, C} shape space (route-aware channel routing with arches, not Z
  jogs).

### Procedure
1. **Baseline.** Score every known diagram (routing-corpus *and* FlowForge) under
   `S` from the **current engine's** output — the reference column. (Today the
   current engine scores `Σ doglegs = 3` on the corpus: it already fails the hard
   prefix on `bracket-intermediates` and `bundle-coaligned`. That is the first
   thing the model must fix.)
2. **Model.** Run the **same scenarios** through the deterministic model (§3);
   score under the **same** `S`.
3. **Compare, numerically.** The model must dominate the baseline lexicographically
   on every diagram: `doglegs → 0` first, then no regression on
   `bends / crossings / length`.
4. **Explain every miss.** If the model does *not* score better on some diagram,
   determine **why from recorded state** — which routes, which component, which
   constraint forced it. Never wave it off as acceptable variance. (Rule 17.)
5. **Visual is the last step.** Only after the numeric comparison passes does a
   diagram go to live review for the human verdict. Eyes confirm; they do not
   discover.

This needs no new harness beyond the `doglegs` metric already landed (#40): the
`corpus_fitness` gate is the baseline column, and the model is scored through the
same diagnostics. It also **supersedes** the earlier "feasibility-split"
framing — the score comparison subsumes it.

### Measure of last resort: the human-fixed oracle (validates the *score*)
When the automated baseline/RCA cannot tell whether a bad result is the score's
fault or the algorithm's, fall back to ground truth: the maintainer hand-reorders
a broken diagram into the form known to be correct, and we score that.

Let `S_oracle` = score of the hand-corrected diagram, `S_algo` = score of our
algorithm on the same scenario (lower is better; `Σ doglegs` hard-0 prefix):

- **`S_oracle ≥ S_algo`** → a hand-verified-correct layout failed to strictly beat
  our output on score. **The scoring model is broken** — it is not measuring
  correctness. Fix the *score/model* before trusting any further numeric
  comparison; algorithm tuning cannot rescue a mis-specified objective.
- **`S_oracle < S_algo`** → the score correctly ranked the known-good layout above
  our output. The score is valid for this case; **root-cause why the algorithm
  failed to reach `S_oracle`** from recorded state (Rule 17).

This is the **last** resort: it consumes the maintainer's hand-construction, so it
is used only when §5 steps 1–4 leave the score-vs-algorithm fault ambiguous. Its
real job is to keep the objective honest — it is the falsifiability check on `S`
itself.
