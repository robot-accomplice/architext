# Package Publishing Plan

Architext publishes the public CLI package to npmjs as
`@robotaccomplice/architext`. That package identity is the release contract.
GitHub Packages is not part of the 1.4.4 automation path because its namespace
requirements do not match the existing package and repository ownership.

## Architecture

The release process should have one automated publication lane:

- **npmjs lane:** publish `@robotaccomplice/architext` to
  `https://registry.npmjs.org` using GitHub Actions trusted publishing once the
  npm package owner configuration is fixed.

That lane checks out the released tag, reruns the release gate, publishes with
provenance, verifies the published version, and smoke-tests the packed CLI from
the public npm registry.

## GitHub Packages Constraints

GitHub's npm registry documentation requires scoped npm packages and routes the
scope to the GitHub user or organization namespace. The repository owner is
`robot-accomplice`, while the public npmjs package is
`@robotaccomplice/architext`.

That mismatch is a release blocker for GitHub Packages. Publishing a second
package identity or moving the repository owner are non-starters because they
would either fragment installation or disturb repository ownership. The only
accepted path is to skip GitHub Packages and finish npmjs trusted publishing.

GitHub Packages can link a package to a repository for permissions and
discoverability, but that is not an npm alias. The package users install remains
the `name` field published to that registry. npm dependency aliases can rename a
dependency inside one consuming project, but they do not make GitHub Packages
serve `@robotaccomplice/architext` from the `robot-accomplice` owner namespace.

## Workflow Shape

The existing publish workflow should remain focused on npmjs and should:

1. Run automatically when a GitHub Release is published.
2. Keep a manual dry-run and recovery dispatch path.
3. Derive the package version from the release tag for release-triggered runs.
4. Checks out tag `vX.Y.Z`.
5. Runs `npm ci` and `npm ci --prefix docs/architext`.
6. Runs `npm run release:check`.
7. Configures `actions/setup-node` with
   `registry-url: https://registry.npmjs.org`.
8. Publishes with npm trusted publishing and provenance.
9. Verifies the version with `npm view`.
10. Smoke-tests a fresh global install from npm.

Local `npm publish --auth-type=web` remains a fallback only until trusted
publishing is fixed.

## Acceptance Criteria

- npmjs release automation is the only automated publish path.
- The public npmjs package remains `@robotaccomplice/architext`.
- A failed npmjs publish fails loudly and leaves the release unpublished.
- Documentation names GitHub Packages as out of scope while the namespace
  mismatch exists.
- Creating a published GitHub Release is enough to start the npmjs publish
  workflow once npm trusted publishing is configured.
