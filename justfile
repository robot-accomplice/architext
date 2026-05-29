set shell := ["bash", "-uc"]

package := "@robotaccomplice/architext"

default:
    @just --list

# Run the local release ceremony gate used before tagging or publishing.
release-check:
    just release-doc-check
    npm run release:check

# Refresh README-facing screenshots and grep for common stale public-doc markers.
release-doc-check:
    node scripts/capture-readme-screenshots.mjs
    rg -n "semver-|currentVersion:|Release Truth|Rules|1\\.[0-9]+\\.[0-9]+" README.md viewer/README.md docs/architecture src || true

# Show the most recent CI runs for the repository.
ci:
    gh run list --limit 10 --json databaseId,headSha,status,conclusion,workflowName,displayTitle,url,createdAt,event

# Show CI status for a specific commit SHA.
ci-for commit:
    gh run list --commit {{commit}} --limit 5 --json databaseId,headSha,status,conclusion,workflowName,displayTitle,url,createdAt,event

# Create a GitHub release for the current package version after CI passes.
github-release:
    version="$(node -p 'JSON.parse(require("fs").readFileSync("package.json", "utf8")).version')"; \
    gh release create "v${version}" --target main --title "Architext ${version}" --notes "Architext ${version} release."

# Start a fresh npm web/passkey login session. The maintainer completes auth in the browser.
npm-passkey-login:
    npm logout || true
    npm login --auth-type=web

# Publish the current package after CI has passed and npm passkey login has completed.
npm-publish:
    npm publish --access public --auth-type=web

# Start a GitHub Actions trusted-publishing run for a released version.
trusted-publish version:
    gh workflow run publish.yml -f version={{version}} -f dry_run=false

# Start a GitHub Actions trusted-publishing dry run for a released version.
trusted-publish-dry-run version:
    gh workflow run publish.yml -f version={{version}} -f dry_run=true

# Verify that the current package version is publicly visible on npm.
npm-verify:
    version="$(node -p 'JSON.parse(require("fs").readFileSync("package.json", "utf8")).version')"; \
    npm view "{{package}}@${version}" version

# Install the current package version into a temporary prefix and smoke-test the CLI.
npm-smoke:
    version="$(node -p 'JSON.parse(require("fs").readFileSync("package.json", "utf8")).version')"; \
    prefix="$(mktemp -d)"; \
    target="$(mktemp -d)"; \
    npm install -g --prefix "$prefix" "{{package}}@${version}"; \
    "$prefix/bin/architext" --version; \
    "$prefix/bin/architext" sync "$target" --yes --branch none; \
    "$prefix/bin/architext" validate "$target"
