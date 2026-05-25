# Release Trend Chart Component

`main.tsx` must not own all Release Truth presentation components. Release
history charting is a bounded view concern and belongs in the presentation
directory.

## Architecture

The Release Trend chart is extracted into `presentation/ReleaseTrendChart.tsx`.
It owns chart-local hover/focus state and SVG geometry. Shared release
formatting, such as date display and release tone mapping, stays in
`presentation/releaseTruth.js` so the chart and the Release Truth workspace use
the same presentation vocabulary.

This is an incremental `main.tsx` decomposition slice. It does not change the
release data model, routing, selection state, or Release Planning workflow.

## Verification

- The extracted chart keeps the same props: release summaries and active release
  id.
- Release date formatting is covered in release presentation tests.
- The frontend build validates TypeScript imports and JSX wiring.
