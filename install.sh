#!/usr/bin/env bash
# ============================================================
# ★ Kiseki Terminal — One-Click Anime Theme Pack Installer for macOS & Unix
#   Supports Ghostty (60fps GLSL + CRT), iTerm2, Kitty, WezTerm + zsh / bash
#
# Usage (local):
#   ./install.sh
#   ./install.sh --theme nerv-eva --static --skip-tools --skip-fonts
#
# Usage (one-line remote):
#   curl -fsSL https://raw.githubusercontent.com/nerv-base/kiseki-terminal/main/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/nerv-base/kiseki-terminal/main/install.sh | bash -s -- --theme kyoto-sakura
# ============================================================

set -euo pipefail

REPO_SLUG="nerv-base/kiseki-terminal"
INSTALL_ROOT="${KISEKI_ROOT:-$HOME/.config/kiseki-terminal}"
THEME="starlight-8bit"
STATIC_MODE=0
SKIP_TOOLS=0
SKIP_FONTS=0
TERMINALS="auto"
UNINSTALL=0

CYAN='\033[0;36m'
GREEN='\033[0;32m'
MAGENTA='\033[0;35m'
YELLOW='\033[0;33m'
GRAY='\033[0;90m'
RED='\033[0;31m'
RESET='\033[0m'

usage() {
  cat <<'EOF'
Kiseki Terminal Installer (macOS / Unix)

Options:
  --theme <id>       Default theme: starlight-8bit | nerv-eva | shinkai-rain | kyoto-sakura
  --static           Start in zero-GPU static wallpaper mode (toggle anytime with `live`)
  --skip-tools       Skip Homebrew CLI tools install (oh-my-posh, eza, bat, fzf, zoxide, fastfetch)
  --skip-fonts       Skip curated anime & pixel font download
  --terminal <list>  Target terminals: auto (default) | ghostty | iterm2 | kitty | wezterm | all | none
  --uninstall        Remove Kiseki Terminal config hooks from shell & terminals
  -h, --help         Show this help
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --theme|-t)
      THEME="${2:-}"
      shift 2
      ;;
    --static)
      STATIC_MODE=1
      shift
      ;;
    --skip-tools)
      SKIP_TOOLS=1
      shift
      ;;
    --skip-fonts)
      SKIP_FONTS=1
      shift
      ;;
    --terminal|--terminals)
      TERMINALS="${2:-auto}"
      shift 2
      ;;
    --uninstall)
      UNINSTALL=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf "${RED}✖ Unknown option: %s${RESET}\n" "$1" >&2
      usage
      exit 1
      ;;
  esac
done

if ! command -v python3 >/dev/null 2>&1; then
  printf "${RED}✖ python3 is required. On macOS, run: xcode-select --install${RESET}\n" >&2
  exit 1
fi

if [ "$UNINSTALL" -eq 1 ]; then
  if [ -f "$INSTALL_ROOT/scripts/kiseki-engine.py" ]; then
    python3 "$INSTALL_ROOT/scripts/kiseki-engine.py" unhook
  else
    printf "${GRAY}Kiseki Terminal does not appear to be installed at %s${RESET}\n" "$INSTALL_ROOT"
  fi
  exit 0
fi

case "$THEME" in
  starlight-8bit|nerv-eva|shinkai-rain|kyoto-sakura) ;;
  *)
    printf "${RED}✖ Invalid theme '%s'. Choose: starlight-8bit | nerv-eva | shinkai-rain | kyoto-sakura${RESET}\n" "$THEME" >&2
    exit 1
    ;;
esac

printf "\n${CYAN}★ Installing Kiseki Anime Terminal Pack for macOS / Unix...${RESET}\n"

# 0. Resolve local repo root or download tarball for `curl | bash` one-liner
SCRIPT_DIR=""
if [ -n "${BASH_SOURCE[0]:-}" ] && [ -f "${BASH_SOURCE[0]}" ]; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi

TEMP_CLONE=""
cleanup() {
  if [ -n "$TEMP_CLONE" ] && [ -d "$TEMP_CLONE" ]; then
    rm -rf "$TEMP_CLONE"
  fi
}
trap cleanup EXIT

REPO_ROOT="$SCRIPT_DIR"
if [ -z "$REPO_ROOT" ] || [ ! -d "$REPO_ROOT/themes" ] || [ ! -f "$REPO_ROOT/scripts/kiseki-engine.py" ]; then
  TEMP_CLONE="$(mktemp -d "${TMPDIR:-/tmp}/kiseki-dl-XXXXXX")"
  ZIP_PATH="$TEMP_CLONE/repo.zip"
  printf "${CYAN}  [0/5] Downloading Kiseki Terminal from GitHub...${RESET}\n"
  curl -fsSL "https://github.com/${REPO_SLUG}/archive/refs/heads/main.zip" -o "$ZIP_PATH"
  python3 - "$ZIP_PATH" "$TEMP_CLONE" <<'PY'
import sys, zipfile
with zipfile.ZipFile(sys.argv[1]) as zf:
    zf.extractall(sys.argv[2])
PY
  REPO_ROOT="$(find "$TEMP_CLONE" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
fi

# 1. Backup existing shell & terminal configs
BACKUP_DIR="$HOME/.config/kiseki-backup-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$BACKUP_DIR"

backup_if_exists() {
  local src="$1" name="$2"
  if [ -f "$src" ]; then
    cp -f "$src" "$BACKUP_DIR/$name"
  fi
}

backup_if_exists "$HOME/.zshrc" "zshrc"
backup_if_exists "$HOME/.bashrc" "bashrc"
backup_if_exists "$HOME/.config/ghostty/config.ghostty" "ghostty-xdg-config.ghostty"
backup_if_exists "$HOME/.config/ghostty/config" "ghostty-xdg-config"
backup_if_exists "$HOME/Library/Application Support/com.mitchellh.ghostty/config.ghostty" "ghostty-appsupport-config.ghostty"
backup_if_exists "$HOME/Library/Application Support/com.mitchellh.ghostty/config" "ghostty-appsupport-config"
backup_if_exists "$HOME/Library/Application Support/iTerm2/DynamicProfiles/kiseki-terminal.json" "iterm2-kiseki.json"
backup_if_exists "$HOME/Library/Preferences/com.googlecode.iterm2.plist" "com.googlecode.iterm2.plist"
backup_if_exists "$HOME/Library/Preferences/com.apple.Terminal.plist" "com.apple.Terminal.plist"
backup_if_exists "$HOME/.config/kitty/kitty.conf" "kitty.conf"
backup_if_exists "$HOME/.wezterm.lua" "wezterm.lua"
printf "${GRAY}  [1/5] Backup saved to %s${RESET}\n" "$BACKUP_DIR"

# 2. Copy themes, scripts, shaders, version.json, and default bat config
mkdir -p "$INSTALL_ROOT"
rm -rf "$INSTALL_ROOT/themes" "$INSTALL_ROOT/scripts" "$INSTALL_ROOT/shaders"
cp -R "$REPO_ROOT/themes" "$INSTALL_ROOT/themes"
cp -R "$REPO_ROOT/scripts" "$INSTALL_ROOT/scripts"
cp -R "$REPO_ROOT/shaders" "$INSTALL_ROOT/shaders"
if [ -f "$REPO_ROOT/version.json" ]; then
  cp -f "$REPO_ROOT/version.json" "$INSTALL_ROOT/version.json"
fi
chmod +x "$INSTALL_ROOT/scripts/kiseki-engine.py" "$INSTALL_ROOT/scripts/kiseki.sh"

BAT_CFG_DIR="$HOME/.config/bat"
if [ ! -f "$BAT_CFG_DIR/config" ]; then
  mkdir -p "$BAT_CFG_DIR"
  cat > "$BAT_CFG_DIR/config" <<'EOF'
--theme="Catppuccin Mocha"
--style="numbers,changes,header,grid"
--italic-text=always
--paging=auto
EOF
fi

VER_LABEL="$(python3 -c 'import json,sys; print("v"+json.load(open(sys.argv[1]))["version"])' "$INSTALL_ROOT/version.json" 2>/dev/null || true)"
THEME_COUNT="$(find "$INSTALL_ROOT/themes" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')"
printf "${GREEN}  [2/5] Installed %s anime themes (%s) to %s${RESET}\n" "$THEME_COUNT" "$VER_LABEL" "$INSTALL_ROOT"

# 3. Optional CLI tools via Homebrew
if [ "$SKIP_TOOLS" -eq 0 ] && command -v brew >/dev/null 2>&1; then
  printf "${CYAN}  [3/5] Installing recommended CLI tools via Homebrew...${RESET}\n"
  BREW_PKGS=(oh-my-posh eza bat fzf zoxide fastfetch)
  for pkg in "${BREW_PKGS[@]}"; do
    if ! command -v "$pkg" >/dev/null 2>&1; then
      brew install "$pkg" >/dev/null 2>&1 || printf "${YELLOW}    ⚠ Could not install %s via brew (skipping)${RESET}\n" "$pkg"
    fi
  done
else
  printf "${GRAY}  [3/5] Skipped CLI tools install${RESET}\n"
fi

# 4. Optional curated fonts (DepartureMono NF, JetBrainsMono NF, Fusion Pixel 12px CN/JP, Maple Mono NF CN)
if [ "$SKIP_FONTS" -eq 0 ]; then
  printf "${CYAN}  [4/5] Installing curated anime & pixel fonts...${RESET}\n"
  if [ "$(uname -s)" = "Darwin" ]; then
    FONT_DIR="$HOME/Library/Fonts"
  else
    FONT_DIR="$HOME/.local/share/fonts/kiseki-terminal"
  fi
  mkdir -p "$FONT_DIR"

  python3 - "$FONT_DIR" <<'PY'
import json, re, shutil, subprocess, sys, tempfile, urllib.request, zipfile
from pathlib import Path

font_dir = Path(sys.argv[1])
sources = [
    ("ryanoasis/nerd-fonts",      r"^DepartureMono\.zip$",          r"\.(otf|ttf)$"),
    ("ryanoasis/nerd-fonts",      r"^JetBrainsMono\.zip$",          r"JetBrainsMonoNerdFont-(Regular|Bold|Italic)\.ttf$"),
    ("TakWolf/fusion-pixel-font", r"12px-monospaced-ttf.*\.zip$",   r"(zh_hans|ja)\.ttf$"),
    ("subframe7536/maple-font",   r"^MapleMono-NF-CN\.zip$",        r"-(Regular|Medium|Bold|Italic)\.ttf$"),
]

def fetch_to_file(url: str, dest: Path, timeout: int = 60) -> None:
    if shutil.which("curl"):
        subprocess.run(
            ["curl", "-fsSL", "--max-time", str(timeout), "-A", "KisekiTerminalInstaller", "-o", str(dest), url],
            check=True,
            capture_output=True,
        )
        return
    req = urllib.request.Request(url, headers={"User-Agent": "KisekiTerminalInstaller"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        dest.write_bytes(resp.read())

def fetch_json(url: str, timeout: int = 20) -> dict:
    if shutil.which("curl"):
        res = subprocess.run(
            ["curl", "-fsSL", "--max-time", str(timeout), "-A", "KisekiTerminalInstaller", url],
            check=True,
            capture_output=True,
            text=True,
        )
        return json.loads(res.stdout)
    req = urllib.request.Request(url, headers={"User-Agent": "KisekiTerminalInstaller"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read().decode("utf-8"))

installed_count = 0
with tempfile.TemporaryDirectory(prefix="kiseki-fonts-") as tmp:
    tmp_path = Path(tmp)
    for repo, pat, flt in sources:
        try:
            rel = fetch_json(f"https://api.github.com/repos/{repo}/releases/latest", timeout=20)
            asset = next((a for a in rel.get("assets", []) if re.search(pat, a["name"])), None)
            if not asset:
                continue
            zip_file = tmp_path / asset["name"]
            fetch_to_file(asset["browser_download_url"], zip_file, timeout=90)
            ext_dir = tmp_path / asset["name"].removesuffix(".zip")
            with zipfile.ZipFile(zip_file) as zf:
                zf.extractall(ext_dir)
            for f in ext_dir.rglob("*"):
                if not f.is_file():
                    continue
                if "__MACOSX" in f.parts or f.name.startswith("._"):
                    continue
                if re.search(flt, f.name):
                    shutil.copy2(f, font_dir / f.name)
                    installed_count += 1
        except Exception as exc:
            print(f"    ⚠ Could not download font from {repo}: {exc}", file=sys.stderr)

print(f"    ✓ Installed {installed_count} font files to {font_dir}")
PY

  if [ "$(uname -s)" != "Darwin" ] && command -v fc-cache >/dev/null 2>&1; then
    fc-cache -f "$FONT_DIR" >/dev/null 2>&1 || true
  fi
else
  printf "${GRAY}  [4/5] Skipped font install${RESET}\n"
fi

# 5. Wire zsh (and bash if ~/.bashrc exists) + hook & sync detected terminals
python3 - "$INSTALL_ROOT" <<'PY'
import sys
from pathlib import Path

root = Path(sys.argv[1])
sys.path.insert(0, str(root / "scripts"))
import importlib.util

spec = importlib.util.spec_from_file_location("kiseki_engine", root / "scripts" / "kiseki-engine.py")
eng = importlib.util.module_from_spec(spec)
spec.loader.exec_module(eng)

hook_body = (
    'if [ -f "$HOME/.config/kiseki-terminal/scripts/kiseki.sh" ]; then\n'
    '  . "$HOME/.config/kiseki-terminal/scripts/kiseki.sh"\n'
    'fi'
)

targets = [Path.home() / ".zshrc"]
if (Path.home() / ".bashrc").is_file():
    targets.append(Path.home() / ".bashrc")

for rc in targets:
    eng.upsert_hook_block(rc, eng.HOOK_BEGIN, eng.HOOK_END, hook_body)
PY

if [ "$STATIC_MODE" -eq 1 ]; then
  printf "off" > "$INSTALL_ROOT/live-mode.txt"
else
  printf "on" > "$INSTALL_ROOT/live-mode.txt"
fi
printf "off" > "$INSTALL_ROOT/crt-mode.txt"

KISEKI_ROOT="$INSTALL_ROOT" python3 "$INSTALL_ROOT/scripts/kiseki-engine.py" \
  switch "$THEME" --hook --quiet --terminals "$TERMINALS"

SHIM_JSON="$(KISEKI_ROOT="$INSTALL_ROOT" python3 "$INSTALL_ROOT/scripts/kiseki-engine.py" install-shims)"
SHIM_DIR="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1]).get("bin_dir",""))' "$SHIM_JSON")"

SYNCED_LIST="$(KISEKI_ROOT="$INSTALL_ROOT" python3 - "$INSTALL_ROOT" "$TERMINALS" <<'PY'
import importlib.util, sys
from pathlib import Path
root, spec_str = Path(sys.argv[1]), sys.argv[2]
spec = importlib.util.spec_from_file_location("kiseki_engine", root / "scripts" / "kiseki-engine.py")
eng = importlib.util.module_from_spec(spec)
spec.loader.exec_module(eng)
targets = eng.resolve_target_terminals(spec_str)
print(", ".join(targets) if targets else "shell-only")
PY
)"

if [ "$STATIC_MODE" -eq 1 ]; then
  MODE_LABEL="Static"
else
  MODE_LABEL="Live 60fps GLSL (Ghostty) / Static (iTerm2, Kitty, WezTerm)"
fi

printf "${GREEN}  [5/5] Activated theme: %s (%s)${RESET}\n" "$THEME" "$MODE_LABEL"
printf "${GRAY}        Synced terminals: %s${RESET}\n" "$SYNCED_LIST"
if [ -n "$SHIM_DIR" ]; then
  printf "${GRAY}        CLI shims ready in: %s (usable right now — no 'source ~/.zshrc' needed!)${RESET}\n" "$SHIM_DIR"
fi
printf "\n${MAGENTA}✦ Done! Try right now (no reload needed):${RESET}\n"
printf "${CYAN}  themes (↑/↓ interactive picker) | theme nerv-eva | live | crt | theme update${RESET}\n"
printf "${GRAY}  New tabs automatically load the Oh My Posh prompt & shell colors too.${RESET}\n"
if printf "%s" "$SYNCED_LIST" | grep -q "iterm2"; then
  printf "${GRAY}  iTerm2 tip: select profile 'Kiseki Active' (or any 'Kiseki: ...') in Settings → Profiles.${RESET}\n"
fi
printf "\n"
