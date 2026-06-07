export const pdfExportControlLabel = "PDF";
export const pdfExportUnavailableMessage = "PDF export is unavailable in this browser.";
export const pdfExportReadyMessage = "Use the browser print dialog to save this view as PDF.";

export function requestPdfExport({ print, requestAnimationFrame }) {
  if (typeof print !== "function") {
    return { ok: false, message: pdfExportUnavailableMessage };
  }
  requestAnimationFrame(() => print());
  return { ok: true, message: pdfExportReadyMessage };
}
