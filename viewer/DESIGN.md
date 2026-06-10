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
