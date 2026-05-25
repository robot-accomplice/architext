# Collapsible Release Path

Release Path milestones can become long after a release group is complete. The
path still needs to preserve the release schedule outline, but completed groups
should not force readers to scroll through every finished item.

## Architecture

Collapse state is local presentation state owned by
`presentation/ReleasePathView.tsx`.

This is intentionally not stored in Release Truth data. The expanded/collapsed
shape is a viewer preference for the current dashboard session, not release
metadata, and it should not create writes to the target repository.

Each Release Path milestone keeps its summary row visible when collapsed:

- path number;
- release status;
- milestone label;
- timing and item count, including `X/Y complete`;
- active blocker summary.

Collapsed milestones hide only their item rows. Selection remains unchanged: the
milestone row can still be selected while the item rows are hidden, and expanding
the milestone restores the same item list.

The collapse state transition is isolated in a small presentation helper so it
can be tested without rendering React.

## Verification

- Unit tests cover collapse-state toggling and immutability.
- The production frontend build validates the React wiring and CSS selectors.
- Browser verification checks that Release Path renders, a completed milestone
  can collapse, and the item rows are hidden from the page.
