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
- **H2 — Monotonicity (no dogleg).** The path is monotone in each axis: `x(·)` is
  non-decreasing *or* non-increasing along the whole route, and likewise `y(·)`.
  Equivalently, a heading and its reverse never both occur. The route **never
  doubles back.** This is absolute.
- **H3 — Clearance.** No segment crosses the interior of any node in
  `O \ {A, B}`; the first/last segment does not re-enter `A`/`B`.
- **H4 — Face correctness.** The route leaves `A` outward through its chosen
  surface and enters `B` inward through its chosen surface
  (`first_dir · n_{sA} > 0`, `last_dir · n_{sB} < 0`).

### Objectives (Q) — minimize, strict lexicographic priority
1. **Q1 — bend score** `β(r)`: a **shape-weighted** score, *not* a raw turn count
   (maintainer, 2026-06-20). Two routes with the same number of turns are not
   equal — a clean turn and a doubling-back turn score very differently:

   | shape | turns | monotone? | `β` |
   |---|---|---|---|
   | straight | 0 | yes | **0** |
   | L | 1 | yes | **1** |
   | C | 2 | **yes** (clean, never doubles back) | **2** |
   | Z | 2 | **no** (reverses heading) | **99** = `REVERSAL_BEND_PENALTY` |

   General rule: `β(r) = bends(r)` if `monotone(r)`, else `REVERSAL_BEND_PENALTY`.
   So `C` and `Z` tie on count (2) but separate on `β` (2 vs 99). `99` is a named
   constant, never a literal (project rule 3).
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
(dogleg/reversal still a hard exclusion at `REVERSAL_BEND_PENALTY`=99). A C beats
an L when `W_b < Δcrossings · W_x`. The text below is the **superseded** strict
order, kept for provenance.

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

This folds the old separate `doglegs` term into `β`: a doubling-back route is
simply one whose `β = 99`. The model never *emits* one (H2); the score still
*represents* one, so a baselined dogleg loses decisively.

The full order is therefore fixed: `β`, then `crossings`, then `length`,
then `(displayIndex, relationshipId)` for determinism.

### Determinism (D)
`r : (V, O, E, mounts) ↦ routes` is a **pure function.** Identical input ⇒
identical output. No randomness; every ordering used is a fixed total order.
(Required by the plan-farm / parity contract.) Determinism is required;
*global optimality is not* — the procedure is greedy under a fixed order
(see §4).

### Shape taxonomy (a consequence of H2 + Q1, not a separate rule)
| Shape | bends | monotone? | status |
|---|---|---|---|
| straight | 0 | yes | preferred |
| L | 1 | yes | preferred |
| staircase | ≥2 | **yes** | forced-only; minimized |
| **dogleg** | ≥2 | **no** | **excluded by H2 — never produced, not ranked** |

A dogleg is not a high-cost option; it fails H2, so it is never a candidate.

---

## 2. Mathematical model

### 2.1 Monotonicity ≡ dogleg-free (the formal core)
Let `P = (q₀, q₁, …, q_k)`, `sx_i = sign(q_{i+1}.x − q_i.x)`, `sy_i` likewise.

```
monotone(P)  ⟺  |{ sx_i : sx_i ≠ 0 }| ≤ 1   ∧   |{ sy_i : sy_i ≠ 0 }| ≤ 1
dogleg(P)    ⟺  ¬monotone(P)   (both +1 and −1 occur on some axis)
H2           ≡  monotone(P)
```
A monotone route lies entirely within the bounding box
`Box(p_a, p_b) = [min x, max x] × [min y, max y]` (monotone coordinates stay
between their endpoints). This bounds the search: **a dogleg-free route never
leaves the endpoints' bounding box** — except when H3 forces a detour, which is
exactly the boundary between the two solution components (§3).

### 2.2 Bends
For a reduced orthogonal path, `bends(P) = k − 1`. `straight ⟺ k=1`;
`L ⟺ k=2`; `staircase ⟺ k ≥ 3 ∧ monotone(P)`.

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
Per-route **shape cost** β (the shape ladder, not a raw bend count):
```
β(r) = bends(r)             if monotone(r)
       REVERSAL_BEND_PENALTY  otherwise        # = 99
     ⇒ straight 0, L 1, C 2, staircase k, Z(reversal) 99
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
  it removes `> k/3` crossings. The dogleg still loses outright (β=99 dominates).
- Selection = `argmin` weighted cost; ties `(displayIndex, relationshipId)`.

> **Superseded:** the strict lexicographic order `(β ≻ crossings ≻ length)`. It
> made the all-L router law-optimal at FlowForge β 138 / crossings 18, but the
> fan crossings were then irreducible (removing them needs a C's extra bend the
> order forbids). The maintainer revised the law to weighted; see §1.

---

## 3. Deterministic procedure

Two deterministic components, joined by exhaustive eviction. Component 2 is
reached only as a **proof** that no straight/L clears `O` for that connection.

```
route_all(V, E):
    O      = { rect(v) : v ∈ V }
    placed = {}                                  # edge ↦ route
    for e in order(E):                           # most-constrained first,
                                                 # then (displayIndex, relationshipId)
        r = component1(e, O, placed)             # clean straight / L
        if r is None:
            r = component2(e, O, placed)         # forced monotone staircase
        require monotone(r) ∧ clears(r, O)       # H2, H3 — always, asserted
        placed[e] = r
    return placed


# ── Component 1 — clean straight / L, exhaustive over surfaces × mounts ──
component1(e=(A,B), O, placed):
    C = [ cand
          for cand in surface_mount_product(A, B)        # all 4×4 surface pairs ×
          if feasible(cand, O) ]                          #   all mount slots, clearing O
    if C is empty: return None
    #   crossings() applies co-monotone mount reordering within shared bundles
    return argmin_lex( c(route(cand)) for cand in C )     # (bends,crossings,length),
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

# ── Component 2 — forced monotone detour, fewest bends ──
component2(e=(A,B), O, placed):
    # No straight/L clears O. Draw the MONOTONE route around O with the fewest
    # bends (Q1), then shortest (Q3). Reversal moves are forbidden, so a dogleg
    # cannot arise. (Realizable as monotone A* on the obstacle-induced grid with
    # the −heading move set removed, lex-ordered (bends, length), fixed tie-break.)
    return min_bend_monotone_path(A, B, O)
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
- `β` encodes the dogleg as a 99 penalty, so a routing with *any* reversal has its
  `Σβ` blown past any realistic clean total — doglegs lose regardless of weights.
- the model's hard constraint H2 (never emit a reversal) means its output always
  has `Σ β = Σ bends`; only a *baseline* (current engine) pays the 99s.
- **Measured (FlowForge, current engine → model):** all-L `route_all_slotted`
  scores `(β 138, crossings 18)`; the coordinated per-fan router at `W_CROSS=3`
  scores `(β 145, crossings 12)` — fewer crossings for a small β rise, the
  weighted-law tradeoff. Engine baseline `(β 1064, crossings 28, doglegs 9)`.
  `W_CROSS` is under calibration: higher → fewer crossings, more C-bends.

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
