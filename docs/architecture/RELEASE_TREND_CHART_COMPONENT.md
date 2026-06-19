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

The chart must render edge-to-edge within its section without distorting the
data geometry. The SVG keeps one fixed viewBox aspect ratio and fills available
container width with `height: auto`; it must not use CSS stretching or
`preserveAspectRatio="none"`. Axis labels, rotated release labels, active
markers, and the rightmost data point stay inside the viewBox through chart
padding rather than relying on clipping or overflow outside the SVG.

This is an incremental `main.tsx` decomposition slice. It does not change the
release data model, routing, selection state, or Release Planning workflow.

## Verification

- The extracted chart keeps the same props: release summaries and active release
  id.
- Release date formatting is covered in release presentation tests.
- CSS tests cover the chart's responsive aspect-ratio contract.
- The frontend build validates TypeScript imports and JSX wiring.
