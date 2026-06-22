set shell := ["bash", "-uc"]

repo := "robot-accomplice/architext"

default:
    @just --list

# Run the local release gate used before tagging. Native-only: validate the
# Architext data and build the WASM viewer (which is embedded into the binary).
release-check:
    just release-doc-check
    cargo run --quiet -p architext-cli -- validate .
    trunk build --release --config crates/architext-viewer/Trunk.toml

# Grep for common stale public-doc markers. README screenshots are regenerated
# manually from architext's own served data (see README captions).
release-doc-check:
    rg -n "semver-|currentVersion:|Release Truth|Rules|1\\.[0-9]+\\.[0-9]+" README.md viewer/README.md docs/architecture src || true

# Show the most recent CI runs for the repository.
ci:
    gh run list --limit 10 --json databaseId,headSha,status,conclusion,workflowName,displayTitle,url,createdAt,event

# Show CI status for a specific commit SHA.
ci-for commit:
    gh run list --commit {{commit}} --limit 5 --json databaseId,headSha,status,conclusion,workflowName,displayTitle,url,createdAt,event

# Create a GitHub release for the current VERSION after CI passes. Publishing the
# release fires the Release workflow, which builds the native binaries and
# attaches them + SHA256SUMS to the release (the install.sh / `architext update`
# source). There is no npm step.
github-release:
    version="$(tr -d '[:space:]' < VERSION)"; \
    gh release create "v${version}" --repo {{repo}} --target main --title "Architext ${version}" --notes "Architext ${version} release."

# Dispatch the Release workflow in dry-run mode (build binaries + viewer, attach
# nothing) to validate the release build before tagging.
release-dry-run version:
    gh workflow run publish.yml --repo {{repo}} -f version={{version}} -f dry_run=true
