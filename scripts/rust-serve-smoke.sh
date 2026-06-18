#!/usr/bin/env bash
# Rust-serve end-to-end smoke (Phase 2 cutover gate).
#
# Boots `architext serve --foreground` (native Rust CLI) against a TEMP copy of
# docs/architext/data and the Trunk-built Leptos dist, on an OS-assigned port,
# polls for readiness (the serve farm precomputes plans before it listens, so
# fixed sleeps are unreliable), then asserts 200 + JSON on the core endpoints,
# and finally stops ONLY the server it started, by PID.
#
# Run from the repo root:  bash scripts/rust-serve-smoke.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DIST="$ROOT/crates/architext-viewer/dist"
if [ ! -f "$DIST/index.html" ]; then
  echo "Trunk dist missing ($DIST/index.html). Run: trunk build --release --config crates/architext-viewer/Trunk.toml" >&2
  exit 1
fi

# Build the native CLI (debug is fine for a smoke).
cargo build -p architext-cli

# TEMP copy of the data dir so the smoke never mutates the real source of truth.
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ax-rust-serve-smoke.XXXXXX")"
mkdir -p "$TMP/docs/architext"
cp -R "$ROOT/docs/architext/data" "$TMP/docs/architext/data"

SERVE_LOG="$TMP/serve.log"
SERVE_PID=""

cleanup() {
  if [ -n "$SERVE_PID" ] && kill -0 "$SERVE_PID" 2>/dev/null; then
    kill -TERM "$SERVE_PID" 2>/dev/null || true
    for _ in $(seq 1 20); do
      kill -0 "$SERVE_PID" 2>/dev/null || break
      sleep 0.25
    done
    kill -0 "$SERVE_PID" 2>/dev/null && kill -KILL "$SERVE_PID" 2>/dev/null || true
  fi
  rm -rf "$TMP"
}
trap cleanup EXIT

# Serve in the background, OS-assigned port, no browser, our Trunk dist.
ARCHITEXT_VIEWER_DIST="$DIST" \
  ./target/debug/architext serve "$TMP" --foreground --port 0 --no-open \
  > "$SERVE_LOG" 2>&1 &
SERVE_PID=$!

# The foreground server prints `Open http://127.0.0.1:<port>/` once it has bound
# its (OS-assigned) port. Read the URL straight from the log — robust to how the
# temp path normalizes into the serve-state filename key.
URL=""
for _ in $(seq 1 120); do  # up to ~60s
  if ! kill -0 "$SERVE_PID" 2>/dev/null; then
    echo "serve exited before becoming reachable; log:" >&2
    cat "$SERVE_LOG" >&2
    exit 1
  fi
  CANDIDATE="$(sed -n 's#^Open \(http://[^ ]*\)$#\1#p' "$SERVE_LOG" | head -1)"
  if [ -n "$CANDIDATE" ]; then
    CANDIDATE="${CANDIDATE%/}"  # strip trailing slash; appending /api/x to .../ yields //api/x → SPA fallback
    if [ "$(curl -s -o /dev/null -w '%{http_code}' "$CANDIDATE/api/status")" = "200" ]; then
      URL="$CANDIDATE"
      break
    fi
  fi
  sleep 0.5
done

if [ -z "$URL" ]; then
  echo "Rust serve did not become reachable in time; log:" >&2
  cat "$SERVE_LOG" >&2
  exit 1
fi
echo "Rust serve reachable at $URL"

# 64-char lowercase-hex plan hash (valid shape; hit or miss both return 200).
PLAN_HASH="$(printf 'a%.0s' $(seq 1 64))"

fail=0
assert_200() {
  local path="$1"
  local code
  code="$(curl -s -o /dev/null -w '%{http_code}' "$URL$path")"
  if [ "$code" = "200" ]; then
    echo "OK   $path -> 200"
  else
    echo "FAIL $path -> $code" >&2
    fail=1
  fi
}
assert_json() {
  local path="$1"
  local ct
  ct="$(curl -s -o /dev/null -w '%{content_type}' "$URL$path")"
  case "$ct" in
    application/json*) echo "OK   $path content-type $ct" ;;
    *) echo "FAIL $path content-type $ct (expected application/json)" >&2; fail=1 ;;
  esac
}

assert_200 "/data/manifest.json"
assert_json "/data/manifest.json"
assert_200 "/api/status"
assert_json "/api/status"
assert_200 "/api/repo-tree"
assert_json "/api/repo-tree"
assert_200 "/api/plan/$PLAN_HASH"
assert_json "/api/plan/$PLAN_HASH"

if [ "$fail" -ne 0 ]; then
  echo "Rust serve smoke FAILED" >&2
  exit 1
fi
echo "Rust serve smoke PASSED"
