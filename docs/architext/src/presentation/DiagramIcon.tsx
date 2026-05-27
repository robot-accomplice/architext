import React from "react";
import { iconLabel } from "./diagramIconModel.js";

const iconPaths: Record<string, string> = {
  actor: "M12 5a3 3 0 1 0 0.1 0 M12 8v9 M8 12h8 M9 21l3-4 3 4",
  artifact: "M7 3h7l3 3v15H7z M14 3v4h4 M9 11h6 M9 15h6",
  client: "M4 5h16v11H4z M9 20h6 M12 16v4",
  database: "M6 6c0-2 12-2 12 0v12c0 2-12 2-12 0z M6 6c0 2 12 2 12 0 M6 12c0 2 12 2 12 0",
  decision: "M12 3l9 9-9 9-9-9z",
  external: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18 M3 12h18 M12 3c3 3 3 15 0 18 M12 3c-3 3-3 15 0 18",
  module: "M4 7h16v12H4z M4 7l4-4h12v12l-4 4 M20 3v12",
  node: "M5 5h14v14H5z",
  package: "M4 8l8-4 8 4v8l-8 4-8-4z M4 8l8 4 8-4 M12 12v8",
  process: "M5 7h14v10H5z M8 10h8 M8 14h5",
  queue: "M5 6h14 M5 12h14 M5 18h14 M8 4v4 M8 10v4 M8 16v4",
  return: "M9 7l-5 5 5 5 M4 12h11a5 5 0 0 0 0-10h-2",
  service: "M5 5h14v14H5z M8 9h8 M8 13h8 M8 17h4",
  shield: "M12 3l7 3v5c0 5-3 8-7 10-4-2-7-5-7-10V6z",
  start: "M8 5l10 7-10 7z",
  stop: "M7 7h10v10H7z",
  system: "M4 6h16v12H4z M7 9h10 M7 13h10",
  worker: "M12 7v-3 M12 20v-3 M7 12H4 M20 12h-3 M8.5 8.5L6.3 6.3 M17.7 17.7l-2.2-2.2 M15.5 8.5l2.2-2.2 M6.3 17.7l2.2-2.2 M9 12a3 3 0 1 0 6 0 3 3 0 0 0-6 0"
};

export function DiagramIcon({
  icon,
  className = ""
}: {
  icon: string;
  className?: string;
}) {
  const label = iconLabel(icon);
  const path = iconPaths[icon] ?? iconPaths.node;
  return (
    <svg className={`diagram-icon ${className}`.trim()} viewBox="0 0 24 24" aria-label={label} role="img">
      <path d={path} />
    </svg>
  );
}
