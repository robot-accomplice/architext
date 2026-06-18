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

# Build the native CLI in RELEASE: the serve farm precomputes every flow plan
# before it binds, and a debug farm can exceed the readiness window on a slow
# shared CI runner (observed as "did not become reachable").
cargo build --release -p architext-cli

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

# Serve in the background on a FIXED high port, no browser, our Trunk dist.
# We poll the port directly (below) rather than parsing the server's stdout:
# when stdout is redirected to a file (non-TTY, as in CI) it is block-buffered,
# so the announce line may not reach the log until well after the server is
# actually listening. A clean CI runner has this port free; the server also
# searches the next 50 ports if busy, but on CI it binds the one we ask for.
PORT=8799
URL="http://127.0.0.1:$PORT"
ARCHITEXT_VIEWER_DIST="$DIST" \
  ./target/release/architext serve "$TMP" --foreground --port "$PORT" --no-open \
  > "$SERVE_LOG" 2>&1 &
SERVE_PID=$!

# Poll the port itself for readiness — independent of stdout buffering. Generous
# window for the farm precompute on a cold CI runner.
reachable=0
for _ in $(seq 1 240); do  # up to ~120s
  if ! kill -0 "$SERVE_PID" 2>/dev/null; then
    echo "serve exited before becoming reachable; log:" >&2
    cat "$SERVE_LOG" >&2
    exit 1
  fi
  if [ "$(curl -s -o /dev/null -w '%{http_code}' "$URL/api/status")" = "200" ]; then
    reachable=1
    break
  fi
  sleep 0.5
done

if [ "$reachable" -ne 1 ]; then
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
