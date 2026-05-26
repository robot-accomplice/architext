# Routing Grid Budgets

Orthogonal routing may escalate from cheap route candidates to a grid search
when direct and corridor routes are not clean enough. The grid is derived from
candidate x/y lines around blockers, so dense diagrams can grow the search space
quadratically.

## Contract

Grid routing is an optimization, not the only correctness path. It must have
bounded work:

- refuse grids with more points than the configured point budget
- stop Dijkstra when visited expansions exceed the configured expansion budget
- record budget bailouts in routing stats when stats are supplied
- return `null` on budget exhaustion so the existing perimeter candidates can
  provide a deterministic fallback route

The fallback may be less pretty than a fully searched grid route, but it keeps
the worker responsive and returns a finite route for the diagram.

## Verification

- Existing routing quality and benchmark tests continue to pass under the
  default budgets.
- A deliberately small grid budget causes grid route bailout stats and still
  returns a finite orthogonal fallback route.
