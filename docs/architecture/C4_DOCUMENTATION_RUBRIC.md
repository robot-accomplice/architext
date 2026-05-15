# C4 Documentation Rubric

Architext C4 views are architecture documents, not decorative variants of the
system map. They must make the selected abstraction level clear before routing
or styling is allowed to carry the burden.

Primary references:

- C4 Model notation guidance: https://c4model.com/diagrams/notation
- Structurizr automatic layout guidance: https://docs.structurizr.com/ui/diagrams/automatic-layout

## Required Quality Gates

Every C4 view must satisfy these gates:

- The view has a clear name, scope summary, visible boundary, and lane labels
  that explain the projection.
- A node ID appears at most once in a single C4 view.
- Relationships are labeled, unidirectional, and structural. They show uses,
  calls, reads/writes, publishes, or dependencies; they do not show workflow
  step numbers or selected-flow behavior.
- Relationship labels are visible by default and selectable when the diagram is
  dense.
- Dense views are split or scoped before adding routing complexity.
- Data must come from source-backed architecture facts. Do not invent runtime,
  protocol, or ownership claims to make a diagram look complete.

As a practical fitness gate, C4 views should stay below the density at which the
router has to recover through fallback paths. Current budgets are intentionally
conservative: context views should stay at or below 14 nodes and 18
relationships, container views at or below 14 nodes and 24 relationships, and
component views at or below 14 nodes and 28 relationships. If a project exceeds
those budgets, split the view by scope.

## Level Rules

Context views show the project or software system in relation to people and
external systems. They should not expose internal implementation detail unless
the current model has no more precise system boundary element yet.

Container views show deployable or runtime units inside the system boundary,
with external actors and systems retained only as context. Where available,
labels should expose interaction style or protocol.

Component views scope one selected container or runtime area. They should show
major components and collaborators that explain that container, not unrelated
runtime units from the whole project. External actors or clients may appear only
when they provide necessary context for that scoped component interaction.

## Renderer Responsibilities

The C4 renderer must use C4-specific spacing, padding, label clearance, and
boundary treatment independently from workflow diagrams. Workflow route
selection, selected-flow path highlighting, and numbered step markers must not
leak into C4 views.

The renderer must detect duplicate node membership in C4 views and surface it as
a document warning while rendering the node once. Duplicate rendering is a data
quality failure, not a layout feature.

## Acceptance Fixtures

Formal Architext acceptance must be self-contained. CI and package scripts must
not depend on sibling repositories such as Roboticus or Aegis being checked out
on the runner.

Local sibling repositories may still be used as informal litmus tests. Roboticus
and Aegis are useful because they represent different stress profiles:
Roboticus stresses UI/runtime surface boundaries, and Aegis stresses dense
blockchain component decomposition. Those checks are manual proving passes, not
formal lifecycle gates.

Acceptance requires:

- zero duplicate node IDs per C4 view;
- zero route collisions in bundled C4 views;
- fewer C4 document warnings than the previous five `nodes-too-close` warnings;
- route warnings that remain must explain real density or data issues rather
  than renderer failure.
