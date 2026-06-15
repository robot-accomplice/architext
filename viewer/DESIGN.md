# Architext viewer — design language

One set of primitives shared by every mode (Flows, Sequence, C4, Deployment,
Data/Risks, Repo Tree, Blast Radius, Release Truth, Rules) so the product reads
as one thing, not nine. Tokens live in `:root` in `styles.css`.

## Primitives

### Overline (section labels & eyebrows)
Mono, uppercase, tracked, muted. One treatment everywhere via `.overline` or the
tokens `--overline-size` (11px) and `--overline-tracking` (0.08em).
Replaces ad-hoc labels whose letter-spacing currently ranges 0.04–0.16em.

### Accent rail (cards, rows, sections)
A 1px bordered surface with a colored 3px left rail driven by the `--accent`
custom property (`--accent-rail` width, `--card-radius`). Use `.accent-surface`,
or set `--accent` on an existing card. The rail color encodes meaning:
- **Repo Tree** rows → owning component (C4 type color)
- **Blast Radius** cards → relationship kind; chips → component C4 type
- **Notes** cards → note category
- **Detail panel** sections → section identity (`--section-accent`)

### Component-type color — single source of truth
Component/node type color **always** comes from the `--c4-*` tokens
(`--c4-client`, `--c4-service`, `--c4-worker`, `--c4-data`, `--c4-external`,
`--c4-actor`, `--c4-deployment`, `--c4-module`). Never hardcode raw
`--blue`/`--purple` for a type. The JS `C4_COLOR` map (repoTreeColors.js) must
reference `--c4-*`, not raw palette colors.

### Chips & count pills
Pill shape (`--chip-radius` for chips, `--pill-radius` for counts), bordered,
optional leading icon, optional type tint. One definition, not per-view.

## Known inconsistencies to remediate (cross-board)

1. **Two C4 color systems disagree.** `--c4-worker` is yellow in the diagram
   tokens but `C4_COLOR.worker` is purple in repoTreeColors.js. Fix: point
   `C4_COLOR` at the `--c4-*` tokens (single source) — resolves worker and
   guarantees Repo Tree / Blast Radius / C4 diagrams agree.
2. **Overline drift.** ~15 label rules with letter-spacing 0.04–0.16em and
   mixed sizes. Migrate to `.overline` / the tokens.
3. **Accent rail re-implemented under three names** (`--section-accent`,
   `--card-accent`, `--note-accent`) — converge on `--accent` + `.accent-surface`.
4. **Card radius drift** (blast 8px, notes 6px) — converge on `--card-radius`.

## Remediation sequencing

The new views (Repo Tree, Blast Radius, Notes) already follow this language but
their code is on unmerged feature branches; the older modes are on `main`. The
clean cross-board sweep — migrating every mode to `.overline` / `.accent-surface`
and fixing the `C4_COLOR` worker mismatch — should run as a single consolidation
**once the feature PRs (#89/#91/#92/#93) merge**, so it is one coherent,
visually-verifiable pass rather than a refactor split across branches.

(Items 1 and 3 above were since resolved by the `design-language-consolidation`
work; the section is kept for history.)

## 1.7.0 facelift target — "Cyber-Tactical Monolith" (PLANNED, not yet shipped)

The contract for the `design-language-facelift` scope item in release `v1-7-0`.
Explored externally (Stitch) and audited 2026-06-15; **locked here as the source
of truth so we implement in `viewer/src` without further mockup round-trips.** The
Stitch HTML is reference, not source. This is a **chrome-only** refactor: it must
not change diagram-layout-input *defaults* (see the rewrite spec §10). It evolves
the existing dark/neon-on-dark language above — it does not discard the token
architecture, which is what makes the audit's two open nits non-issues in the real
viewer (role color is single-sourced; the wordmark is one component).

### What is preserved (non-negotiable)
- **Single-source role color** via the `--c4-*` tokens and the `C4_COLOR` map. Every
  mode pulls role color from there, so "one hue per role across all screens" is
  enforced by construction. The Stitch screens disagreed (Actor cyan in Sequence vs
  purple in Repo Tree) only because hand-built HTML bypassed the tokens.
- `.overline`, `.accent-surface` (the 3px `--accent` rail), and the chip/pill
  primitives.

### Rules baked in from the audit
1. **State is never a role hue.** `active` / `selected` / primary-action use a
   dedicated state treatment — a 1px border/ring/inner-glow on `--accent` — NOT a
   `--c4-*` color. (Critical here: cyan is already a *role* color — `--c4-client` /
   `--c4-system` — so "cyan = active," as the Stitch notes proposed, would collide.
   State gets its own signal.)
2. **Wordmark renders "ARCHITEXT" everywhere**, from one component. (Source already
   says ARCHITEXT; two Stitch screens still *rendered* "HITEXT" — a mockup artifact
   that cannot occur with a single shared header component.)
3. **The diagram canvas is fluid.** Computed `canvasWidth/canvasHeight` inside a
   pan/zoom/scale-to-fit viewport, with explicit zoom / fit controls; never pinned to
   a fixed `w-[…]/h-[…]`. (Rewrite spec §10.)
4. **One canvas-black.** A single darkest surface token for diagram backgrounds.

### Typography
- **Hanken Grotesk** — UI navigation, headers, content (humanist-grotesque, tight
  tracking). Display 32/700, headline 20/600, body 14/400.
- **JetBrains Mono** — all technical data: file paths, tree items, metrics, step
  numbers, status chips, and the `.overline` caps labels (11/700, 0.08em — already
  the `--overline-*` tokens).
- Body pinned to 14px for density; two weights only (400 / 500-ish), sentence case.

### Layout & spacing
- **Fixed-fluid console grid:** fixed-width left nav (~280px) and right inspector
  (~320px) flanking a **fluid** central canvas — the left-nav / canvas /
  right-inspector triad the audit called the redesign's biggest win.
- **4px baseline grid;** tight component padding (8–12px); **1px hairline panel gaps**
  (not wide margins) for the single-machine "console" feel.
- Reflow: auxiliary panels collapse to icons / drawers on narrow viewports; the
  diagram keeps priority.

### Shape & elevation
- Soft-geometric: 4px radius for UI chrome, up to 8px for diagram node cards; pills
  reserved for status indicators only. Node cards carry a 2px category top-bar in the
  node's `--c4-*` role color.
- Hierarchy via tonal layering + 1px outlines, not heavy shadows. Selected nodes get
  the `--accent` inner-glow (rule 1). Glass reserved for the command palette / global
  search overlay.

### Surfaces (tiered blacks)
Adopt the tiered-black surface ramp (background → panel → popover) on top of the
existing `--bg`/`--surface` tokens; tune exact values against a secondary-text
contrast target (the audit flagged muted-gray-on-near-black as hard to scan). Role
hues stay governed by `--c4-*`; only their exact hex may be refreshed, and only with
one hue per role held across every mode.
