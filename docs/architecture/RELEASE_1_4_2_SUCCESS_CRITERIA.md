# Architext 1.4.2 Success Criteria

## Release Intent

Architext 1.4.2 is a patch release for repository-level sync choice reuse.
The goal is to reduce repeated interactive prompts without hiding lifecycle
decisions or making remembered choices irreversible.

This release must not introduce schema changes. Remembered sync choices are
CLI lifecycle metadata, not architecture facts.

## Sync Choice Reuse

Interactive `sync` should make repeated repository maintenance less noisy:

- After a sync records prompt answers, a later interactive sync asks whether to
  reuse the previous answers.
- Reused answers cover branch handling, managed instruction files, `.gitignore`
  management, root package scripts, and doctor repair confirmation where those
  choices are applicable.
- Explicit command-line switches override remembered choices.
- `--prompt` forces normal prompts and does not offer to reuse saved answers.
- `--quiet` chooses defaults without prompting and records the resulting
  choices for later reuse.
- `--yes` keeps its existing meaning of accepting defaults; it may share the
  same non-interactive selection path as `--quiet`.
- Dry runs may report what would happen, but must not update saved choices.

## Persistence

Remembered choices belong in `docs/architext/.architext.json`:

- The metadata records only deterministic CLI sync choices.
- It does not rewrite Architext architecture data.
- It remains repository-local so different repositories can use different
  lifecycle policies.
- A future explicit reset command may delete saved choices, but this release
  only needs `--prompt` to bypass them.

## PDF Self-Data Alignment

Architext 1.4.0 shipped the first PDF export path as browser-native
print/save-as-PDF for the active view. The self-hosted architecture data should
not continue describing PDF export as an undecided planned CLI artifact.

- The PDF node describes the shipped browser-print export surface.
- The PDF flow is marked implemented and names the viewer PDF control, print
  dialog handoff, and print CSS path.
- Future headless or CLI-generated PDF export remains out of scope for this
  patch release.
- PDF fidelity risk stays visible, but is classified as mitigated by the
  current print CSS and UAT coverage rather than as an unstarted feature risk.

## Verification

Before release:

- CLI tests cover first-run prompt recording, later reuse, `--prompt`, and
  `--quiet`.
- Existing sync behavior with explicit switches remains unchanged.
- Architext self-data validates after PDF status correction.
- CI and trusted-publishing release gates install Playwright browsers before
  running UAT.
- `npm test`
- `npm run validate`
