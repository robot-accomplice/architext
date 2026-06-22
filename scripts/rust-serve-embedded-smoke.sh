#!/usr/bin/env bash
# Bare-binary serve smoke — the EMBEDDED viewer path (off-npm / off-disk).
#
# The regular rust-serve-smoke.sh serves with an on-disk Trunk dist
# (ARCHITEXT_VIEWER_DIST=<dist>), so it never exercises the viewer that is
# EMBEDDED into the native binary (rust-embed, 1.7.4+). That embedded path is what
# a user gets from the npm per-platform binary (no co-located dist) and from
# running the bare binary directly — and it shipped broken once precisely because
# nothing in CI ran it. This smoke forces it: serve with an EMPTY dist dir so the
# only place the viewer can come from is the embedded copy, then assert the root
# actually returns the viewer SPA (title + wasm bootstrap).
#
# Run from the repo root:  bash scripts/rust-serve-embedded-smoke.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# The viewer is baked into the binary at compile time from this dist, so it must
# exist BEFORE the build for the embed to be real (CI runs `trunk build` first).
DIST="$ROOT/crates/architext-viewer/dist"
if [ ! -f "$DIST/index.html" ]; then
  echo "Trunk dist missing ($DIST/index.html). Run: trunk build --release --config crates/architext-viewer/Trunk.toml" >&2
  exit 1
fi

# Release build: the embedded assets are baked in here; debug also works but the
# farm precompute is slower on shared runners.
cargo build --release -p architext-cli

TMP="$(mktemp -d "${TMPDIR:-/tmp}/ax-embedded-smoke.XXXXXX")"
mkdir -p "$TMP/docs/architext"
cp -R "$ROOT/docs/architext/data" "$TMP/docs/architext/data"
# An EMPTY dist dir: on-disk resolution finds nothing, so serve must fall back to
# the embedded viewer. This is the whole point of the smoke.
EMPTY_DIST="$TMP/empty-dist"
mkdir -p "$EMPTY_DIST"

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

PORT=8797
URL="http://127.0.0.1:$PORT"
# Empty dist → forces the embedded viewer. No ARCHITEXT_VIEWER_DIST pointing at a
# real dist, no co-located dist.
ARCHITEXT_VIEWER_DIST="$EMPTY_DIST" \
  ./target/release/architext serve "$TMP" --foreground --port "$PORT" --no-open \
  > "$SERVE_LOG" 2>&1 &
SERVE_PID=$!

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
  echo "embedded serve did not become reachable; log:" >&2
  cat "$SERVE_LOG" >&2
  exit 1
fi
echo "embedded serve reachable at $URL"

fail=0
# Root must return the EMBEDDED viewer SPA (with an empty dist, it can come from
# nowhere else): HTML, the Architext title, and the wasm bootstrap reference.
ROOT_BODY="$(curl -s "$URL/")"
ROOT_CT="$(curl -s -o /dev/null -w '%{content_type}' "$URL/")"
case "$ROOT_CT" in
  text/html*) echo "OK   / content-type $ROOT_CT" ;;
  *) echo "FAIL / content-type $ROOT_CT (expected text/html)" >&2; fail=1 ;;
esac
if printf '%s' "$ROOT_BODY" | grep -q "<title>Architext</title>"; then
  echo "OK   / serves the embedded viewer (title present)"
else
  echo "FAIL / did not serve the embedded viewer (no Architext title)" >&2
  fail=1
fi
if printf '%s' "$ROOT_BODY" | grep -q "\.wasm"; then
  echo "OK   / references the embedded wasm bootstrap"
else
  echo "FAIL / missing wasm bootstrap reference" >&2
  fail=1
fi
# And the API still works off the bare binary.
if [ "$(curl -s -o /dev/null -w '%{http_code}' "$URL/api/status")" = "200" ]; then
  echo "OK   /api/status -> 200"
else
  echo "FAIL /api/status not 200" >&2
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  echo "Embedded serve smoke FAILED" >&2
  exit 1
fi
echo "Embedded serve smoke PASSED"
