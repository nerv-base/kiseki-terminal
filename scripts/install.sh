#!/usr/bin/env sh
# Kiseki Terminal v2 standalone installer (macOS / Linux)
# Stage 1: Downloads, SHA-256-verifies and installs the `kiseki` binary.
# Stage 2: Hands off parallel tool & font installation with live progress bars
#          to `kiseki deps install`.
#
# Usage:
#   curl -fsSL https://kisekiterm.dev/install.sh | sh
#   curl -fsSL https://kisekiterm.dev/install.sh | KISEKI_MIRROR=cn sh
#   curl -fsSL https://kisekiterm.dev/install.sh | KISEKI_VERSION=v2.0.0-beta.1 sh

set -eu

REPO="${KISEKI_REPO:-nerv-base/kiseki-terminal}"
CN_ORIGIN="https://kiseki-dl.nerv-base.com"
GLOBAL_ORIGIN="https://dl.kisekiterm.dev"
EXPLICIT_DL_BASE="${KISEKI_DL_BASE:-}"
MIRROR_PREF="${KISEKI_MIRROR:-}"
INSTALL_DIR="${KISEKI_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${KISEKI_VERSION:-}"
CONCURRENCY="${KISEKI_CONCURRENCY:-}"
SKIP_FONTS=0

normalize_origin() {
  val="${1%/}"
  case "$val" in
    */releases) val="${val%/releases}" ;;
  esac
  printf '%s' "$val"
}

is_loopback_origin() {
  rest="${1#http://}"
  rest="${rest#https://}"
  case "$rest" in
    127.0.0.1*|localhost*|\[::1\]*) return 0 ;;
    *) return 1 ;;
  esac
}

build_mirror_origins() {
  ordered=""
  explicit_all_loopback=1
  has_explicit=0

  add_origin() {
    cand="$(normalize_origin "$1")"
    [ -n "$cand" ] || return 0
    case " $ordered " in
      *" $cand "*) return 0 ;;
    esac
    ordered="${ordered:+$ordered }$cand"
  }

  if [ -n "$EXPLICIT_DL_BASE" ]; then
    old_ifs="$IFS"
    IFS=','
    for part in $EXPLICIT_DL_BASE; do
      cand="$(normalize_origin "$part")"
      [ -n "$cand" ] || continue
      has_explicit=1
      add_origin "$cand"
      if ! is_loopback_origin "$cand"; then
        explicit_all_loopback=0
      fi
    done
    IFS="$old_ifs"
    if [ "$has_explicit" = 1 ] && [ "$explicit_all_loopback" = 1 ]; then
      printf '%s' "$ordered"
      return 0
    fi
  fi

  if [ "$MIRROR_PREF" = "cn" ]; then
    add_origin "$CN_ORIGIN"
    add_origin "$GLOBAL_ORIGIN"
  elif [ "$MIRROR_PREF" = "global" ]; then
    add_origin "$GLOBAL_ORIGIN"
    add_origin "$CN_ORIGIN"
  else
    tz_offset="$(date +%z 2>/dev/null || true)"
    case "$tz_offset" in
      +0800)
        add_origin "$CN_ORIGIN"
        add_origin "$GLOBAL_ORIGIN"
        ;;
      *)
        add_origin "$GLOBAL_ORIGIN"
        add_origin "$CN_ORIGIN"
        ;;
    esac
  fi
  printf '%s' "$ordered"
}

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
    --mirror)
      MIRROR_PREF="${2:-}"
      shift 2
      ;;
    --concurrency|-j)
      CONCURRENCY="${2:-}"
      shift 2
      ;;
    --skip-fonts)
      SKIP_FONTS=1
      shift
      ;;
    --help|-h)
      cat <<'USAGE'
Kiseki Terminal v2 Installer

Options:
  -v, --version <tag>       Install a specific release tag (e.g. v2.0.0-beta.1)
  -d, --install-dir <dir>   Installation directory (default: ~/.local/bin)
      --mirror <cn|global>  Preferred download mirror region
  -j, --concurrency <N>     Parallel dependency download workers (1-8, default: 4)
      --skip-fonts          Skip all four font families
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
      echo "Error: unsupported OS '$os'. On Windows, use scripts/install.ps1." >&2
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

all_origins_loopback() {
  for origin in $MIRROR_ORIGINS; do
    if ! is_loopback_origin "$origin"; then
      return 1
    fi
  done
  [ -n "$MIRROR_ORIGINS" ]
}

resolve_version() {
  if [ -n "$VERSION" ]; then
    case "$VERSION" in
      *[!0-9A-Za-z.+v-]*) echo "Error: invalid release tag '$VERSION'." >&2; return 1 ;;
      v*) printf '%s' "$VERSION" ;;
      *)  printf 'v%s' "$VERSION" ;;
    esac
    return
  fi

  # Try mirrors in priority order, then fall back to the GitHub API.
  for origin in $MIRROR_ORIGINS; do
    tag="$(curl -fsSL --connect-timeout 5 --max-time 8 "${origin}/releases/latest.json" 2>/dev/null | sed -nE 's/.*"tag":[[:space:]]*"([^"]+)".*/\1/p' | head -n 1 || true)"
    case "$tag" in
      *[!0-9A-Za-z.+-]*|'') ;;
      v2.*) printf '%s' "$tag"; return ;;
    esac
  done

  if all_origins_loopback; then
    echo "Error: could not resolve release tag from configured local mirror." >&2
    return 1
  fi

  # Fetch latest published v2 release tag (never falls back to branch archives)
  api_url="https://api.github.com/repos/${REPO}/releases"
  tag="$(curl -fsSL --connect-timeout 10 --max-time 30 "$api_url" | grep -E '"tag_name":[[:space:]]*"v2\.' | head -n 1 | sed -E 's/.*"tag_name":[[:space:]]*"([^"]+)".*/\1/')"
  if [ -z "$tag" ]; then
    echo "Error: no published v2 release found for ${REPO}." >&2
    echo "Pass KISEKI_VERSION=v2.0.0-beta.1 once published, or build from source." >&2
    exit 1
  fi
  printf '%s' "$tag"
}

download_meta() (
  url="$1" dest="$2"
  asset_name="${url##*/}"
  for origin in $MIRROR_ORIGINS; do
    if curl -fsSL --connect-timeout 5 --max-time 15 "${origin}/releases/${TAG}/${asset_name}" -o "$dest" 2>/dev/null; then
      return 0
    fi
    rm -f "$dest"
  done
  if all_origins_loopback; then
    return 1
  fi
  curl -fsSL --connect-timeout 10 --max-time 30 "$url" -o "$dest"
)

# Uses stall detection (--speed-limit/--speed-time) rather than a fixed total
# timeout, and renders a live progress bar when stderr is an interactive TTY
# (which stays true even when piped via `curl ... | sh`).
download_with_progress() (
  url="$1" dest="$2"
  asset_name="${url##*/}"
  use_bar=0
  if [ -t 2 ] && [ "${TERM:-}" != "dumb" ]; then
    use_bar=1
  fi

  for origin in $MIRROR_ORIGINS; do
    src="${origin}/releases/${TAG}/${asset_name}"
    if [ "$use_bar" = 1 ]; then
      if curl -fSL --progress-bar --connect-timeout 8 --speed-limit 1024 --speed-time 30 "$src" -o "$dest"; then
        return 0
      fi
    else
      if curl -fsSL --connect-timeout 8 --speed-limit 1024 --speed-time 30 "$src" -o "$dest" 2>/dev/null; then
        return 0
      fi
    fi
    rm -f "$dest"
  done

  if all_origins_loopback; then
    return 1
  fi

  if [ "$use_bar" = 1 ]; then
    curl -fSL --progress-bar --connect-timeout 10 --speed-limit 1024 --speed-time 45 "$url" -o "$dest"
  else
    curl -fsSL --connect-timeout 10 --speed-limit 1024 --speed-time 45 "$url" -o "$dest"
  fi
)

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    echo "Error: neither 'sha256sum' nor 'shasum' is available to verify download integrity." >&2
    return 1
  fi
}

verify_sha256() (
  actual="$(sha256 "$1")" || return 1
  if [ -z "$2" ] || [ "$actual" != "$2" ]; then
    echo "Error: SHA-256 checksum mismatch for $(basename "$1")!" >&2
    echo "  expected: $2" >&2
    echo "  actual:   $actual" >&2
    return 1
  fi
)

release_checksum() {
  awk -v name="$1" '$2 == name || $2 == "*" name { print $1; exit }' "$TMP_DIR/checksums.txt"
}

# Work in the destination directory so replacement is atomic and can be retried.
install_file() (
  src="$1" dest="$2" mode="$3"
  if cmp -s "$src" "$dest" && { [ "$mode" != 0755 ] || [ -x "$dest" ]; }; then
    return 0
  fi
  mkdir -p "$(dirname "$dest")" || return 1
  staged="$(mktemp "${dest}.tmp.XXXXXX")" || return 1
  trap 'rm -f "$staged"' 0
  cp "$src" "$staged" && chmod "$mode" "$staged" && mv -f "$staged" "$dest"
)

install_core() {
  archive="kiseki-${TAG}-${TARGET}.tar.gz"
  # A network failure has a distinct status (2) so an offline rerun can reuse a
  # previously verified installation of this exact tag.
  download_meta "${BASE_URL}/checksums.txt" "$TMP_DIR/checksums.txt" || return 2
  printf '  ↓ 下载主程序 %s...\n' "$archive" >&2
  download_with_progress "${BASE_URL}/${archive}" "$TMP_DIR/$archive" || return 2
  verify_sha256 "$TMP_DIR/$archive" "$(release_checksum "$archive")" || return 1
  tar -xzf "$TMP_DIR/$archive" -C "$TMP_DIR" || return 1
  extracted_bin="$TMP_DIR/kiseki-${TAG}-${TARGET}/kiseki"
  if [ ! -f "$extracted_bin" ]; then
    extracted_bin="$TMP_DIR/kiseki"
  fi
  chmod +x "$extracted_bin" && "$extracted_bin" --version >/dev/null || return 1
  core_hash="$(sha256 "$extracted_bin")" || return 1
  install_file "$extracted_bin" "$INSTALL_DIR/kiseki" 0755 || return 1
  printf '%s\t%s\n' "$TAG" "$core_hash" > "$TMP_DIR/core.receipt" || return 1
  install_file "$TMP_DIR/core.receipt" "$INSTALL_DIR/.kiseki-release" 0644
}

use_installed_core() {
  [ -r "$INSTALL_DIR/.kiseki-release" ] || return 1
  IFS="$TAB" read -r installed_tag installed_hash < "$INSTALL_DIR/.kiseki-release" || return 1
  [ -n "$installed_tag" ] && { [ -z "$1" ] || [ "$1" = "$installed_tag" ]; } || return 1
  verify_sha256 "$INSTALL_DIR/kiseki" "$installed_hash" && "$INSTALL_DIR/kiseki" --version >/dev/null || return 1
  TAG="$installed_tag"
  echo "✓ 已保留安装验证过的 Kiseki ${TAG}（无法连接发布服务器）"
}

run_stage2_deps() {
  set -- deps install --version "$TAG" --repo "$REPO"
  if [ -r "$TMP_DIR/checksums.txt" ]; then
    set -- "$@" --checksums "$TMP_DIR/checksums.txt"
  fi
  if [ -n "$MIRROR_PREF" ]; then
    set -- "$@" --mirror "$MIRROR_PREF"
  fi
  if [ -n "$EXPLICIT_DL_BASE" ]; then
    set -- "$@" --dl-base "$EXPLICIT_DL_BASE"
  fi
  if [ "$SKIP_FONTS" = 1 ]; then
    set -- "$@" --skip-fonts
  fi
  if [ -n "$CONCURRENCY" ]; then
    set -- "$@" --concurrency "$CONCURRENCY"
  fi
  if ! "${INSTALL_DIR}/kiseki" "$@"; then
    echo "✗ 部分依赖安装失败，可运行 'kiseki deps install' 重试（见 https://kisekiterm.dev）。" >&2
  fi
}

TARGET="$(detect_target)"
TAB="$(printf '\t')"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' 0
trap 'exit 1' HUP INT TERM
MIRROR_ORIGINS="$(build_mirror_origins)"
PRIMARY_MIRROR="${MIRROR_ORIGINS%% *}"
printf '· Mirror: %s (fallback: GitHub)\n' "${PRIMARY_MIRROR#https://}"

if TAG="$(resolve_version)"; then
  echo "Installing Kiseki Terminal ${TAG} (${TARGET})..."
  BASE_URL="https://github.com/${REPO}/releases/download/${TAG}"
  if install_core; then
    echo "✓ Installed to ${INSTALL_DIR}/kiseki ($("${INSTALL_DIR}/kiseki" --version))"
  else
    result="$?"
    if [ "$result" != 2 ] || ! use_installed_core "$TAG"; then exit 1; fi
  fi
  run_stage2_deps
elif use_installed_core "$VERSION"; then
  echo "✗ 无法连接发布服务器以检查依赖，请重跑安装脚本（见 https://kisekiterm.dev）。" >&2
else
  exit 1
fi

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
"${INSTALL_DIR}/kiseki" deps || echo "✗ 无法读取能力清单，可运行 kiseki deps 重试。" >&2
if [ "$SKIP_FONTS" = 1 ]; then
  echo '· 本次未安装字体（已按 --skip-fonts 跳过）。'
fi
