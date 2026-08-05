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

# Run the FULL verify-rust CI job locally, same steps in the same order as
# .github/workflows/ci.yml — the pre-merge gate without the push-and-wait.
# Keep this recipe in lockstep with ci.yml when the workflow changes.
# ~5-10 min warm cache. Fails fast on the first red step.
ci-local:
    @echo "[1/7] cargo test --workspace"
    cargo test --workspace
    @echo "[2/7] routing corpus fitness + perf ratchet"
    cargo test -p architext-routing --test corpus_fitness
    @echo "[3/7] clippy wasm viewer (-D warnings)"
    cargo clippy -p architext-viewer --target wasm32-unknown-unknown -- -D warnings
    @echo "[4/7] trunk build (release viewer)"
    trunk build --release --config crates/architext-viewer/Trunk.toml
    @echo "[5/7] architext validate ."
    cargo run --quiet -p architext-cli -- validate .
    @echo "[6/7] rust serve smoke"
    bash scripts/rust-serve-smoke.sh
    @echo "[7/7] bare-binary embedded serve smoke"
    bash scripts/rust-serve-embedded-smoke.sh
    @echo "ci-local GREEN — matches verify-rust"

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
