# Release Truth Plan

## Summary

Architext should treat release status as first-class project architecture data,
not as an external handmade status page. The UI and feature set is named
**Release Truth**: a repository-owned source of truth for what release the
project is working toward, release posture, completed and in-progress
workstreams, blockers, dependencies, milestones, historical releases, and
release trend metrics through JSON data that the viewer renders dynamically.

The early static prototype was content inventory only. It is not a visual target.
The implementation should use Architext's existing dense engineering interface:
left navigation, central structured views, right detail panel, compact cards,
filters, and data-driven drill-downs.

## Goals

- Show the current target release and its overall posture at a glance.
- Show release scope as structured workstreams and release items, not prose.
- Separate required scope, planned scope, stretch scope, completed work, in
  progress work, and blocked work.
- Show release blockers with owner, severity, dependency, next action, and
  evidence requirements.
- Show milestone progress with concrete dates or target windows.
- Show a progress bar derived from item/workstream status, not hand-maintained
  text.
- Show `lastUpdated` and update provenance so stale status is visible.
- Let users navigate historical releases without loading full historical detail
  files up front.
- Let users compare release history with trend charts, including feature count
  and bug-fix count by release date.

## Data Shape

Release tracking should use an index-plus-detail file structure:

```text
docs/architext/data/
  manifest.json
  releases/
    index.json
    v1-1-2.json
    v1-2-0.json
```

`manifest.json` should reference the release index, not every historical
release file:

```json
{
  "files": {
    "releases": "releases/index.json"
  }
}
```

`releases/index.json` should stay small and cheap to load. It is the generated
history/navigation projection, not the canonical release fact source. Release
detail files own release facts; lifecycle tooling should refresh index
summaries, counts, and historical chart inputs from those detail files.

```json
{
  "currentReleaseId": "v1-2-0",
  "releases": [
    {
      "id": "v1-1-2",
      "version": "1.1.2",
      "name": "Architext 1.1.2",
      "status": "released",
      "posture": "shipped",
      "targetDate": "2026-05-16",
      "releasedAt": "2026-05-16T09:20:00.000Z",
      "lastUpdated": "2026-05-16T09:20:00.000Z",
      "summary": "Dense spline routing and dense topology layout improvements.",
      "counts": {
        "features": 0,
        "bugFixes": 2,
        "workstreams": 1,
        "blockers": 0,
        "complete": 3,
        "inProgress": 0,
        "planned": 0,
        "stretch": 0
      },
      "file": "v1-1-2.json"
    }
  ]
}
```

Each detail file owns the full release snapshot:

- release identity: `id`, `version`, `name`, `status`, `posture`, `summary`
- dates: `targetDate`, `releasedAt`, `lastUpdated`
- scope: required, planned, stretch, deferred, and out-of-scope items
- release decisions: priority, ordering, deferral rationale, and decision
  provenance when an LLM or maintainer changes scope during release work
- workstreams: area, owner, status, posture, summary, progress, evidence
- blockers: severity, owner, dependency, next action, evidence needed
- milestones: label, status, date/window, order, linked release items
- dependencies: item-to-item and external dependency references
- counts: feature/fix/blocker/status counters derived into the index from the
  detail file
- evidence: links or local paths to validation notes, CI runs, manual drills,
  screenshots, or release notes

## Status Vocabulary

Release status should be narrow and consistent:

- `planning`
- `active`
- `blocked`
- `candidate`
- `released`
- `deferred`

Release item status should be similarly constrained:

- `planned`
- `in-progress`
- `blocked`
- `complete`
- `deferred`
- `stretch`
- `cut`

Posture should be a snapshot label, not a status duplicate:

- `on-track`
- `at-risk`
- `blocked`
- `release-candidate`
- `shipped`

## Release Truth Viewer

Release Truth should add a native top-level mode rather than overloading
Flows/Data/Risks.

Release Truth exists to make LLM-assisted release work auditable. A user should
not have to infer progress, priority, or deferrals from chat history. The shared
JSON source of truth must show what the project is working toward, what has been
completed, what is still blocked, what was deferred, and why the release order
changed. If an LLM makes or proposes a release-scope decision in a session, the
resulting Release Truth update should make that decision visible for user review.

Primary Release Truth views:

- **Current Release:** snapshot of target version, posture, scope, blockers,
  progress, milestones, and next actions.
- **Release Path:** the single milestone/workstream model. Milestones are not a
  separate artifact from the path; they are the coarse rows derived from release
  path grouping and order. Each coarse row contains an indented vertical list of
  atomic sub-milestones/work items. Blocker data is state on the blocked item,
  not a separate sibling row. The center Release Path is a compact navigable
  status index: every line must show completion, state, title, and enough scope
  metadata to scan progress quickly. Long rationale, blocker explanation,
  dependency detail, evidence, and next action belong in the right details pane
  for the selected milestone or item.
- The Release Path must represent the full release scope expected to ship, not
  only the current session's active work or release-management gates. Completed
  product/project changes, validation evidence, release blockers, deferrals, and
  final ceremony work all belong in the same release path so the user can judge
  the true shape of the release.
- Release Path rendering must not hide items that are missing milestone links.
  Required, planned, stretch, deferred, and out-of-scope items all need a visible
  home. Unlinked items should be surfaced under an explicit catch-all milestone
  rather than silently disappearing.
- **Blockers:** integrated into the related Release Path item as item state,
  with severity and unblock action visible inline. A separate blocker wall is
  acceptable only as a future filter view, not as the primary current-release
  experience.
- **Milestones:** never render as standalone labels. Without linked work items,
  owners, and workstream context, they are not useful enough to occupy their own
  section.
- **History:** navigable historical release list and trend chart.

The History view should initially load only `releases/index.json`. Selecting a
historical release loads that release's detail file on demand.

Historical releases give the user a cadence and composition view: how often the
project ships, how much scope each release carries, and whether historical
delivery trends toward features, fixes, validation, or churn. Feature and bug-fix
counts are the first required trend lines because they answer different release
health questions than the current-release progress bar.

## State Color Semantics

Release Truth depends on fast state recognition. Color must be semantic and
consistent across release cards, badges, progress bars, workstreams, blockers,
milestones, and historical trend markers:

- Green: shipped, complete, on-track, and otherwise healthy.
- Yellow: planned, active, in-progress, release-candidate, stretch, and work
  that is progressing but not complete.
- Red: blocked, at-risk, critical, and high-severity blockers.
- Muted neutral: deferred, cut, historical, or intentionally inactive scope.
- Cyan: structural selection and navigational focus, not release state.

Color must not be the only state signal. Every colored element also needs text,
shape, position, border treatment, or progress context so screenshots and
low-contrast displays remain readable.

Progress bars must encode completeness, not decorative variety and not the
release status badge. Use muted/inactive for no progress, then transition from
yellow toward green as completion increases. Red is reserved for blocked items,
high-severity blockers, and release posture/status warnings.

## Historical Navigation And Trends

The release index should contain enough summary data to render historical
navigation without fetching every detail file:

- release version/name
- release date or target date
- status/posture
- summary
- feature count
- bug-fix count
- blocker count at ship time
- total scope count
- detail file path

The historical release view should include:

- release timeline/list sorted by date
- filters for released, active, blocked, deferred, and planned releases
- full-width filled trend chart at the bottom of the current-release canvas
  with release date on the X axis and counts on the Y axis
- one filled series for feature count
- one filled series for bug-fix count
- hover/focus on a release point reveals the release date and summary counts
- chart points are informational by default; selection should remain an
  explicit app control such as the release list so users do not lose the
  current release by brushing across the chart
- optional secondary series for blockers, scope size, or stretch items
- provide an in-app return to the current release when inspecting history

The chart should be compact, low-profile, and visually aligned with Architext's
existing technical interface. It should not become a marketing dashboard or a
separate visual language from the rest of Architext.

## Derived Metrics

The viewer should derive useful metrics when possible:

- percent complete: complete required items divided by total required items
- blocker count by severity
- overdue milestone count
- workstream completion count
- planned vs stretch scope split
- released feature/fix trend over time

Schema should still store summary counts in `releases/index.json` so history can
render quickly, but those counts are generated metadata. Detail pages are the
source of truth, and `validate`/`doctor` should detect stale generated history.

## CLI And Validation

Validation should cover:

- current release id exists in the index
- every index entry points to a detail file path
- release detail ids match their index entry
- item dependencies reference existing release items
- milestones reference existing release items
- blockers reference existing items or external dependencies
- historical counts are non-negative integers
- released entries include `releasedAt`
- active/planned entries include target date or target window

CLI commands should not require a new lifecycle. Existing commands should pick
up release data automatically:

- `architext validate [path]` validates release index and loaded detail files.
- `architext doctor [path]` reports missing current release files, stale
  `lastUpdated`, broken dependencies, and count mismatches, and can regenerate
  release history/index metadata from release detail files.
- `architext serve [path]` exposes release index and detail files through the
  local data server.
- `architext build [path]` includes release index and detail files in static
  output.

## Implementation Order

1. Add release index/detail schemas and manifest support.
2. Add release data adapter that loads the index first and detail files on
   demand.
3. Add Architext self release fixture data for the latest released patch and
   the next planned release.
4. Add validation/reference tests for release dependencies and index/detail
   consistency.
5. Add Release Truth mode selection and current-release snapshot UI.
6. Add workstream/blocker/milestone detail panels.
7. Add historical release navigation.
8. Add the feature/fix trend chart.
9. Add doctor reporting for stale or inconsistent release status data.

## Initial Implementation Slice

The first implementation slice should establish the reusable Release Truth
foundation without making existing data-only repositories invalid:

- `manifest.files.releases` is optional.
- When present, it points to a small release index under
  `docs/architext/data/releases/index.json`.
- The viewer loads the release index with the rest of the manifest file graph
  and loads detail files for index entries on demand.
- `architext validate [path]` validates the release index, validates loaded
  release detail files, and checks id/dependency/milestone references.
- The Architext self model includes release data for the latest released
  version and the next Release Truth target release.
- Fresh `architext sync [path]` installs seed neutral Release Truth starter data
  so users have a CLI-created path into the feature.
- The first UI is a native Release Truth mode with current-release snapshot,
  workstream/blocker/milestone cards, and a compact history/trend view.

Later slices can add richer doctor repair suggestions and more advanced
historical analytics once the data contract is proven.

## Non-Goals

- Do not copy one-off prototype styling.
- Do not create an operational publishing runbook in the public README.
- Do not require all historical release detail files to load on startup.
- Do not rewrite release detail facts silently during `sync`; regenerating the
  release history index from existing detail facts is allowed because it is
  derived metadata.
- Do not make Release Truth dependent on GitHub or npm network access.
