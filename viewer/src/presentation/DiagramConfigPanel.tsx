import React from "react";
import type { DiagramConfig } from "./diagramConfigContext.js";

export interface DiagramFieldSpec {
  default: number;
  min: number;
  max: number;
  step: number;
  unit: string;
  label: string;
}
export type DiagramFieldsSpec = Record<string, Record<string, DiagramFieldSpec>>;
export type DiagramSectionLabels = Record<string, string>;

// Sections whose controls take effect live in this build. The legibility gap
// control is added once the routing gap-knob refactor lands (the router reads
// MIN_LEGIBLE_GAP at module load, so a gap slider can't preview until then).
const ACTIVE_SECTIONS = new Set(["layout", "sequence", "zoom"]);

function readField(value: DiagramConfig, section: string, field: string, fallback: number): number {
  const sectionValue = (value as Record<string, Record<string, number>> | null)?.[section];
  const current = sectionValue?.[field];
  return typeof current === "number" ? current : fallback;
}

export function DiagramConfigPanel({
  fields,
  sections,
  value,
  saved,
  busy,
  message,
  onChange,
  onSave,
  onRevert,
  onResetDefaults,
  onClose
}: {
  fields: DiagramFieldsSpec;
  sections: DiagramSectionLabels;
  value: DiagramConfig;
  saved: DiagramConfig;
  busy: boolean;
  message: string | null;
  onChange: (section: string, field: string, next: number) => void;
  onSave: (scope: "project" | "user") => void;
  onRevert: () => void;
  onResetDefaults: () => void;
  onClose: () => void;
}) {
  const dirty = JSON.stringify(value) !== JSON.stringify(saved);

  return (
    <aside className="diagram-config-panel" aria-label="Diagram settings">
      <header className="diagram-config-header">
        <h3>Diagram settings</h3>
        <button type="button" className="quiet-action" onClick={onClose} aria-label="Close diagram settings">×</button>
      </header>

      <div className="diagram-config-body">
        {Object.entries(fields)
          .filter(([section]) => ACTIVE_SECTIONS.has(section))
          .map(([section, sectionFields]) => (
            <section key={section} className="diagram-config-section">
              <h4>{sections[section] ?? section}</h4>
              {Object.entries(sectionFields).map(([field, spec]) => {
                const current = readField(value, section, field, spec.default);
                const isDefault = current === spec.default;
                return (
                  <div key={field} className="diagram-config-field">
                    <label className="diagram-config-field-label" htmlFor={`cfg-${section}-${field}`}>
                      {spec.label}
                    </label>
                    <div className="diagram-config-field-controls">
                      <input
                        type="range"
                        min={spec.min}
                        max={spec.max}
                        step={spec.step}
                        value={current}
                        aria-label={`${spec.label} slider`}
                        onChange={(event) => onChange(section, field, Number(event.target.value))}
                      />
                      <input
                        id={`cfg-${section}-${field}`}
                        type="number"
                        min={spec.min}
                        max={spec.max}
                        step={spec.step}
                        value={current}
                        onChange={(event) => onChange(section, field, Number(event.target.value))}
                      />
                      <span className="diagram-config-unit">{spec.unit}</span>
                      <button
                        type="button"
                        className="diagram-config-field-reset"
                        disabled={isDefault}
                        title={`Reset to default (${spec.default}${spec.unit})`}
                        aria-label={`Reset ${spec.label} to default`}
                        onClick={() => onChange(section, field, spec.default)}
                      >
                        ↺
                      </button>
                    </div>
                  </div>
                );
              })}
            </section>
          ))}
      </div>

      <footer className="diagram-config-footer">
        {message ? <p className="diagram-config-message">{message}</p> : null}
        <div className="diagram-config-actions">
          <button type="button" className="quiet-action" onClick={onResetDefaults} disabled={busy}>Reset to defaults</button>
          <button type="button" className="secondary-action" onClick={onRevert} disabled={busy || !dirty}>Revert</button>
          <button type="button" className="secondary-action" onClick={() => onSave("user")} disabled={busy}>Save to user</button>
          <button type="button" className="primary-action" onClick={() => onSave("project")} disabled={busy}>Save to project</button>
        </div>
      </footer>
    </aside>
  );
}
