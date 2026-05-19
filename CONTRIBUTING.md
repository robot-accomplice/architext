# Contributing

Architext uses a modified Gitflow process. The goal is a single path from
implementation work to release, with `develop` collecting normal changes and
`main` representing release-ready history.

## Branches

- `main` is the release branch. A pull request into `main` signals a release.
- `develop` is the integration branch for the next release.
- Feature work starts from `develop` on a `feature/<short-name>` branch.
- Bug fixes start from `develop` on a `fix/<short-name>` branch.
- Emergency production fixes may start from `main` on a `hotfix/<short-name>`
  branch.

Keep branch names short, specific, and tied to the change being made.

## Normal Change Flow

1. Update local branches:

   ```sh
   git fetch origin
   git checkout develop
   git pull --ff-only origin develop
   ```

2. Create a feature or fix branch from `develop`:

   ```sh
   git checkout -b feature/<short-name>
   ```

3. Make focused changes. Architecture and documentation updates come before code
   changes when the behavior or system shape changes.

4. Run the local release gate before opening the pull request:

   ```sh
   npm run release:check
   ```

5. Open a pull request into `develop`.

Pull requests into `develop` aggregate for the next release. They should be
small enough to review directly and complete enough that `develop` remains a
valid release-candidate branch after merge.

## Release Flow

When `develop` contains the intended release scope:

1. Verify `develop` is green in CI.
2. Open a pull request from `develop` to `main`.
3. Treat that pull request as the release signal.
4. Complete final release checks, version updates, release notes, and package
   validation before merging.
5. After merge, tag and publish from `main` according to the maintained release
   process.

Do not merge unrelated work directly into a release pull request. If new scope is
needed, merge it through `develop` first so the release branch remains the
single aggregation point.

## Hotfix Flow

Hotfixes are reserved for urgent corrections that cannot wait for the normal
`develop` release train.

1. Create a `hotfix/<short-name>` branch from `main`.
2. Open the hotfix pull request directly into `main`.
3. Run the same release gate:

   ```sh
   npm run release:check
   ```

4. After the hotfix merges to `main`, immediately backmerge `main` into
   `develop` through a pull request:

   ```sh
   git checkout develop
   git pull --ff-only origin develop
   git checkout -b hotfix/backmerge-<short-name>
   git merge --no-ff origin/main
   git push origin hotfix/backmerge-<short-name>
   ```

5. Open the backmerge pull request into `develop`.

The backmerge pull request is required. A hotfix is not complete until `develop`
contains the same correction, because otherwise the next regular release can
accidentally drop the fix.

## Pull Request Expectations

- Explain the user-facing or maintainer-facing reason for the change.
- Keep the diff scoped to the branch purpose.
- Include tests or validation evidence appropriate to the risk.
- Update Architext data and LLM instructions when behavior or project truth
  changes.
- Do not include private operational publication details in public docs.
- Do not use target repositories as formal test fixtures; external projects are
  local litmus tests only.

## Required Checks

At minimum, contributors should run:

```sh
npm run release:check
```

For targeted iteration, the release gate expands to the same core checks used by
CI:

```sh
npm test
npm run test:benchmark
npm run validate
npm run build
npm pack --dry-run
```

Pull requests must pass CI before merge.
