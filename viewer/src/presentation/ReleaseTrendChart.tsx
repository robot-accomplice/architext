import { useState } from "react";
import type { Id, ReleaseSummary } from "../domain/architectureTypes.js";
import { formatReleaseDate, releaseTone } from "./releaseTruth.js";

export function ReleaseTrendChart({
  releases,
  activeReleaseId
}: {
  releases: ReleaseSummary[];
  activeReleaseId: Id;
}) {
  const [inspectedReleaseId, setInspectedReleaseId] = useState<Id | null>(null);
  const sorted = [...releases].sort((a, b) => (a.releasedAt ?? a.targetDate ?? a.targetWindow ?? "").localeCompare(b.releasedAt ?? b.targetDate ?? b.targetWindow ?? ""));
  const width = 1200;
  const height = 240;
  const padTop = 38;
  const padRight = 24;
  const padBottom = 78;
  const padLeft = 36;
  const baseline = height - padBottom;
  const xLabelY = baseline + 22;
  const maxCount = Math.max(1, ...sorted.flatMap((release) => [release.counts.features, release.counts.bugFixes]));
  const markerReleaseId = inspectedReleaseId ?? activeReleaseId;
  const markerIndex = Math.max(0, sorted.findIndex((release) => release.id === markerReleaseId));
  const xFor = (index: number) => sorted.length === 1 ? width / 2 : padLeft + (index * (width - padLeft - padRight)) / (sorted.length - 1);
  const yFor = (count: number) => baseline - (count * (baseline - padTop)) / maxCount;
  const pathFor = (key: "features" | "bugFixes") => sorted
    .map((release, index) => `${index === 0 ? "M" : "L"} ${xFor(index)} ${yFor(release.counts[key])}`)
    .join(" ");
  const areaFor = (key: "features" | "bugFixes") => {
    const line = sorted
      .map((release, index) => `${index === 0 ? "M" : "L"} ${xFor(index)} ${yFor(release.counts[key])}`)
      .join(" ");
    return `${line} L ${xFor(sorted.length - 1)} ${baseline} L ${xFor(0)} ${baseline} Z`;
  };
  const yTicks = Array.from(new Set([0, Math.ceil(maxCount / 2), maxCount]));

  return (
    <div className="release-history">
      <svg viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="xMinYMin meet" role="img" aria-label="Release feature and bug-fix count trend">
        <path className="release-chart-axis" d={`M ${padLeft} ${padTop} V ${baseline} H ${width - padRight}`} />
        {yTicks.map((tick) => (
          <g key={tick}>
            <path className="release-chart-tick" d={`M ${padLeft - 3} ${yFor(tick)} H ${width - padRight}`} />
            <text className="release-chart-y-label" x={padLeft - 7} y={yFor(tick) + 3} textAnchor="end">{tick}</text>
          </g>
        ))}
        <path className="release-chart-area feature" d={areaFor("features")} />
        <path className="release-chart-area fix" d={areaFor("bugFixes")} />
        <path className="release-chart-line feature" d={pathFor("features")} />
        <path className="release-chart-line fix" d={pathFor("bugFixes")} />
        <path className="release-chart-active-line" d={`M ${xFor(markerIndex)} ${padTop} V ${baseline}`} />
        {sorted.map((release, index) => {
          const releaseDateLabel = release.releasedAt
            ? `completed ${formatReleaseDate(release.releasedAt)}`
            : release.targetDate
              ? `target ${formatReleaseDate(release.targetDate)}`
              : release.targetWindow
                ? `target ${release.targetWindow}`
                : "no date recorded";
          return (
            <g
              key={release.id}
              role="listitem"
              tabIndex={0}
              aria-label={`${release.name}, ${releaseDateLabel}, ${release.counts.features} features, ${release.counts.bugFixes} bug fixes. Select it from the release list to inspect details.`}
              onClick={() => setInspectedReleaseId(release.id)}
              onFocus={() => setInspectedReleaseId(release.id)}
            >
              <title>{`${release.name} · ${releaseDateLabel} · ${release.counts.features} features · ${release.counts.bugFixes} bug fixes · select from the release list`}</title>
              <circle className={`${release.id === activeReleaseId ? "active" : ""} ${releaseTone(release.posture)}`} cx={xFor(index)} cy={yFor(release.counts.features)} r="3.5" />
              <text className="release-chart-x-label" x={xFor(index)} y={xLabelY} textAnchor="end" transform={`rotate(-65 ${xFor(index)} ${xLabelY})`}>{release.version}</text>
            </g>
          );
        })}
      </svg>
      <div className="release-chart-legend">
        <span><i className="feature" />Features</span>
        <span><i className="fix" />Bug fixes</span>
      </div>
    </div>
  );
}
