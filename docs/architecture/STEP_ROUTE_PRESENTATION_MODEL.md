# Step Route Presentation Model

Flow and sequence step overlays share one SVG route presentation contract. Tests
must assert that contract through exported presentation helpers rather than
matching `main.tsx` source text.

## Architecture

`StepRoute` owns the SVG marker and label structure. View-specific callers
choose only a route kind (`flow` or `sequence`) and pass route geometry. Shared
helper functions derive the group, marker, label, and sequence message classes
from that kind, optional caller-specific class names, and sequence semantics.

This keeps the contract stable while allowing the main app and `StepRoute`
component to be refactored without rewriting tests.

Sequence routes have additional semantics because a sequence diagram is not
just a numbered list of messages. Return messages must be classified explicitly
from `step.kind`/`step.returnOf` when the data provides it, and otherwise from
clear return language or reverse lifeline direction. Rendered returns must be
visually distinct from outbound requests, and a paired return should close an
activation bar on the participant that handled the outbound message. Loop,
retry, optional, and transaction frames are flow data, not CSS decoration; the
renderer only draws them when the selected flow records the governed step ids.

Branched flow decisions are one semantic step with multiple rendered route
fragments. The main step route enters the component that makes the decision, a
short connector attaches that component to a separate decision diamond, and
outcome branches leave the diamond. Outcome labels are pills anchored to their
branch lines. Branch support records do not create separate step-strip items and
do not relabel target components as decisions.

When a decision outcome returns to an earlier component in the same lane, prefer
the component's western surface over its bottom surface. The branch is an
outcome return, not another downward request, and the western entry usually
keeps the route shorter with fewer corners while avoiding the decision
connector's endpoint.

Line-hop rendering is based on visible route surfaces, not just ordinary flow
relationships. If a flow route crosses a visible decision connector or another
visible route, the rendered route must either avoid that crossing or draw an
explicit hop at the crossing.

## Verification

- Flow and sequence kinds map to distinct wrapper classes.
- Marker and label class projection preserves the shared base classes.
- Branched decisions render as one selectable step with attached outcome branch
  routes.
- Visible flow-route crossings are avoided or rendered with hop-over geometry,
  including crossings against decision connectors.
- Sequence return classification distinguishes outbound, return, async,
  persistence, and self messages.
- Sequence return pairing can connect an explicit `returnOf` edge or a clear
  reverse-direction return to its outbound source message.
- Sequence activation spans start at outbound/self messages and end at their
  paired return when one exists.
- Tests do not read `main.tsx` as source.
