#!/bin/sh
# Architext native installer — no npm required.
#
#   curl -fsSL https://raw.githubusercontent.com/robot-accomplice/architext/main/install.sh | sh
#
# Downloads the latest release binary for your platform, verifies its SHA-256
# checksum, and installs it to ~/.local/bin (override with ARCHITEXT_INSTALL_DIR).
# Once installed, keep it current with `architext update`.
set -eu

REPO="robot-accomplice/architext"
BIN_NAME="architext"
INSTALL_DIR="${ARCHITEXT_INSTALL_DIR:-$HOME/.local/bin}"

die() { echo "architext install: $1" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"; }
need curl

# --- detect platform → release asset key (must match the published asset names)
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
  Linux) plat="linux" ;;
  Darwin) plat="darwin" ;;
  *) die "unsupported OS '$os'. On Windows, download architext-win32-x64.exe from https://github.com/$REPO/releases/latest" ;;
esac
case "$arch" in
  x86_64 | amd64) cpu="x64" ;;
  arm64 | aarch64) cpu="arm64" ;;
  *) die "unsupported architecture '$arch'" ;;
esac
key="${plat}-${cpu}"
asset="architext-${key}"

# --- resolve the latest release tag (no jq dependency)
tag="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep '"tag_name"' | head -1 | sed -E 's/.*"tag_name"[ ]*:[ ]*"([^"]+)".*/\1/')"
[ -n "$tag" ] || die "could not resolve the latest release tag (GitHub API rate limit?)"

base="https://github.com/${REPO}/releases/download/${tag}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Installing Architext ${tag} (${key})..."
curl -fSL --proto '=https' --tlsv1.2 "${base}/${asset}" -o "${tmp}/${BIN_NAME}" \
  || die "download failed: ${base}/${asset}"

# --- verify checksum if the release ships SHA256SUMS (it should)
if curl -fsSL "${base}/SHA256SUMS" -o "${tmp}/SHA256SUMS" 2>/dev/null; then
  want="$(grep "  ${asset}\$" "${tmp}/SHA256SUMS" | awk '{print $1}' | head -1)"
  if [ -n "$want" ]; then
    if command -v sha256sum >/dev/null 2>&1; then
      got="$(sha256sum "${tmp}/${BIN_NAME}" | awk '{print $1}')"
    else
      got="$(shasum -a 256 "${tmp}/${BIN_NAME}" | awk '{print $1}')"
    fi
    [ "$want" = "$got" ] || die "checksum mismatch for ${asset} (expected ${want}, got ${got})"
    echo "Checksum verified."
  fi
else
  echo "warning: no SHA256SUMS on this release; skipping checksum verification" >&2
fi

chmod +x "${tmp}/${BIN_NAME}"
mkdir -p "$INSTALL_DIR"
mv "${tmp}/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"
echo "Installed ${BIN_NAME} ${tag} to ${INSTALL_DIR}/${BIN_NAME}"

case ":$PATH:" in
  *":$INSTALL_DIR:"*)
    echo "Run: architext --version" ;;
  *)
    echo
    echo "Add ${INSTALL_DIR} to your PATH:"
    echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
    echo "Then run: architext --version" ;;
esac
