#!/usr/bin/env sh
# Kiseki Terminal v2 standalone installer (macOS / Linux)
# Downloads a tagged GitHub Release archive, verifies SHA-256 against checksums.txt,
# and installs the `kiseki` single binary to ~/.local/bin (or $KISEKI_INSTALL_DIR).
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/nerv-base/kiseki-terminal/v2/scripts/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/nerv-base/kiseki-terminal/v2/scripts/install.sh | KISEKI_VERSION=v2.0.0-alpha.1 sh

set -eu

REPO="${KISEKI_REPO:-nerv-base/kiseki-terminal}"
INSTALL_DIR="${KISEKI_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${KISEKI_VERSION:-}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version|-v)
      VERSION="${2:-}"
      shift 2
      ;;
    --install-dir|-d)
      INSTALL_DIR="${2:-}"
      shift 2
      ;;
    --help|-h)
      cat <<'USAGE'
Kiseki Terminal v2 Installer

Options:
  -v, --version <tag>       Install a specific release tag (e.g. v2.0.0-alpha.1)
  -d, --install-dir <dir>   Installation directory (default: ~/.local/bin)
  -h, --help                Show this help message
USAGE
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Error: required command '$1' is not installed." >&2
    exit 1
  fi
}

need_cmd curl
need_cmd tar

detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Darwin) os_part="apple-darwin" ;;
    Linux)  os_part="unknown-linux-musl" ;;
    *)
      echo "Error: unsupported OS '$os'. On Windows, use scripts/install.ps1, Scoop, or npm/bun." >&2
      exit 1
      ;;
  esac

  case "$arch" in
    arm64|aarch64) arch_part="aarch64" ;;
    x86_64|amd64)  arch_part="x86_64" ;;
    *)
      echo "Error: unsupported CPU architecture '$arch'." >&2
      exit 1
      ;;
  esac

  printf '%s-%s' "$arch_part" "$os_part"
}

resolve_version() {
  if [ -n "$VERSION" ]; then
    case "$VERSION" in
      v*) printf '%s' "$VERSION" ;;
      *)  printf 'v%s' "$VERSION" ;;
    esac
    return
  fi

  # Fetch latest published v2 release tag (never falls back to branch archives)
  api_url="https://api.github.com/repos/${REPO}/releases"
  tag="$(curl -fsSL "$api_url" | grep -E '"tag_name":[[:space:]]*"v2\.' | head -n 1 | sed -E 's/.*"tag_name":[[:space:]]*"([^"]+)".*/\1/')"
  if [ -z "$tag" ]; then
    echo "Error: no published v2 release found for ${REPO}." >&2
    echo "Pass KISEKI_VERSION=v2.0.0-alpha.1 once published, or build from source." >&2
    exit 1
  fi
  printf '%s' "$tag"
}

verify_sha256() {
  file="$1"
  expected="$2"

  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$file" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "$file" | awk '{print $1}')"
  else
    echo "Error: neither 'sha256sum' nor 'shasum' is available to verify download integrity." >&2
    exit 1
  fi

  if [ "$actual" != "$expected" ]; then
    echo "Error: SHA-256 checksum mismatch for $(basename "$file")!" >&2
    echo "  expected: $expected" >&2
    echo "  actual:   $actual" >&2
    exit 1
  fi
}

TARGET="$(detect_target)"
TAG="$(resolve_version)"
ARCHIVE="kiseki-${TAG}-${TARGET}.tar.gz"
BASE_URL="https://github.com/${REPO}/releases/download/${TAG}"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

echo "Installing Kiseki Terminal ${TAG} (${TARGET})..."
curl -fsSL "${BASE_URL}/${ARCHIVE}" -o "${TMP_DIR}/${ARCHIVE}"
curl -fsSL "${BASE_URL}/checksums.txt" -o "${TMP_DIR}/checksums.txt"

EXPECTED_SHA="$(awk -v name="$ARCHIVE" '$2 == name || $2 == "*" name { print $1; exit }' "${TMP_DIR}/checksums.txt")"
if [ -z "$EXPECTED_SHA" ]; then
  echo "Error: checksum entry for '${ARCHIVE}' not found in checksums.txt." >&2
  exit 1
fi

verify_sha256 "${TMP_DIR}/${ARCHIVE}" "$EXPECTED_SHA"
echo "✓ SHA-256 verified"

tar -xzf "${TMP_DIR}/${ARCHIVE}" -C "$TMP_DIR"
EXTRACTED_BIN="${TMP_DIR}/kiseki-${TAG}-${TARGET}/kiseki"
if [ ! -f "$EXTRACTED_BIN" ]; then
  EXTRACTED_BIN="${TMP_DIR}/kiseki"
fi
if [ ! -f "$EXTRACTED_BIN" ]; then
  echo "Error: 'kiseki' binary not found in archive." >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"
chmod +x "$EXTRACTED_BIN"

# Health-check before atomic move into place
"$EXTRACTED_BIN" --version >/dev/null

TMP_DEST="${INSTALL_DIR}/.kiseki.new.$$"
cp "$EXTRACTED_BIN" "$TMP_DEST"
chmod +x "$TMP_DEST"
mv -f "$TMP_DEST" "${INSTALL_DIR}/kiseki"

echo "✓ Installed to ${INSTALL_DIR}/kiseki ($("${INSTALL_DIR}/kiseki" --version))"

case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    echo ""
    echo "Note: ${INSTALL_DIR} is not currently in your PATH. Add it to your shell profile:"
    echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
    ;;
esac

echo ""
echo "Next step: run 'kiseki init' to launch the interactive setup wizard."
