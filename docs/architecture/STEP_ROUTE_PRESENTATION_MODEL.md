# Step Route Presentation Model

Flow and sequence step overlays share one SVG route presentation contract. Tests
must assert that contract through exported presentation helpers rather than
matching `main.tsx` source text.

## Architecture

`StepRoute` owns the SVG marker and label structure. View-specific callers
choose only a route kind (`flow` or `sequence`) and pass route geometry. Shared
helper functions derive the group, marker, and label classes from that kind and
from optional caller-specific class names.

This keeps the contract stable while allowing the main app and `StepRoute`
component to be refactored without rewriting tests.

## Verification

- Flow and sequence kinds map to distinct wrapper classes.
- Marker and label class projection preserves the shared base classes.
- Tests do not read `main.tsx` as source.
