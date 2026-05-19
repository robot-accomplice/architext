import React from "react";

type StepRouteProps = {
  className: string;
  lineClassName: string;
  markerClassName?: string;
  labelClassName?: string;
  d?: string;
  x1?: number;
  y1?: number;
  x2?: number;
  y2?: number;
  markerEnd: string;
  labelX: number;
  labelY: number;
  label: React.ReactNode;
};

export function StepRoute({
  className,
  lineClassName,
  markerClassName = "",
  labelClassName = "",
  d,
  x1,
  y1,
  x2,
  y2,
  markerEnd,
  labelX,
  labelY,
  label
}: StepRouteProps) {
  return (
    <g className={className}>
      {d ? (
        <path className={lineClassName} d={d} markerEnd={markerEnd} />
      ) : (
        <line className={lineClassName} x1={x1} y1={y1} x2={x2} y2={y2} markerEnd={markerEnd} />
      )}
      <g className="route-step-pill">
        <rect className={`route-step-marker ${markerClassName}`.trim()} x={labelX - 12} y={labelY - 10} width="24" height="20" rx="10" />
        <text className={`route-step-label ${labelClassName}`.trim()} x={labelX} y={labelY + 4}>{label}</text>
      </g>
    </g>
  );
}
