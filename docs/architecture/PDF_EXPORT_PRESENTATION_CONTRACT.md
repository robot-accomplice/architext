# PDF Export Presentation Contract

PDF export is a browser print workflow for the active diagram artifact. Tests
must verify the export and print-style contract without scraping component
source text.

## Architecture

The app uses a small presentation helper for the browser print request:

- report a clear unavailable message when `window.print` is missing;
- report the browser-save-as-PDF guidance before printing;
- schedule `print()` on the next animation frame so the notice can render.

The shared diagram controls expose the action label from the same helper module.
Print CSS remains in `styles.css`, but tests inspect the `@media print` rule as
CSS data and assert the required behavior: chrome is hidden, and the diagram
artifact remains visible and unframed.

## Verification

- PDF export helper schedules print when available.
- PDF export helper does not schedule print when unavailable.
- Diagram control label comes from the presentation helper.
- Print CSS hides navigation/details/controls and preserves the diagram area.
