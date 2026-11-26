#!/usr/bin/env sh
# Kiseki Terminal v2 standalone installer (macOS / Linux)
# Installs Kiseki, its optional tools and fonts from a tagged release.
#
# Usage:
#   curl -fsSL https://kisekiterm.dev/install.sh | sh
#   curl -fsSL https://kisekiterm.dev/install.sh | KISEKI_VERSION=v2.0.0-alpha.1 sh

set -eu

REPO="${KISEKI_REPO:-nerv-base/kiseki-terminal}"
DL_BASE="${KISEKI_DL_BASE:-https://dl.kisekiterm.dev/releases}"
INSTALL_DIR="${KISEKI_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${KISEKI_VERSION:-}"
SKIP_FONTS=0

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
    --skip-fonts)
      SKIP_FONTS=1
      shift
      ;;
    --help|-h)
      cat <<'USAGE'
Kiseki Terminal v2 Installer

Options:
  -v, --version <tag>       Install a specific release tag (e.g. v2.0.0-alpha.1)
  -d, --install-dir <dir>   Installation directory (default: ~/.local/bin)
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

resolve_version() {
  if [ -n "$VERSION" ]; then
    case "$VERSION" in
      *[!0-9A-Za-z.+v-]*) echo "Error: invalid release tag '$VERSION'." >&2; return 1 ;;
      v*) printf '%s' "$VERSION" ;;
      *)  printf 'v%s' "$VERSION" ;;
    esac
    return
  fi

  # Try the release mirror first, then fall back to the GitHub API.
  tag="$(curl -fsSL --connect-timeout 5 --max-time 5 "${DL_BASE%/}/latest.json" 2>/dev/null | sed -nE 's/.*"tag":[[:space:]]*"([^"]+)".*/\1/p' | head -n 1)"
  case "$tag" in
    *[!0-9A-Za-z.+-]*) ;;
    v2.*) printf '%s' "$tag"; return ;;
  esac

  # Fetch latest published v2 release tag (never falls back to branch archives)
  api_url="https://api.github.com/repos/${REPO}/releases"
  tag="$(curl -fsSL --connect-timeout 10 --max-time 30 "$api_url" | grep -E '"tag_name":[[:space:]]*"v2\.' | head -n 1 | sed -E 's/.*"tag_name":[[:space:]]*"([^"]+)".*/\1/')"
  if [ -z "$tag" ]; then
    echo "Error: no published v2 release found for ${REPO}." >&2
    echo "Pass KISEKI_VERSION=v2.0.0-alpha.1 once published, or build from source." >&2
    exit 1
  fi
  printf '%s' "$tag"
}

download() (
  url="$1" dest="$2"
  if curl -fsSL --connect-timeout 5 --max-time 60 "${DL_BASE%/}/${TAG}/${url##*/}" -o "$dest" 2>/dev/null; then
    return 0
  fi
  curl -fsSL --connect-timeout 10 --max-time 300 "$url" -o "$dest"
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
  # A network failure has a distinct status so an offline rerun can reuse a
  # previously verified installation of this exact tag.
  download "${BASE_URL}/checksums.txt" "$TMP_DIR/checksums.txt" || return 2
  download "${BASE_URL}/${archive}" "$TMP_DIR/$archive" || return 2
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

dependency_platform() (
  case "$TARGET" in
    aarch64-apple-darwin) printf '%s' darwin-arm64 ;;
    x86_64-apple-darwin) printf '%s' darwin-amd64 ;;
    aarch64-unknown-linux-musl)
      if ! getconf GNU_LIBC_VERSION >/dev/null 2>&1 && [ -f /lib/ld-musl-aarch64.so.1 ]; then printf '%s' linux-musl-arm64; else printf '%s' linux-arm64; fi ;;
    x86_64-unknown-linux-musl)
      if ! getconf GNU_LIBC_VERSION >/dev/null 2>&1 && [ -f /lib/ld-musl-x86_64.so.1 ]; then printf '%s' linux-musl-amd64; else printf '%s' linux-amd64; fi ;;
  esac
)

require_unzip() {
  if ! command -v unzip >/dev/null 2>&1; then
    printf '%s\n' '✗ 解压 ZIP 资产需要 unzip。请安装 unzip（例如 sudo apt install unzip），再重跑安装脚本。' >&2
    return 1
  fi
}

install_dependency() (
  kind="$1" url="$2" digest="$3" relative="$4" mode="$5"
  asset_dir="$(mktemp -d "$TMP_DIR/dependency.XXXXXX")" || return 1
  trap 'rm -rf "$asset_dir"' 0
  asset="$asset_dir/asset"
  extracted="$asset_dir/extracted"
  case "$kind:$relative:$mode" in
    tool:vendor/bin/oh-my-posh:0755|tool:vendor/bin/fastfetch:0755)
      dest="$KISEKI_CONFIG_ROOT/$relative"
      if [ -x "$dest" ] && [ "$(sha256 "$dest")" = "$digest" ]; then return 0; fi
      if [ "$relative" = vendor/bin/fastfetch ]; then
        require_unzip || return 1
      fi
      download "$url" "$asset" && verify_sha256 "$asset" "$digest" || return 1
      if [ "$relative" = vendor/bin/fastfetch ]; then
        unzip -p "$asset" '*/usr/bin/fastfetch' > "$extracted" || return 1
      else
        cp "$asset" "$extracted" || return 1
      fi
      chmod +x "$extracted" && "$extracted" --version >/dev/null || return 1
      install_file "$extracted" "$dest" "$mode"
      ;;
    font-raw:fonts/*.ttf:0644)
      name="${relative#fonts/}"
      case "$name" in *[!A-Za-z0-9_.-]*|.*) return 1 ;; esac
      [ -n "$FONT_DIR" ] || return 1
      dest="$FONT_DIR/$name"
      if [ -f "$dest" ] && [ "$(sha256 "$dest")" = "$digest" ]; then return 0; fi
      download "$url" "$asset" && verify_sha256 "$asset" "$digest" || return 1
      install_file "$asset" "$dest" "$mode"
      ;;
    font:fonts/*:0644)
      family="${relative#fonts/}"
      pattern="$(awk -F '\t' -v id="$family" '$1 == "# font" && $2 == id { print $3; exit }' "$manifest")"
      [ -n "$FONT_DIR" ] && [ -n "$pattern" ] || return 1
      require_unzip || return 1
      download "$url" "$asset" && verify_sha256 "$asset" "$digest" || return 1
      unzip -Z1 "$asset" > "$asset_dir/font.entries" || return 1
      found=0
      while IFS= read -r entry; do
        name="${entry##*/}"
        case "$entry" in *'*'*|*'?'*|*'['*) continue ;; esac
        printf '%s\n' "$name" | grep -Eq -e "$pattern" || continue
        unzip -p "$asset" "$entry" > "$extracted" || return 1
        [ -s "$extracted" ] && install_file "$extracted" "$FONT_DIR/$name" "$mode" || return 1
        found=1
      done < "$asset_dir/font.entries"
      [ "$found" = 1 ]
      ;;
    *) return 1 ;;
  esac
)

install_dependencies() {
  manifest="$TMP_DIR/deps.tsv"
  if [ ! -r "$TMP_DIR/checksums.txt" ] ||
    ! download "${BASE_URL}/deps.tsv" "$manifest" ||
    ! verify_sha256 "$manifest" "$(release_checksum deps.tsv)"; then
    echo "✗ 依赖清单不可用，请重跑安装脚本（见 https://kisekiterm.dev）。" >&2
    return 0
  fi
  dep_platform="$(dependency_platform)"
  case "$dep_platform" in darwin-*) font_os=macos ;; *) font_os=linux ;; esac
  font_path="$(awk -F '\t' -v os="$font_os" '$1 == "# font-dir" && $2 == os { print $3; exit }' "$manifest")"
  case "$font_path" in
    \~/*) FONT_DIR="$HOME/${font_path#\~/}" ;;
    *) FONT_DIR= ;;
  esac
  awk -F '\t' -v target="$dep_platform" '$1 !~ /^#/ && $2 == target' "$manifest" > "$TMP_DIR/deps-target.tsv" || return 0
  fonts_ready=0
  while IFS="$TAB" read -r kind _platform url digest relative mode; do
    case "$kind" in
      font|font-raw) [ "$SKIP_FONTS" = 0 ] || continue ;;
    esac
    if install_dependency "$kind" "$url" "$digest" "$relative" "$mode"; then
      case "$kind" in
        font|font-raw)
          printf '✓ %s 字体已就绪\n' "${relative#fonts/}"
          fonts_ready=1
          ;;
      esac
    else
      printf '✗ %s 安装失败，请重跑安装脚本（见 https://kisekiterm.dev）。\n' "$relative" >&2
    fi
  done < "$TMP_DIR/deps-target.tsv"
  if [ "$dep_platform" = linux-musl-arm64 ]; then
    echo "· Fastfetch 的上游发布暂无 Linux musl ARM64 二进制。"
  fi
  if [ "$font_os" = linux ] && [ "$fonts_ready" = 1 ]; then
    if ! command -v fc-cache >/dev/null 2>&1 || ! fc-cache -f "$FONT_DIR"; then
      echo "✗ 无法刷新字体缓存，可运行 fc-cache -f。" >&2
    fi
  fi
  return 0
}

TARGET="$(detect_target)"
TAB="$(printf '\t')"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' 0
trap 'exit 1' HUP INT TERM
KISEKI_CONFIG_ROOT="${KISEKI_HOME:-${XDG_CONFIG_HOME:-$HOME/.config}/kiseki}"

if TAG="$(resolve_version)"; then
  echo "Installing Kiseki Terminal ${TAG} (${TARGET})..."
  BASE_URL="https://github.com/${REPO}/releases/download/${TAG}"
  if install_core; then
    echo "✓ Installed to ${INSTALL_DIR}/kiseki ($("${INSTALL_DIR}/kiseki" --version))"
  else
    result="$?"
    if [ "$result" != 2 ] || ! use_installed_core "$TAG"; then exit 1; fi
  fi
  install_dependencies
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
