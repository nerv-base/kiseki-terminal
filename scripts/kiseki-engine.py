#!/usr/bin/env python3
"""
★ Kiseki Terminal — Cross-Platform macOS/Unix Theme Engine
Supports Ghostty (with 60fps GLSL live shaders + CRT), iTerm2 (Dynamic Profiles + live OSC 1337),
Kitty, WezTerm, and universal zsh/bash shell integration (Oh My Posh, Fastfetch, fzf, eza, bat,
zsh-syntax-highlighting, zsh-autosuggestions, plus zero-lag 24h update checker).
"""

from __future__ import annotations

import argparse
import datetime
import json
import os
import platform
import re
import select
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import termios
import tty
import urllib.error
import urllib.request
import zipfile
from pathlib import Path
from typing import Any

REPO_SLUG = "nerv-base/kiseki-terminal"
REMOTE_MANIFEST_URL = f"https://raw.githubusercontent.com/{REPO_SLUG}/main/version.json"
RELEASES_URL = f"https://github.com/{REPO_SLUG}/releases"

HOOK_BEGIN = "# >>> kiseki-terminal >>>"
HOOK_END = "# <<< kiseki-terminal <<<"
LUA_HOOK_BEGIN = "-- >>> kiseki-terminal >>>"
LUA_HOOK_END = "-- <<< kiseki-terminal <<<"

FONT_STYLE_MAP: dict[str, list[str]] = {
    "8bit": [
        "DepartureMono Nerd Font",
        "Fusion Pixel 12px Mono zh-Hans",
        "Fusion Pixel 12px Mono ja",
        "JetBrainsMono NF",
    ],
    "code": [
        "Pixel Code",
        "Fusion Pixel 12px Mono zh-Hans",
        "Fusion Pixel 12px Mono ja",
        "JetBrainsMono NF",
    ],
    "maple": [
        "Maple Mono NF CN",
        "JetBrainsMono NF",
    ],
    "modern": [
        "JetBrainsMono NF",
        "Maple Mono NF CN",
    ],
}

# Maps Windows DirectWrite face names from theme.json to preferred macOS/CoreText candidates
FONT_CANDIDATES: dict[str, list[str]] = {
    "JetBrainsMono NF": [
        "JetBrainsMono Nerd Font",
        "JetBrainsMono Nerd Font Mono",
        "JetBrainsMonoNL Nerd Font",
        "JetBrainsMonoNL Nerd Font Mono",
        "JetBrains Mono",
    ],
    "DepartureMono Nerd Font": [
        "DepartureMono Nerd Font",
        "DepartureMono Nerd Font Mono",
        "Departure Mono",
    ],
    "Fusion Pixel 12px Mono zh-Hans": [
        "Fusion Pixel 12px Monospaced zh_hans",
        "Fusion Pixel 12px Mono zh_hans",
        "Fusion Pixel 12px Mono zh-Hans",
    ],
    "Fusion Pixel 12px Mono ja": [
        "Fusion Pixel 12px Monospaced ja",
        "Fusion Pixel 12px Mono ja",
    ],
    "Maple Mono NF CN": [
        "Maple Mono NF CN",
        "Maple Mono NF",
        "Maple Mono CN",
        "Maple Mono",
    ],
    "Pixel Code": [
        "Pixel Code",
        "PixelCode",
    ],
}

ITERM_PS_FONT_CANDIDATES: dict[str, list[str]] = {
    "JetBrainsMono NF": [
        "JetBrainsMonoNF-Regular",
        "JetBrainsMonoNFM-Regular",
        "JetBrainsMonoNerdFont-Regular",
        "JetBrainsMonoNerdFontMono-Regular",
        "JetBrainsMono-Regular",
    ],
    "DepartureMono Nerd Font": [
        "DepartureMonoNF-Regular",
        "DepartureMonoNFM-Regular",
        "DepartureMonoNerdFont-Regular",
        "DepartureMono-Regular",
    ],
    "Fusion Pixel 12px Mono zh-Hans": [
        "FusionPixel12pxMonospacedzh_hans-Regular",
        "Fusion-Pixel-12px-Monospaced-zh_hans",
        "FusionPixel12pxMonozh_hans-Regular",
    ],
    "Maple Mono NF CN": [
        "MapleMono-NF-CN-Regular",
        "MapleMonoNFCN-Regular",
        "MapleMono-Regular",
    ],
    "Pixel Code": [
        "PixelCode-Regular",
        "PixelCode",
    ],
}

ANSI_KEYS = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "purple",
    "cyan",
    "white",
    "brightBlack",
    "brightRed",
    "brightGreen",
    "brightYellow",
    "brightBlue",
    "brightPurple",
    "brightCyan",
    "brightWhite",
]


def get_root() -> Path:
    return Path(os.environ.get("KISEKI_ROOT", str(Path.home() / ".config" / "kiseki-terminal"))).expanduser()


def read_text_trimmed(path: Path, default: str = "") -> str:
    try:
        return path.read_text(encoding="utf-8").strip()
    except OSError:
        return default


def write_text_atomic(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(f".{path.name}.tmp.{os.getpid()}")
    tmp.write_text(content, encoding="utf-8")
    os.replace(tmp, path)


def list_theme_ids(root: Path) -> list[str]:
    themes_dir = root / "themes"
    if not themes_dir.is_dir():
        return []
    ids = []
    for entry in sorted(themes_dir.iterdir(), key=lambda p: p.name):
        if entry.is_dir() and (entry / "theme.json").is_file():
            ids.append(entry.name)
    return ids


def load_theme(root: Path, theme_id: str) -> dict[str, Any]:
    path = root / "themes" / theme_id / "theme.json"
    if not path.is_file():
        raise FileNotFoundError(f"Theme '{theme_id}' not found at {path}")
    return json.loads(path.read_text(encoding="utf-8"))


def get_active_theme_id(root: Path) -> str:
    saved = read_text_trimmed(root / "current-theme.txt")
    if saved and (root / "themes" / saved / "theme.json").is_file():
        return saved
    ids = list_theme_ids(root)
    if "starlight-8bit" in ids:
        return "starlight-8bit"
    return ids[0] if ids else "starlight-8bit"


def get_live_mode(root: Path) -> bool:
    return read_text_trimmed(root / "live-mode.txt", "on").lower() != "off"


def get_crt_mode(root: Path) -> bool:
    return read_text_trimmed(root / "crt-mode.txt", "off").lower() == "on"


def get_bg_override(root: Path) -> str | None:
    val = read_text_trimmed(root / "bg-opacity.txt")
    return val if val else None


def get_font_override(root: Path) -> dict[str, Any] | None:
    p = root / "font-override.json"
    if not p.is_file():
        return None
    try:
        data = json.loads(p.read_text(encoding="utf-8"))
        if isinstance(data, dict) and data.get("style") in FONT_STYLE_MAP:
            return data
    except (OSError, json.JSONDecodeError):
        pass
    return None


def parse_hex_rgb(hex_color: str) -> tuple[float, float, float]:
    h = hex_color.strip().lstrip("#")
    if len(h) != 6:
        return (0.0, 0.0, 0.0)
    return (int(h[0:2], 16) / 255.0, int(h[2:4], 16) / 255.0, int(h[4:6], 16) / 255.0)


def parse_padding(padding_str: str) -> tuple[int, int]:
    """Parse Windows Terminal 'left, top, right, bottom' -> (pad_x, pad_y)."""
    parts = [int(x.strip()) for x in re.split(r"[,\s]+", padding_str.strip()) if x.strip().isdigit()]
    if len(parts) >= 2:
        return (parts[0], parts[1])
    if len(parts) == 1:
        return (parts[0], parts[0])
    return (16, 12)


def wt_size_to_macos_pt(wt_size: float) -> float:
    """Windows DirectWrite 11pt (96 DPI) visually matches ~13.5-14pt on macOS CoreText (72 DPI)."""
    if platform.system() == "Darwin":
        return round(wt_size * 1.25 * 2.0) / 2.0
    return float(wt_size)


def cell_height_to_ghostty_adjust(cell_height: str | float) -> str:
    try:
        ch = float(cell_height)
    except (TypeError, ValueError):
        return "15%"
    # Ghostty's default line height is ~1.12x; map WT 1.26..1.30 -> +12%..+18%
    pct = max(0, min(40, round((ch - 1.12) * 100)))
    return f"{pct}%"


def find_ghostty_bin() -> str | None:
    found = shutil.which("ghostty")
    if found:
        return found
    mac_bin = Path("/Applications/Ghostty.app/Contents/MacOS/ghostty")
    if mac_bin.is_file():
        return str(mac_bin)
    user_bin = Path.home() / "Applications" / "Ghostty.app" / "Contents" / "MacOS" / "ghostty"
    if user_bin.is_file():
        return str(user_bin)
    return None


def get_installed_ghostty_fonts() -> set[str]:
    gbin = find_ghostty_bin()
    if not gbin:
        return set()
    try:
        res = subprocess.run(
            [gbin, "+list-fonts"],
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
        if res.returncode != 0:
            return set()
        families: set[str] = set()
        for line in res.stdout.splitlines():
            if line and not line[0].isspace():
                families.add(line.strip())
        return families
    except (OSError, subprocess.SubprocessError):
        return set()


def resolve_font_faces(raw_faces: list[str], installed: set[str] | None = None) -> list[str]:
    """Resolve theme font face names to installed macOS/Unix font families."""
    resolved: list[str] = []
    seen: set[str] = set()
    for raw in raw_faces:
        name = raw.strip()
        if not name:
            continue
        candidates = FONT_CANDIDATES.get(name, [name])
        chosen: str | None = None
        if installed:
            # Exact match first
            for cand in candidates:
                if cand in installed:
                    chosen = cand
                    break
            # Case-insensitive / prefix match fallback
            if not chosen:
                lower_map = {f.lower(): f for f in installed}
                for cand in candidates:
                    if cand.lower() in lower_map:
                        chosen = lower_map[cand.lower()]
                        break
        else:
            chosen = candidates[0]
        if chosen and chosen not in seen:
            seen.add(chosen)
            resolved.append(chosen)
    return resolved


def get_effective_fonts(root: Path, theme: dict[str, Any]) -> tuple[list[str], float]:
    override = get_font_override(root)
    if override:
        faces = FONT_STYLE_MAP[override["style"]]
        size = float(override.get("size") or wt_size_to_macos_pt(float(theme["font"]["size"])))
        return (faces, size)
    raw_face = str(theme.get("font", {}).get("face", ""))
    faces = [x.strip() for x in raw_face.split(",") if x.strip()]
    size = wt_size_to_macos_pt(float(theme.get("font", {}).get("size", 11.0)))
    return (faces, size)


def get_effective_bg_opacity(root: Path, theme: dict[str, Any]) -> float | None:
    """Return float opacity (0.0..1.0) or None if wallpaper is turned off."""
    override = get_bg_override(root)
    if override is not None:
        if override.lower() == "off":
            return None
        try:
            return max(0.0, min(1.0, float(override)))
        except ValueError:
            pass
    return float(theme.get("terminal", {}).get("backgroundImageOpacity", 0.42))


def detect_installed_terminals() -> list[str]:
    detected: list[str] = []
    home = Path.home()

    # Ghostty
    if (
        find_ghostty_bin()
        or (home / ".config" / "ghostty").exists()
        or (home / "Library" / "Application Support" / "com.mitchellh.ghostty").exists()
    ):
        detected.append("ghostty")

    # iTerm2
    if (
        Path("/Applications/iTerm.app").exists()
        or (home / "Applications" / "iTerm.app").exists()
        or (home / "Library" / "Application Support" / "iTerm2").exists()
    ):
        detected.append("iterm2")

    # Kitty
    if (
        shutil.which("kitty")
        or Path("/Applications/kitty.app").exists()
        or (home / ".config" / "kitty").exists()
    ):
        detected.append("kitty")

    # WezTerm
    if (
        shutil.which("wezterm")
        or Path("/Applications/WezTerm.app").exists()
        or (home / ".wezterm.lua").exists()
        or (home / ".config" / "wezterm").exists()
    ):
        detected.append("wezterm")

    return detected


def detect_current_terminal() -> str:
    tp = os.environ.get("TERM_PROGRAM", "")
    if tp == "ghostty" or os.environ.get("GHOSTTY_RESOURCES_DIR"):
        return "ghostty"
    if tp == "iTerm.app" or os.environ.get("ITERM_SESSION_ID"):
        return "iterm2"
    if os.environ.get("KITTY_WINDOW_ID") or os.environ.get("TERM") == "xterm-kitty":
        return "kitty"
    if tp == "WezTerm" or os.environ.get("WEZTERM_PANE"):
        return "wezterm"
    if tp == "Apple_Terminal":
        return "terminal-app"
    if tp == "WarpTerminal":
        return "warp"
    return tp.lower() if tp else "unknown"


# ---------------------------------------------------------------------------
# Guarded config hook helper (non-destructive, idempotent, cleanly removable)
# ---------------------------------------------------------------------------

def upsert_hook_block(path: Path, begin: str, end: str, body: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    existing = path.read_text(encoding="utf-8") if path.is_file() else ""
    block = f"{begin}\n{body.rstrip()}\n{end}\n"
    pattern = re.compile(rf"{re.escape(begin)}.*?{re.escape(end)}\n?", re.DOTALL)
    if pattern.search(existing):
        updated = pattern.sub(block, existing, count=1)
    else:
        sep = "" if (not existing or existing.endswith("\n")) else "\n"
        prefix = "\n" if existing.strip() else ""
        updated = f"{existing}{sep}{prefix}{block}"
    write_text_atomic(path, updated)


def remove_hook_block(path: Path, begin: str, end: str) -> bool:
    if not path.is_file():
        return False
    existing = path.read_text(encoding="utf-8")
    pattern = re.compile(rf"\n?{re.escape(begin)}.*?{re.escape(end)}\n?", re.DOTALL)
    if not pattern.search(existing):
        return False
    updated = pattern.sub("\n", existing).lstrip("\n")
    write_text_atomic(path, updated)
    return True


# ---------------------------------------------------------------------------
# Ghostty Adapter (1st-class: wallpaper, palette, fonts, blur, 60fps GLSL + CRT)
# ---------------------------------------------------------------------------

def find_ghostty_config_path() -> Path:
    """
    Pick the highest-precedence existing non-empty Ghostty config file so Kiseki's
    `config-file = ?...` isn't overridden by a later config file; default to XDG `config.ghostty`.
    """
    home = Path.home()
    xdg_dir = Path(os.environ.get("XDG_CONFIG_HOME", str(home / ".config"))) / "ghostty"
    candidates = [
        home / "Library" / "Application Support" / "com.mitchellh.ghostty" / "config.ghostty",
        home / "Library" / "Application Support" / "com.mitchellh.ghostty" / "config",
        xdg_dir / "config.ghostty",
        xdg_dir / "config",
    ]
    for c in candidates:
        if c.is_file() and c.stat().st_size > 0:
            return c
    # Prefer existing file even if empty (XDG first so dotfiles stay tidy)
    for c in [xdg_dir / "config.ghostty", xdg_dir / "config"]:
        if c.is_file():
            return c
    return xdg_dir / "config.ghostty"


def generate_ghostty_conf(root: Path, theme_id: str) -> Path:
    theme = load_theme(root, theme_id)
    scheme = theme["scheme"]
    term = theme["terminal"]
    theme_dir = (root / "themes" / theme_id).resolve()
    wallpaper = theme_dir / "wallpaper.png"
    live_glsl = theme_dir / "live.glsl"
    crt_glsl = (root / "shaders" / "retro-crt-subtle.glsl").resolve()

    use_live = get_live_mode(root)
    use_crt = get_crt_mode(root)
    bg_op = get_effective_bg_opacity(root, theme)

    raw_faces, font_size = get_effective_fonts(root, theme)
    installed_fonts = get_installed_ghostty_fonts()
    resolved_faces = resolve_font_faces(raw_faces, installed_fonts if installed_fonts else None)

    cursor_map = {
        "filledBox": "block",
        "bar": "bar",
        "underline": "underline",
        "emptyBox": "block_hollow",
    }
    cursor_style = cursor_map.get(term.get("cursorShape", "filledBox"), "block")

    align_map = {
        "bottomRight": "bottom-right",
        "bottomLeft": "bottom-left",
        "topRight": "top-right",
        "topLeft": "top-left",
        "center": "center",
    }
    bg_pos = align_map.get(term.get("backgroundImageAlignment", "bottomRight"), "bottom-right")

    pad_x, pad_y = parse_padding(str(term.get("padding", "18, 14, 18, 14")))
    win_opacity = max(0.2, min(1.0, float(term.get("opacity", 94)) / 100.0))
    use_blur = bool(term.get("useAcrylic", True))
    cell_adjust = cell_height_to_ghostty_adjust(theme.get("font", {}).get("cellHeight", "1.28"))

    lines = [
        f"# ★ Kiseki Terminal — Ghostty Theme ({theme['name']} / {theme_id})",
        "# Auto-generated by kiseki-engine.py — do not hand-edit (run `theme <id>` to switch)",
        "",
        "# --- Palette & Core Colors ---",
        "theme =",
        f"background = {scheme['background']}",
        f"foreground = {scheme['foreground']}",
        f"cursor-color = {scheme['cursorColor']}",
        f"cursor-style = {cursor_style}",
        f"selection-background = {scheme['selectionBackground']}",
        f"selection-foreground = {scheme['brightWhite']}",
    ]
    for idx, key in enumerate(ANSI_KEYS):
        lines.append(f"palette = {idx}={scheme[key]}")

    lines.extend(
        [
            "",
            "# --- Window, Blur & Padding ---",
            f"background-opacity = {win_opacity:.2f}",
            "background-opacity-cells = false",
            f"background-blur = {'20' if use_blur else 'false'}",
            "macos-titlebar-style = transparent",
            f"window-padding-x = {pad_x}",
            f"window-padding-y = {pad_y}",
            "window-padding-balance = true",
            "shell-integration-features = no-cursor",
        ]
    )

    if resolved_faces:
        lines.append("")
        lines.append("# --- Curated Font Stack ---")
        lines.append('font-family = ""')
        for face in resolved_faces:
            lines.append(f'font-family = "{face}"')
        if platform.system() == "Darwin" and "PingFang SC" not in resolved_faces:
            lines.append('font-family = "PingFang SC"')
    lines.append(f"font-size = {font_size:g}")
    lines.append(f"adjust-cell-height = {cell_adjust}")

    lines.append("")
    lines.append("# --- Wallpaper (UniformToFill / Cover + BottomRight Anchor) ---")
    if bg_op is not None and wallpaper.is_file():
        lines.append(f"background-image = {wallpaper}")
        lines.append(f"background-image-opacity = {bg_op:.2f}")
        lines.append(f"background-image-position = {bg_pos}")
        lines.append("background-image-fit = cover")
        lines.append("background-image-repeat = false")
    else:
        lines.append("background-image =")

    lines.append("")
    lines.append("# --- 60fps Live GLSL / Retro CRT Shader ---")
    lines.append("custom-shader =")
    if use_crt and crt_glsl.is_file():
        lines.append(f"custom-shader = {crt_glsl}")
        lines.append("custom-shader-animation = false")
    elif use_live and live_glsl.is_file():
        lines.append(f"custom-shader = {live_glsl}")
        lines.append("custom-shader-animation = true")
    else:
        lines.append("custom-shader-animation = false")

    out_path = root / "generated" / "ghostty.conf"
    write_text_atomic(out_path, "\n".join(lines) + "\n")
    return out_path


def hook_ghostty_config(root: Path, generated_conf: Path) -> Path:
    cfg_path = find_ghostty_config_path()
    upsert_hook_block(cfg_path, HOOK_BEGIN, HOOK_END, f"config-file = ?{generated_conf.resolve()}")
    return cfg_path


def ghostty_supports_macos_sigusr2(root: Path | None = None) -> bool:
    """Ghostty >= 1.3 on macOS natively handles SIGUSR2 to hard-reload config with zero TCC/AppleScript issues."""
    gbin = find_ghostty_bin()
    if not gbin:
        return False
    gpath = Path(gbin)
    try:
        mtime = str(int(gpath.stat().st_mtime))
    except OSError:
        mtime = ""

    cache_file = (root / ".ghostty-sigusr2-cache") if root else None
    if cache_file and mtime and cache_file.is_file():
        cached = read_text_trimmed(cache_file)
        if cached.startswith(mtime + ":"):
            return cached.endswith(":1")

    supported = False
    try:
        res = subprocess.run([gbin, "+version"], capture_output=True, text=True, timeout=3, check=False)
        m = re.search(r"Ghostty\s+(\d+\.\d+\.\d+)", res.stdout or "")
        if m and parse_version(m.group(1)) >= (1, 3, 0):
            supported = True
    except (OSError, subprocess.SubprocessError):
        pass

    if cache_file and mtime:
        try:
            write_text_atomic(cache_file, f"{mtime}:{'1' if supported else '0'}")
        except OSError:
            pass
    return supported


def _find_process_pids(
    exe_names: tuple[str, ...],
    mac_app_suffix: str | None = None,
    exclude_cli_plus: bool = False,
) -> list[int]:
    """
    Find running process PIDs WITHOUT excluding ancestors.
    Important: macOS/BSD `pgrep` and `pkill` exclude all ancestor processes by default
    (hiding the enclosing terminal when run from inside Ghostty/Kitty unless `-a` is given,
    whereas GNU `pgrep -a` has a different meaning). Using `ps` works reliably on both macOS and Linux.
    """
    me = os.getpid()
    pids: list[int] = []
    lower_names = {n.lower() for n in exe_names}
    mac_suffix_lower = mac_app_suffix.lower() if mac_app_suffix else None

    try:
        res = subprocess.run(
            ["ps", "-axo", "pid=,comm="],
            capture_output=True,
            text=True,
            timeout=3,
            check=False,
        )
        if res.returncode == 0:
            for line in res.stdout.splitlines():
                stripped = line.strip()
                if not stripped:
                    continue
                parts = stripped.split(None, 1)
                if len(parts) < 2 or not parts[0].isdigit():
                    continue
                pid = int(parts[0])
                if pid == me:
                    continue
                comm = parts[1].strip()
                comm_lower = comm.lower()
                base = Path(comm).name.lower()
                if base in lower_names or (mac_suffix_lower and comm_lower.endswith(mac_suffix_lower)):
                    if pid not in pids:
                        pids.append(pid)
    except (OSError, subprocess.SubprocessError):
        pass

    if not pids:
        is_bsd = platform.system() in ("Darwin", "FreeBSD", "OpenBSD", "NetBSD")
        for name in exe_names:
            cmd = ["pgrep", "-a", "-x", name] if is_bsd else ["pgrep", "-x", name]
            try:
                res = subprocess.run(cmd, capture_output=True, text=True, timeout=3, check=False)
                if res.returncode == 0:
                    for line in res.stdout.splitlines():
                        line = line.strip()
                        if line.isdigit():
                            pid = int(line)
                            if pid != me and pid not in pids:
                                pids.append(pid)
            except (OSError, subprocess.SubprocessError):
                continue

    if exclude_cli_plus and pids:
        # Exclude transient CLI invocations like `ghostty +list-fonts` / `ghostty +version`
        try:
            pid_list = ",".join(str(p) for p in pids)
            res = subprocess.run(
                ["ps", "-p", pid_list, "-o", "pid=,args="],
                capture_output=True,
                text=True,
                timeout=3,
                check=False,
            )
            if res.returncode == 0:
                cli_pids: set[int] = set()
                for line in res.stdout.splitlines():
                    stripped = line.strip()
                    if not stripped:
                        continue
                    parts = stripped.split(None, 1)
                    if len(parts) == 2 and parts[0].isdigit():
                        tokens = parts[1].split()[1:]
                        if any(tok.startswith("+") for tok in tokens):
                            cli_pids.add(int(parts[0]))
                if cli_pids:
                    pids = [p for p in pids if p not in cli_pids]
        except (OSError, subprocess.SubprocessError):
            pass

    return pids


def _find_ghostty_pids() -> list[int]:
    return _find_process_pids(
        ("ghostty",),
        mac_app_suffix="/ghostty.app/contents/macos/ghostty",
        exclude_cli_plus=True,
    )


def reload_ghostty(root: Path | None = None) -> tuple[bool, str]:
    """Reload running Ghostty instances. Returns (reloaded, reason)."""
    system = platform.system()
    pids = _find_ghostty_pids()
    if not pids:
        return (False, "not_running")

    # Prefer SIGUSR2 on Linux and macOS Ghostty >= 1.3 (instant, zero TCC Automation permissions needed)
    if system == "Linux" or (system == "Darwin" and ghostty_supports_macos_sigusr2(root)):
        signaled = False
        for pid in pids:
            try:
                os.kill(pid, signal.SIGUSR2)
                signaled = True
            except OSError:
                continue
        if signaled:
            return (True, "sigusr2")

    # Fallback: native Ghostty AppleScript (macOS)
    if system == "Darwin":
        script = (
            'tell application "Ghostty"\n'
            '  if (count of windows) > 0 then\n'
            '    perform action "reload_config" on focused terminal of selected tab of front window\n'
            '  end if\n'
            'end tell'
        )
        try:
            res = subprocess.run(["osascript", "-e", script], capture_output=True, text=True, timeout=4, check=False)
            if res.returncode == 0:
                return (True, "applescript")
            return (False, (res.stderr or "").strip() or "applescript_failed")
        except subprocess.SubprocessError as exc:
            return (False, str(exc))

    return (False, "unsupported")


# ---------------------------------------------------------------------------
# iTerm2 Adapter (Dynamic Profiles auto-watched by iTerm2 + live OSC 1337)
# ---------------------------------------------------------------------------

def iterm_color_dict(hex_color: str) -> dict[str, Any]:
    r, g, b = parse_hex_rgb(hex_color)
    return {
        "Red Component": round(r, 6),
        "Green Component": round(g, 6),
        "Blue Component": round(b, 6),
        "Alpha Component": 1.0,
        "Color Space": "sRGB",
    }


def build_iterm2_profile(
    root: Path,
    theme_id: str,
    profile_name: str,
    guid: str,
    use_overrides: bool = False,
) -> dict[str, Any]:
    theme = load_theme(root, theme_id)
    scheme = theme["scheme"]
    term = theme["terminal"]
    ui = theme.get("uiTheme", {})
    wallpaper = (root / "themes" / theme_id / "wallpaper.png").resolve()

    if use_overrides:
        bg_op = get_effective_bg_opacity(root, theme)
        raw_faces, font_size = get_effective_fonts(root, theme)
    else:
        bg_op = float(term.get("backgroundImageOpacity", 0.42))
        raw_faces = [x.strip() for x in str(theme["font"]["face"]).split(",") if x.strip()]
        font_size = wt_size_to_macos_pt(float(theme["font"]["size"]))

    cursor_map = {
        "underline": 0,
        "bar": 1,
        "filledBox": 2,
        "emptyBox": 2,
    }
    cursor_type = cursor_map.get(term.get("cursorShape", "filledBox"), 2)
    transparency = max(0.0, min(0.8, 1.0 - float(term.get("opacity", 94)) / 100.0))

    primary_raw = raw_faces[0] if raw_faces else "JetBrainsMono NF"
    non_ascii_raw = raw_faces[1] if len(raw_faces) > 1 else primary_raw
    primary_font = ITERM_PS_FONT_CANDIDATES.get(primary_raw, [primary_raw])[0]
    non_ascii_font = ITERM_PS_FONT_CANDIDATES.get(non_ascii_raw, [non_ascii_raw])[0]

    try:
        v_spacing = round(max(1.0, float(theme["font"].get("cellHeight", 1.25)) * 0.92), 2)
    except (TypeError, ValueError):
        v_spacing = 1.15

    tab_bg = ui.get("tab", {}).get("background", scheme["background"])

    profile: dict[str, Any] = {
        "Name": profile_name,
        "Guid": guid,
        "Tags": ["Kiseki Terminal"],
        "Background Color": iterm_color_dict(scheme["background"]),
        "Foreground Color": iterm_color_dict(scheme["foreground"]),
        "Bold Color": iterm_color_dict(scheme["brightWhite"]),
        "Cursor Color": iterm_color_dict(scheme["cursorColor"]),
        "Cursor Text Color": iterm_color_dict(scheme["background"]),
        "Selection Color": iterm_color_dict(scheme["selectionBackground"]),
        "Selected Text Color": iterm_color_dict(scheme["brightWhite"]),
        "Tab Color": iterm_color_dict(tab_bg),
        "Use Tab Color": True,
        "Cursor Type": cursor_type,
        "Blinking Cursor": False,
        "Transparency": round(transparency, 3),
        "Blur": bool(term.get("useAcrylic", True)),
        "Blur Radius": 18.0,
        "Background Image Location": str(wallpaper) if (bg_op is not None and wallpaper.is_file()) else "",
        "Background Image Mode": 2,  # 2 = Scale to Fill (Aspect Fill)
        "Blend": round(bg_op, 3) if bg_op is not None else 0.0,
        "Normal Font": f"{primary_font} {int(round(font_size))}",
        "Use Non-ASCII Font": non_ascii_font != primary_font,
        "Non Ascii Font": f"{non_ascii_font} {int(round(font_size))}",
        "Vertical Spacing": v_spacing,
        "Horizontal Spacing": 1.0,
        "Minimum Contrast": 0.0,
        "Silence Bell": True,
        "Unlimited Scrollback": True,
        "Option Key Sends": 2,  # Esc+
    }
    for idx, key in enumerate(ANSI_KEYS):
        profile[f"Ansi {idx} Color"] = iterm_color_dict(scheme[key])
    return profile


def sync_iterm2(root: Path, active_theme_id: str) -> Path:
    profiles: list[dict[str, Any]] = [
        build_iterm2_profile(
            root,
            active_theme_id,
            profile_name="Kiseki Active",
            guid="kiseki-terminal-active-v1",
            use_overrides=True,
        )
    ]
    for tid in list_theme_ids(root):
        meta = load_theme(root, tid)
        profiles.append(
            build_iterm2_profile(
                root,
                tid,
                profile_name=f"Kiseki: {meta['name']}",
                guid=f"kiseki-terminal-{tid}-v1",
                use_overrides=(tid == active_theme_id),
            )
        )

    out_path = Path.home() / "Library" / "Application Support" / "iTerm2" / "DynamicProfiles" / "kiseki-terminal.json"
    write_text_atomic(out_path, json.dumps({"Profiles": profiles}, indent=2, ensure_ascii=False) + "\n")
    return out_path


# ---------------------------------------------------------------------------
# Kitty Adapter (include + SIGUSR1 live reload)
# ---------------------------------------------------------------------------

def generate_kitty_conf(root: Path, theme_id: str) -> Path:
    theme = load_theme(root, theme_id)
    scheme = theme["scheme"]
    term = theme["terminal"]
    ui = theme.get("uiTheme", {})
    wallpaper = (root / "themes" / theme_id / "wallpaper.png").resolve()

    bg_op = get_effective_bg_opacity(root, theme)
    raw_faces, font_size = get_effective_fonts(root, theme)
    resolved_faces = resolve_font_faces(raw_faces, None)
    pad_x, pad_y = parse_padding(str(term.get("padding", "18, 14, 18, 14")))
    win_opacity = max(0.2, min(1.0, float(term.get("opacity", 94)) / 100.0))

    cursor_map = {"filledBox": "block", "bar": "beam", "underline": "underline"}
    cursor_shape = cursor_map.get(term.get("cursorShape", "filledBox"), "block")

    # In Kitty, background_tint = 1.0 - image_visibility (higher tint = darker background color over image)
    bg_tint = round(1.0 - bg_op, 2) if bg_op is not None else 1.0

    lines = [
        f"# ★ Kiseki Terminal — Kitty Theme ({theme['name']} / {theme_id})",
        f"background {scheme['background']}",
        f"foreground {scheme['foreground']}",
        f"cursor {scheme['cursorColor']}",
        f"cursor_shape {cursor_shape}",
        f"selection_background {scheme['selectionBackground']}",
        f"selection_foreground {scheme['brightWhite']}",
        f"active_tab_background {ui.get('tab', {}).get('background', scheme['background'])}",
        f"active_tab_foreground {scheme['brightWhite']}",
        f"inactive_tab_background {ui.get('tab', {}).get('unfocusedBackground', scheme['black'])}",
        f"inactive_tab_foreground {scheme['brightBlack']}",
        f"tab_bar_background {ui.get('tabRow', {}).get('background', scheme['background'])}",
    ]
    for idx, key in enumerate(ANSI_KEYS):
        lines.append(f"color{idx} {scheme[key]}")

    lines.extend(
        [
            f"background_opacity {win_opacity:.2f}",
            f"background_blur {'20' if term.get('useAcrylic', True) else '0'}",
            f"window_padding_width {pad_y} {pad_x}",
            f"font_size {font_size:g}",
        ]
    )
    if resolved_faces:
        lines.append(f"font_family {resolved_faces[0]}")

    if bg_op is not None and wallpaper.is_file():
        lines.append(f"background_image {wallpaper}")
        lines.append("background_image_layout cscaled")
        lines.append(f"background_tint {bg_tint:.2f}")
    else:
        lines.append("background_image none")

    out_path = root / "generated" / "kitty.conf"
    write_text_atomic(out_path, "\n".join(lines) + "\n")
    return out_path


def reload_kitty() -> bool:
    pids = _find_process_pids(("kitty",), mac_app_suffix="/kitty.app/contents/macos/kitty")
    signaled = False
    for pid in pids:
        try:
            os.kill(pid, signal.SIGUSR1)
            signaled = True
        except OSError:
            continue
    return signaled


def hook_and_reload_kitty(root: Path, generated_conf: Path, hook: bool = True) -> None:
    cfg = Path.home() / ".config" / "kitty" / "kitty.conf"
    if hook:
        upsert_hook_block(cfg, HOOK_BEGIN, HOOK_END, f"include {generated_conf.resolve()}")
    reload_kitty()


# ---------------------------------------------------------------------------
# WezTerm Adapter (generated Lua module + automatic hot-reload)
# ---------------------------------------------------------------------------

def lua_str(s: str) -> str:
    return json.dumps(s, ensure_ascii=False)


def generate_wezterm_lua(root: Path, theme_id: str) -> Path:
    theme = load_theme(root, theme_id)
    scheme = theme["scheme"]
    term = theme["terminal"]
    ui = theme.get("uiTheme", {})
    wallpaper = (root / "themes" / theme_id / "wallpaper.png").resolve()

    bg_op = get_effective_bg_opacity(root, theme)
    raw_faces, font_size = get_effective_fonts(root, theme)
    resolved_faces = resolve_font_faces(raw_faces, None)
    pad_x, pad_y = parse_padding(str(term.get("padding", "18, 14, 18, 14")))
    win_opacity = max(0.2, min(1.0, float(term.get("opacity", 94)) / 100.0))

    cursor_map = {"filledBox": "SteadyBlock", "bar": "SteadyBar", "underline": "SteadyUnderline"}
    cursor_style = cursor_map.get(term.get("cursorShape", "filledBox"), "SteadyBlock")
    try:
        line_height = round(max(1.0, float(theme["font"].get("cellHeight", 1.26)) * 0.94), 2)
    except (TypeError, ValueError):
        line_height = 1.18

    ansi_lua = ", ".join(lua_str(scheme[k]) for k in ANSI_KEYS[:8])
    brights_lua = ", ".join(lua_str(scheme[k]) for k in ANSI_KEYS[8:])
    fonts_lua = ", ".join(lua_str(f) for f in resolved_faces) if resolved_faces else '"JetBrainsMono Nerd Font"'

    bg_block = ""
    if bg_op is not None and wallpaper.is_file():
        bg_block = (
            f"  config.background = {{\n"
            f"    {{ source = {{ Color = {lua_str(scheme['background'])} }}, width = '100%', height = '100%', opacity = {win_opacity:.2f} }},\n"
            f"    {{ source = {{ File = {lua_str(str(wallpaper))} }}, horizontal_align = 'Right', vertical_align = 'Bottom', "
            f"width = 'Cover', height = 'Cover', opacity = {bg_op:.2f} }},\n"
            f"  }}\n"
        )
    else:
        bg_block = (
            f"  config.background = {{\n"
            f"    {{ source = {{ Color = {lua_str(scheme['background'])} }}, width = '100%', height = '100%', opacity = {win_opacity:.2f} }},\n"
            f"  }}\n"
        )

    lua_src = f"""-- ★ Kiseki Terminal — WezTerm Theme Module ({theme['name']} / {theme_id})
-- Auto-generated by kiseki-engine.py — require and call `apply(config)` in ~/.wezterm.lua
local wezterm = require 'wezterm'
local M = {{}}

M.theme_id = {lua_str(theme_id)}
M.theme_name = {lua_str(theme['name'])}

function M.apply(config)
  config.colors = {{
    foreground = {lua_str(scheme['foreground'])},
    background = {lua_str(scheme['background'])},
    cursor_bg = {lua_str(scheme['cursorColor'])},
    cursor_border = {lua_str(scheme['cursorColor'])},
    cursor_fg = {lua_str(scheme['background'])},
    selection_bg = {lua_str(scheme['selectionBackground'])},
    selection_fg = {lua_str(scheme['brightWhite'])},
    ansi = {{ {ansi_lua} }},
    brights = {{ {brights_lua} }},
    tab_bar = {{
      background = {lua_str(ui.get('tabRow', {}).get('background', scheme['background']))},
      active_tab = {{
        bg_color = {lua_str(ui.get('tab', {}).get('background', scheme['background']))},
        fg_color = {lua_str(scheme['brightWhite'])},
      }},
      inactive_tab = {{
        bg_color = {lua_str(ui.get('tab', {}).get('unfocusedBackground', scheme['black']))},
        fg_color = {lua_str(scheme['brightBlack'])},
      }},
    }},
  }}
  config.font = wezterm.font_with_fallback({{ {fonts_lua} }})
  config.font_size = {font_size:g}
  config.line_height = {line_height}
  config.default_cursor_style = {lua_str(cursor_style)}
  config.window_background_opacity = {win_opacity:.2f}
  config.macos_window_background_blur = {20 if term.get('useAcrylic', True) else 0}
  config.window_padding = {{ left = {pad_x}, right = {pad_x}, top = {pad_y}, bottom = {pad_y} }}
{bg_block}  return config
end

return M
"""
    out_path = root / "generated" / "wezterm.lua"
    write_text_atomic(out_path, lua_src)
    return out_path


def find_wezterm_config_path() -> Path:
    home = Path.home()
    for p in [home / ".wezterm.lua", home / ".config" / "wezterm" / "wezterm.lua"]:
        if p.is_file():
            return p
    return home / ".wezterm.lua"


def hook_wezterm_config(root: Path, generated_lua: Path) -> Path:
    cfg = find_wezterm_config_path()
    if cfg.is_file():
        existing = cfg.read_text(encoding="utf-8")
        if LUA_HOOK_BEGIN not in existing and "kiseki-terminal" not in existing:
            # Safely patch `return config` if present, otherwise only generate module & print instructions
            m = re.search(r"^(\s*return\s+([A-Za-z_][A-Za-z0-9_]*)\s*)$", existing, re.MULTILINE)
            if m:
                var_name = m.group(2)
                snippet = (
                    f"{LUA_HOOK_BEGIN}\n"
                    f"local ok_kiseki, kiseki = pcall(dofile, {lua_str(str(generated_lua.resolve()))})\n"
                    f"if ok_kiseki and kiseki and kiseki.apply then kiseki.apply({var_name}) end\n"
                    f"{LUA_HOOK_END}\n"
                )
                updated = existing[: m.start()] + snippet + existing[m.start() :]
                write_text_atomic(cfg, updated)
    else:
        minimal = (
            "local wezterm = require 'wezterm'\n"
            "local config = wezterm.config_builder and wezterm.config_builder() or {}\n\n"
            f"{LUA_HOOK_BEGIN}\n"
            f"local ok_kiseki, kiseki = pcall(dofile, {lua_str(str(generated_lua.resolve()))})\n"
            f"if ok_kiseki and kiseki and kiseki.apply then kiseki.apply(config) end\n"
            f"{LUA_HOOK_END}\n\n"
            "return config\n"
        )
        write_text_atomic(cfg, minimal)
    # Touch config file so WezTerm's watcher triggers live reload
    if cfg.is_file():
        os.utime(cfg, None)
    return cfg


# ---------------------------------------------------------------------------
# Sync All Configured / Detected Terminals
# ---------------------------------------------------------------------------

def resolve_target_terminals(spec: str) -> list[str]:
    if spec == "none":
        return []
    if spec == "all":
        return ["ghostty", "iterm2", "kitty", "wezterm"]
    if spec == "auto":
        return detect_installed_terminals()
    return [t.strip().lower() for t in spec.split(",") if t.strip()]


def sync_terminals(root: Path, theme_id: str, targets: list[str], hook: bool = False) -> dict[str, Any]:
    results: dict[str, Any] = {"synced": [], "reloaded": [], "notes": []}

    if "ghostty" in targets:
        gconf = generate_ghostty_conf(root, theme_id)
        if hook:
            hook_ghostty_config(root, gconf)
        reloaded, reason = reload_ghostty(root)
        results["synced"].append("ghostty")
        if reloaded:
            results["reloaded"].append("ghostty")
        elif reason != "not_running":
            results["notes"].append(f"Ghostty auto-reload skipped ({reason}); press Cmd+Shift+, to reload.")

    if "iterm2" in targets:
        sync_iterm2(root, theme_id)
        results["synced"].append("iterm2")

    if "kitty" in targets:
        kconf = generate_kitty_conf(root, theme_id)
        hook_and_reload_kitty(root, kconf, hook=hook)
        results["synced"].append("kitty")

    if "wezterm" in targets:
        wlua = generate_wezterm_lua(root, theme_id)
        if hook:
            hook_wezterm_config(root, wlua)
        else:
            wcfg = find_wezterm_config_path()
            if wcfg.is_file() and "kiseki-terminal" in wcfg.read_text(encoding="utf-8"):
                os.utime(wcfg, None)
        results["synced"].append("wezterm")

    return results


# ---------------------------------------------------------------------------
# Shell Environment Emitter (zsh / bash)
# ---------------------------------------------------------------------------

def emit_shell_env(root: Path, theme_id: str, shell: str) -> str:
    theme = load_theme(root, theme_id)
    theme_dir = (root / "themes" / theme_id).resolve()
    fzf = theme["fzf"]
    psr = theme.get("psReadLine", {})

    fzf_opts = " ".join(
        [
            "--height=45%",
            "--layout=reverse",
            f"--border={fzf['border']}",
            f"--prompt={shlex.quote(fzf['prompt'])}",
            f"--pointer={shlex.quote(fzf['pointer'])}",
            f"--marker={shlex.quote(fzf['marker'])}",
            f"--color={fzf['colors']}",
        ]
    )

    lines = [
        f"export KISEKI_ACTIVE_THEME={shlex.quote(theme_id)}",
        f"export KISEKI_ACTIVE_NAME={shlex.quote(theme['name'])}",
        f"export KISEKI_ACTIVE_SUBTITLE={shlex.quote(theme['subtitle'])}",
        f"export KISEKI_FASTFETCH_CONFIG={shlex.quote(str(theme_dir / 'fastfetch.jsonc'))}",
        f"export KISEKI_OMP_CONFIG={shlex.quote(str(theme_dir / 'prompt.omp.json'))}",
        f"export FZF_DEFAULT_OPTS={shlex.quote(fzf_opts)}",
        f"export EZA_COLORS={shlex.quote(theme['ezaColors'])}",
        f"export BAT_THEME={shlex.quote(theme['batTheme'])}",
    ]

    if shell == "zsh" and psr:
        # Map PSReadLine colors -> zsh-syntax-highlighting & zsh-autosuggestions
        lines.append("typeset -gA ZSH_HIGHLIGHT_STYLES")
        zsh_map = {
            "default": psr.get("Default"),
            "command": psr.get("Command"),
            "builtin": psr.get("Command"),
            "alias": psr.get("Command"),
            "function": psr.get("Command"),
            "precommand": psr.get("Keyword"),
            "reserved-word": psr.get("Keyword"),
            "single-hyphen-option": psr.get("Parameter"),
            "double-hyphen-option": psr.get("Parameter"),
            "single-quoted-argument": psr.get("String"),
            "double-quoted-argument": psr.get("String"),
            "dollar-quoted-argument": psr.get("String"),
            "comment": psr.get("Comment"),
            "unknown-token": psr.get("Error"),
            "redirection": psr.get("Operator"),
            "commandseparator": psr.get("Operator"),
            "path": psr.get("Member"),
            "globbing": psr.get("Type"),
        }
        for k, col in zsh_map.items():
            if col:
                lines.append(f"ZSH_HIGHLIGHT_STYLES[{k}]={shlex.quote('fg=' + col)}")
        if psr.get("InlinePrediction"):
            lines.append(f"ZSH_AUTOSUGGEST_HIGHLIGHT_STYLE={shlex.quote('fg=' + psr['InlinePrediction'])}")

    return "\n".join(lines) + "\n"


def emit_osc_palette(root: Path, theme_id: str) -> str:
    """Build standard xterm OSC 4/10/11/12 escape sequences for generic terminals."""
    theme = load_theme(root, theme_id)
    scheme = theme["scheme"]
    seqs = [
        f"\033]10;{scheme['foreground']}\007",
        f"\033]11;{scheme['background']}\007",
        f"\033]12;{scheme['cursorColor']}\007",
    ]
    for idx, key in enumerate(ANSI_KEYS):
        seqs.append(f"\033]4;{idx};{scheme[key]}\007")
    return "".join(seqs)


# ---------------------------------------------------------------------------
# Version & Zero-Lag 24h Self-Updater (parity with scripts/updater.ps1)
# ---------------------------------------------------------------------------

def parse_version(raw: str) -> tuple[int, ...]:
    if not raw:
        return (0, 0, 0)
    clean = re.sub(r"-.*$", "", raw.strip().lstrip("v"))
    parts = []
    for p in clean.split("."):
        try:
            parts.append(int(p))
        except ValueError:
            parts.append(0)
    while len(parts) < 3:
        parts.append(0)
    return tuple(parts)


def get_local_version(root: Path) -> dict[str, Any]:
    vf = root / "version.json"
    if vf.is_file():
        try:
            return json.loads(vf.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            pass
    return {"version": "0.0.0", "tag": "v0.0.0", "releasedAt": "", "changelog": "", "themes": []}


def is_update_check_enabled(root: Path) -> bool:
    if os.environ.get("KISEKI_NO_UPDATE_CHECK") == "1":
        return False
    return read_text_trimmed(root / "update-check.txt", "on").lower() != "off"


def http_get_bytes(url: str, timeout: int = 10, no_cache: bool = False) -> bytes:
    """Prefer curl so macOS system Keychain TLS certs work even with python.org Python."""
    if shutil.which("curl"):
        cmd = ["curl", "-fsSL", "--max-time", str(timeout), "-A", "KisekiTerminalUpdater"]
        if no_cache:
            cmd.extend(["-H", "Cache-Control: no-cache"])
        cmd.append(url)
        res = subprocess.run(cmd, capture_output=True, check=True)
        return res.stdout
    headers = {"User-Agent": "KisekiTerminalUpdater"}
    if no_cache:
        headers["Cache-Control"] = "no-cache"
    req = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return resp.read()


def fetch_remote_manifest(timeout: int = 8) -> dict[str, Any]:
    return json.loads(http_get_bytes(REMOTE_MANIFEST_URL, timeout=timeout, no_cache=True).decode("utf-8"))


def save_update_cache(root: Path, remote: dict[str, Any]) -> dict[str, Any]:
    remote_tag = str(remote.get("tag") or f"v{remote['version']}")
    now_iso = datetime.datetime.now(datetime.timezone.utc).isoformat()
    payload = {
        "checkedAtUtc": now_iso,
        "remoteVersion": str(remote["version"]),
        "remoteTag": remote_tag,
        "changelog": str(remote.get("changelog") or ""),
        "themes": list(remote.get("themes") or []),
        "releaseUrl": f"{RELEASES_URL}/tag/{remote_tag}",
    }
    write_text_atomic(root / "update-cache.json", json.dumps(payload, indent=2, ensure_ascii=False))
    write_text_atomic(root / "update-last-attempt.txt", now_iso)
    return payload


def cmd_update_notice(root: Path) -> int:
    if not is_update_check_enabled(root):
        return 0

    cache_file = root / "update-cache.json"
    attempt_file = root / "update-last-attempt.txt"
    now = datetime.datetime.now(datetime.timezone.utc)

    # 1. Instant cache-only startup notice
    if cache_file.is_file():
        try:
            local = get_local_version(root)
            cache = json.loads(cache_file.read_text(encoding="utf-8"))
            if parse_version(cache.get("remoteVersion", "")) > parse_version(local.get("version", "")):
                print(
                    f"  \033[33m✦ Kiseki Terminal v{cache['remoteVersion']} available "
                    f"(current v{local.get('version', '0.0.0')})\033[0m"
                    f"\033[90m  → run \033[36mtheme update\033[0m"
                )
                if cache.get("changelog"):
                    print(f"    \033[90m{cache['changelog']}\033[0m")
        except (OSError, json.JSONDecodeError):
            pass

    # 2. Decide whether to launch background refresh (15m backoff + 24h TTL)
    if attempt_file.is_file():
        try:
            last_try = datetime.datetime.fromisoformat(read_text_trimmed(attempt_file).replace("Z", "+00:00"))
            if (now - last_try).total_seconds() < 15 * 60:
                return 0
        except ValueError:
            pass

    if cache_file.is_file():
        try:
            cache = json.loads(cache_file.read_text(encoding="utf-8"))
            checked = datetime.datetime.fromisoformat(str(cache.get("checkedAtUtc", "")).replace("Z", "+00:00"))
            if (now - checked).total_seconds() < 24 * 3600:
                return 0
        except (OSError, ValueError, json.JSONDecodeError):
            pass

    try:
        write_text_atomic(attempt_file, now.isoformat())
        subprocess.Popen(
            [sys.executable, str(Path(__file__).resolve()), "update-bg-refresh"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            stdin=subprocess.DEVNULL,
            start_new_session=True,
        )
    except OSError:
        pass
    return 0


def cmd_update_bg_refresh(root: Path) -> int:
    try:
        remote = fetch_remote_manifest(timeout=4)
        if remote and remote.get("version"):
            save_update_cache(root, remote)
    except Exception:
        pass
    return 0


def cmd_update(root: Path, action: str) -> int:
    act = (action or "").strip().lower()
    if act == "mute":
        write_text_atomic(root / "update-check.txt", "off")
        print("\033[36m✦ Auto update check: OFF (run 'theme update check' anytime to check manually)\033[0m")
        return 0
    if act == "unmute":
        write_text_atomic(root / "update-check.txt", "on")
        print("\033[36m✦ Auto update check: ON (checks silently in background every 24h)\033[0m")
        return 0

    local = get_local_version(root)
    local_ver = str(local.get("version", "0.0.0"))
    print(f"\033[36m✦ Checking Kiseki Terminal updates (current: v{local_ver})...\033[0m")

    try:
        remote = fetch_remote_manifest(timeout=10)
    except Exception as exc:
        print(f"\033[31m✖ Could not reach GitHub: {exc}\033[0m")
        return 1

    if not remote or not remote.get("version"):
        print("\033[31m✖ Remote version manifest is invalid.\033[0m")
        return 1

    cache = save_update_cache(root, remote)
    remote_ver = cache["remoteVersion"]
    remote_tag = cache["remoteTag"]
    release_url = cache["releaseUrl"]
    has_update = parse_version(remote_ver) > parse_version(local_ver)

    if act == "check":
        if has_update:
            print(f"\033[33m✦ Update available: v{local_ver} → v{remote_ver}\033[0m")
            if cache["changelog"]:
                print(f"  Notes: {cache['changelog']}")
            print(f"\033[90m  Release: {release_url}\033[0m")
            print("\033[36m  Run 'theme update' to upgrade in-place (keeps your active theme & live mode).\033[0m")
        else:
            print(f"\033[32m✓ Already up to date (v{local_ver}).\033[0m")
            print(f"\033[90m  Release: {release_url}\033[0m")
        return 0

    if not has_update and act not in ("force", "edge"):
        print(f"\033[32m✓ Already up to date (v{local_ver}). Use 'theme update force' to refresh files anyway.\033[0m")
        return 0

    old_themes = set(list_theme_ids(root))
    urls = (
        [f"https://github.com/{REPO_SLUG}/archive/refs/heads/main.zip"]
        if act == "edge"
        else [
            f"https://github.com/{REPO_SLUG}/archive/refs/tags/{remote_tag}.zip",
            f"https://github.com/{REPO_SLUG}/archive/refs/heads/main.zip",
        ]
    )

    with tempfile.TemporaryDirectory(prefix="kiseki-upd-") as tmp:
        zip_path = Path(tmp) / "release.zip"
        downloaded = False
        for u in urls:
            try:
                print(f"\033[90m  Downloading {u} ...\033[0m")
                zip_path.write_bytes(http_get_bytes(u, timeout=30))
                downloaded = True
                break
            except Exception:
                continue
        if not downloaded:
            print("\033[31m✖ Failed to download release archive from GitHub.\033[0m")
            return 1

        with zipfile.ZipFile(zip_path) as zf:
            zf.extractall(tmp)

        dirs = [p for p in Path(tmp).iterdir() if p.is_dir()]
        if not dirs or not (dirs[0] / "themes").is_dir():
            print("\033[31m✖ Downloaded archive does not contain a valid Kiseki Terminal layout.\033[0m")
            return 1

        extracted = dirs[0]
        for sub in ("themes", "shaders", "scripts"):
            src = extracted / sub
            if src.is_dir():
                shutil.copytree(src, root / sub, dirs_exist_ok=True)
        if (extracted / "version.json").is_file():
            shutil.copy2(extracted / "version.json", root / "version.json")

        # Ensure executable bit on scripts
        for script_name in ("kiseki-engine.py", "kiseki.sh"):
            sp = root / "scripts" / script_name
            if sp.is_file():
                sp.chmod(sp.stat().st_mode | 0o755)

    active = get_active_theme_id(root)
    sync_terminals(root, active, detect_installed_terminals(), hook=False)
    # Refresh installed PATH shims if present
    shims_manifest = root / "installed-shims.txt"
    if shims_manifest.is_file():
        lines = [l.strip() for l in shims_manifest.read_text(encoding="utf-8").splitlines() if l.strip()]
        if lines:
            cmd_install_shims(root, str(Path(lines[0]).parent))

    new_themes = sorted(set(list_theme_ids(root)) - old_themes)
    label = f"v{remote_ver} (main)" if act == "edge" else f"v{remote_ver}"
    print(f"\033[32m✦ Updated Kiseki Terminal to {label}!\033[0m")
    if cache["changelog"]:
        print(f"  Changelog: {cache['changelog']}")
    if new_themes:
        print(f"\033[35m  New themes added: {', '.join(new_themes)}  (try: theme {new_themes[0]})\033[0m")
    print(f"\033[90m  Active theme '{active}' refreshed.\033[0m")
    return 0


# ---------------------------------------------------------------------------
# CLI Commands
# ---------------------------------------------------------------------------

SHIM_MARKER = "# kiseki-terminal-shim"
SHIM_COMMANDS = ("themes", "theme", "live", "motion", "crt", "termfont", "termbg", "fetch")


def hex_to_ansi_fg(hex_color: str) -> str:
    h = hex_color.strip().lstrip("#")
    if len(h) != 6:
        return ""
    r, g, b = int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16)
    return f"\033[38;2;{r};{g};{b}m"


def theme_swatch(meta: dict[str, Any]) -> str:
    scheme = meta.get("scheme", {})
    keys = ("cursorColor", "red", "yellow", "green")
    blocks = "".join(f"{hex_to_ansi_fg(scheme.get(k, '#FFFFFF'))}█" for k in keys)
    return f"{blocks}\033[0m"


def mode_badge(root: Path) -> str:
    cur_term = detect_current_terminal()
    if get_crt_mode(root):
        return "CRT SHADER"
    if get_live_mode(root) and cur_term in ("ghostty", "unknown"):
        return "LIVE 60FPS GLSL"
    if get_live_mode(root):
        return "STATIC (Live GPU: Ghostty)"
    return "STATIC"


def cmd_list(root: Path) -> int:
    active = get_active_theme_id(root)
    ver = get_local_version(root).get("version", "")
    ver_tag = f"v{ver}" if ver and ver != "0.0.0" else ""
    mode_tag = mode_badge(root)

    print(
        f"\n\033[36m★ Kiseki Anime Terminal Themes {ver_tag} [{mode_tag}]"
        f"  (switch: theme <id> | motion: live | crt: crt | update: theme update)\033[0m\n"
    )
    for tid in list_theme_ids(root):
        meta = load_theme(root, tid)
        is_act = tid == active
        mark = "●" if is_act else " "
        col = "\033[35m" if is_act else "\033[37m"
        sw = theme_swatch(meta)
        print(f"{col}  {mark} {tid:<16} {sw}  {col}[{meta['style']:<25}] {meta['subtitle']}\033[0m")
    print()
    return 0


def _tty_read_key(fd: int) -> str:
    b = os.read(fd, 1)
    if not b:
        return "EOF"
    ch = b[0]
    if ch in (13, 10):
        return "ENTER"
    if ch == 3:
        return "CTRL_C"
    if ch == 32:
        return "SPACE"
    if ch == 27:
        # Distinguish standalone Esc from ANSI arrow escape sequences (\x1b[A, \x1bOA, etc.)
        rlist, _, _ = select.select([fd], [], [], 0.03)
        if not rlist:
            return "ESC"
        b2 = os.read(fd, 1)
        if b2 in (b"[", b"O"):
            rlist2, _, _ = select.select([fd], [], [], 0.03)
            if rlist2:
                b3 = os.read(fd, 1)
                if b3 == b"A":
                    return "UP"
                if b3 == b"B":
                    return "DOWN"
                if b3 == b"C":
                    return "RIGHT"
                if b3 == b"D":
                    return "LEFT"
        return "ESC"
    try:
        return b.decode("utf-8", errors="ignore")
    except Exception:
        return ""


def _apply_live_preview_tty(root: Path, theme_id: str, tty_fd: int, targets: list[str]) -> None:
    write_text_atomic(root / "current-theme.txt", theme_id)
    sync_terminals(root, theme_id, targets, hook=False)
    cur_term = detect_current_terminal()
    if cur_term == "iterm2":
        meta = load_theme(root, theme_id)
        os.write(tty_fd, f"\033]1337;SetProfile=Kiseki: {meta['name']}\007".encode("utf-8"))
    elif cur_term not in ("ghostty", "kitty", "wezterm") and not os.environ.get("TMUX"):
        os.write(tty_fd, emit_osc_palette(root, theme_id).encode("utf-8"))


def cmd_pick(root: Path, terminals: str) -> int:
    """Interactive arrow-key theme selector rendering to /dev/tty and printing chosen theme_id to stdout."""
    ids = list_theme_ids(root)
    if not ids:
        return 1

    metas = [load_theme(root, tid) for tid in ids]
    initial_active = get_active_theme_id(root)
    active_id = initial_active
    selected_idx = ids.index(initial_active) if initial_active in ids else 0
    previewed_id: str | None = None
    targets = resolve_target_terminals(terminals)

    ver = get_local_version(root).get("version", "")
    ver_tag = f"v{ver}" if ver and ver != "0.0.0" else ""
    mode_tag = mode_badge(root)

    opened_tty_fd: int | None = None
    if sys.stdin.isatty() and sys.stderr.isatty():
        in_fd = sys.stdin.fileno()
        out_fd = sys.stderr.fileno()
    else:
        try:
            opened_tty_fd = os.open("/dev/tty", os.O_RDWR)
            in_fd = opened_tty_fd
            out_fd = opened_tty_fd
        except OSError:
            cmd_list(root)
            return 1

    old_attrs = termios.tcgetattr(in_fd)
    total_lines = len(ids) + 4
    drawn = False
    chosen: str | None = None

    def render() -> None:
        nonlocal drawn
        buf: list[str] = []
        if drawn:
            buf.append(f"\033[{total_lines}A\r")
        lines = [
            f"\033[1;36m★ Kiseki Anime Terminal Themes {ver_tag} [{mode_tag}]\033[0m",
            "\033[90m  ↑/↓ (j/k) 选择  ·  Enter 切换  ·  Space 实时预览  ·  1-9 快捷  ·  q/Esc 退出\033[0m",
            "",
        ]
        for idx, (tid, meta) in enumerate(zip(ids, metas)):
            is_sel = idx == selected_idx
            is_act = tid == active_id
            sw = theme_swatch(meta)
            act_dot = "\033[32m●\033[0m" if is_act else " "
            if is_sel:
                accent = hex_to_ansi_fg(meta["scheme"]["cursorColor"])
                lines.append(
                    f"  {accent}\033[1m❯\033[0m {act_dot} "
                    f"{accent}\033[1m{tid:<16}\033[0m {sw}  "
                    f"\033[1;97m[{meta['style']:<25}]\033[0m \033[97m{meta['subtitle']}\033[0m"
                )
            else:
                lines.append(
                    f"    {act_dot} \033[37m{tid:<16}\033[0m {sw}  "
                    f"\033[90m[{meta['style']:<25}] {meta['subtitle']}\033[0m"
                )
        status = (
            f"  \033[35m✦ 正在预览: {previewed_id}（按 Enter 确认，Esc 恢复 {initial_active}）\033[0m"
            if previewed_id
            else ""
        )
        lines.append(status)

        for line in lines:
            buf.append(f"\033[2K{line}\r\n")
        os.write(out_fd, "".join(buf).encode("utf-8"))
        drawn = True

    try:
        tty.setraw(in_fd)
        os.write(out_fd, b"\033[?25l")
        render()

        while True:
            key = _tty_read_key(in_fd)
            if key in ("UP", "k", "K"):
                selected_idx = (selected_idx - 1) % len(ids)
                render()
            elif key in ("DOWN", "j", "J"):
                selected_idx = (selected_idx + 1) % len(ids)
                render()
            elif key == "SPACE":
                previewed_id = ids[selected_idx]
                active_id = previewed_id
                _apply_live_preview_tty(root, previewed_id, out_fd, targets)
                render()
            elif key == "ENTER":
                chosen = ids[selected_idx]
                break
            elif key in ("ESC", "q", "Q", "CTRL_C", "EOF"):
                if previewed_id and previewed_id != initial_active:
                    _apply_live_preview_tty(root, initial_active, out_fd, targets)
                chosen = None
                break
            elif len(key) == 1 and "1" <= key <= "9":
                num = int(key) - 1
                if num < len(ids):
                    selected_idx = num
                    render()
    finally:
        if drawn:
            os.write(out_fd, f"\033[{total_lines}A\r\033[J".encode("utf-8"))
        os.write(out_fd, b"\033[?25h")
        termios.tcsetattr(in_fd, termios.TCSADRAIN, old_attrs)
        if opened_tty_fd is not None:
            os.close(opened_tty_fd)

    if chosen:
        print(chosen)
        return 0
    return 130


def find_writable_path_bin_dir() -> Path:
    """Pick a directory already on $PATH that is writable (preferring ~/.local/bin or Homebrew)."""
    path_entries = [Path(p).expanduser() for p in os.environ.get("PATH", "").split(os.pathsep) if p]
    preferred = [
        Path.home() / ".local" / "bin",
        Path("/opt/homebrew/bin"),
        Path("/usr/local/bin"),
    ]
    for cand in preferred:
        if cand in path_entries:
            if cand.is_dir() and os.access(cand, os.W_OK):
                return cand
            if cand == Path.home() / ".local" / "bin":
                try:
                    cand.mkdir(parents=True, exist_ok=True)
                    if os.access(cand, os.W_OK):
                        return cand
                except OSError:
                    pass
    for cand in preferred:
        if cand.is_dir() and os.access(cand, os.W_OK):
            return cand
    fallback = Path.home() / ".local" / "bin"
    fallback.mkdir(parents=True, exist_ok=True)
    return fallback


def cmd_install_shims(root: Path, bin_dir_arg: str | None) -> int:
    """Install standalone executable shims on PATH so commands work immediately without `source ~/.zshrc`."""
    bin_dir = Path(bin_dir_arg).expanduser() if bin_dir_arg else find_writable_path_bin_dir()
    bin_dir.mkdir(parents=True, exist_ok=True)
    root_q = shlex.quote(str(root.resolve()))

    templates: dict[str, str] = {
        "themes": f"""#!/usr/bin/env bash
{SHIM_MARKER}
export KISEKI_ROOT="${{KISEKI_ROOT:-{root.resolve()}}}"
ENGINE="$KISEKI_ROOT/scripts/kiseki-engine.py"
if [ "${{1:-}}" = "--list" ] || [ "${{1:-}}" = "-l" ] || [ ! -t 0 ] || [ ! -t 1 ]; then
  exec python3 "$ENGINE" list
fi
CHOSEN="$(python3 "$ENGINE" pick)" || exit 0
[ -n "$CHOSEN" ] && exec "$(dirname "$0")/theme" "$CHOSEN"
""",
        "theme": f"""#!/usr/bin/env bash
{SHIM_MARKER}
export KISEKI_ROOT="${{KISEKI_ROOT:-{root.resolve()}}}"
ENGINE="$KISEKI_ROOT/scripts/kiseki-engine.py"
if [ -z "${{1:-}}" ]; then
  exec "$(dirname "$0")/themes"
fi
if [ "$1" = "update" ]; then
  exec python3 "$ENGINE" update "${{2:-}}"
fi
NAME="$1"
shift
python3 "$ENGINE" switch "$NAME" "$@" || exit $?
if [ "${{TERM_PROGRAM:-}}" = "iTerm.app" ] || [ -n "${{ITERM_SESSION_ID:-}}" ]; then
  ACTIVE_NAME="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["name"])' "$KISEKI_ROOT/themes/$NAME/theme.json" 2>/dev/null || true)"
  [ -n "$ACTIVE_NAME" ] && [ -w /dev/tty ] && printf '\\033]1337;SetProfile=Kiseki: %s\\007' "$ACTIVE_NAME" >/dev/tty 2>/dev/null || true
elif [ -z "${{GHOSTTY_RESOURCES_DIR:-}}" ] && [ "${{TERM_PROGRAM:-}}" != "ghostty" ] \\
  && [ -z "${{KITTY_WINDOW_ID:-}}" ] && [ "${{TERM_PROGRAM:-}}" != "WezTerm" ] \\
  && [ -z "${{TMUX:-}}" ] && [ -w /dev/tty ]; then
  python3 "$ENGINE" osc --theme "$NAME" >/dev/tty 2>/dev/null || true
fi
case " $* " in
  *" --quiet "*|*" -q "*) ;;
  *)
    if command -v fastfetch >/dev/null 2>&1 && [ -f "$KISEKI_ROOT/themes/$NAME/fastfetch.jsonc" ]; then
      fastfetch --config "$KISEKI_ROOT/themes/$NAME/fastfetch.jsonc"
    fi
    ;;
esac
""",
        "live": f"""#!/usr/bin/env bash
{SHIM_MARKER}
export KISEKI_ROOT="${{KISEKI_ROOT:-{root.resolve()}}}"
exec python3 "$KISEKI_ROOT/scripts/kiseki-engine.py" live "${{1:-toggle}}"
""",
        "motion": f"""#!/usr/bin/env bash
{SHIM_MARKER}
export KISEKI_ROOT="${{KISEKI_ROOT:-{root.resolve()}}}"
exec python3 "$KISEKI_ROOT/scripts/kiseki-engine.py" live "${{1:-toggle}}"
""",
        "crt": f"""#!/usr/bin/env bash
{SHIM_MARKER}
export KISEKI_ROOT="${{KISEKI_ROOT:-{root.resolve()}}}"
exec python3 "$KISEKI_ROOT/scripts/kiseki-engine.py" crt
""",
        "termfont": f"""#!/usr/bin/env bash
{SHIM_MARKER}
export KISEKI_ROOT="${{KISEKI_ROOT:-{root.resolve()}}}"
if [ -z "${{1:-}}" ]; then
  echo "Usage: termfont <8bit|code|maple|modern> [size]"
  exit 1
fi
python3 "$KISEKI_ROOT/scripts/kiseki-engine.py" font "$@" || exit $?
if [ "${{TERM_PROGRAM:-}}" = "iTerm.app" ] && [ -w /dev/tty ]; then
  printf '\\033]1337;SetProfile=Kiseki Active\\007' >/dev/tty 2>/dev/null || true
fi
""",
        "termbg": f"""#!/usr/bin/env bash
{SHIM_MARKER}
export KISEKI_ROOT="${{KISEKI_ROOT:-{root.resolve()}}}"
if [ -z "${{1:-}}" ]; then
  echo "Usage: termbg <0.0..1.0 | off>"
  exit 1
fi
python3 "$KISEKI_ROOT/scripts/kiseki-engine.py" bg "$1" || exit $?
if [ "${{TERM_PROGRAM:-}}" = "iTerm.app" ] && [ -w /dev/tty ]; then
  printf '\\033]1337;SetProfile=Kiseki Active\\007' >/dev/tty 2>/dev/null || true
fi
""",
        "fetch": f"""#!/usr/bin/env bash
{SHIM_MARKER}
export KISEKI_ROOT="${{KISEKI_ROOT:-{root.resolve()}}}"
if command -v fastfetch >/dev/null 2>&1; then
  ACTIVE="$(python3 "$KISEKI_ROOT/scripts/kiseki-engine.py" active 2>/dev/null || echo starlight-8bit)"
  CFG="$KISEKI_ROOT/themes/$ACTIVE/fastfetch.jsonc"
  if [ -f "$CFG" ]; then
    exec fastfetch --config "$CFG" "$@"
  fi
  exec fastfetch "$@"
fi
""",
    }
    _ = root_q

    installed: list[str] = []
    skipped: list[str] = []
    for name, body in templates.items():
        dst = bin_dir / name
        if dst.exists() or dst.is_symlink():
            try:
                if SHIM_MARKER not in dst.read_text(encoding="utf-8", errors="ignore"):
                    skipped.append(str(dst))
                    continue
            except OSError:
                skipped.append(str(dst))
                continue
        write_text_atomic(dst, body)
        dst.chmod(0o755)
        installed.append(str(dst))

    if installed:
        write_text_atomic(root / "installed-shims.txt", "\n".join(installed) + "\n")
    print(json.dumps({"bin_dir": str(bin_dir), "installed": installed, "skipped": skipped}))
    return 0


def cmd_switch(root: Path, args: argparse.Namespace) -> int:
    tid = args.theme
    if tid not in list_theme_ids(root):
        print(f"\033[31m✖ Unknown theme '{tid}'. Available themes:\033[0m")
        cmd_list(root)
        return 1

    write_text_atomic(root / "current-theme.txt", tid)
    if args.static:
        write_text_atomic(root / "live-mode.txt", "off")
    # Reset per-session overrides on explicit theme switch (parity with Windows)
    for ov in (root / "font-override.json", root / "bg-opacity.txt"):
        if ov.is_file():
            ov.unlink()
    if args.bg_opacity is not None:
        write_text_atomic(root / "bg-opacity.txt", str(args.bg_opacity))

    targets = resolve_target_terminals(args.terminals)
    sync_res = sync_terminals(root, tid, targets, hook=args.hook)

    if not args.quiet:
        meta = load_theme(root, tid)
        cur_term = detect_current_terminal()
        if get_crt_mode(root):
            mode_tag = "CRT"
        elif get_live_mode(root) and cur_term == "ghostty":
            mode_tag = "LIVE 60FPS GLSL"
        elif get_live_mode(root) and cur_term == "iterm2":
            mode_tag = "STATIC (iTerm2)"
        elif get_live_mode(root):
            mode_tag = "LIVE 60FPS"
        else:
            mode_tag = "STATIC"
        print(f"\033[36m✦ Switched to [{meta['name']}] ({mode_tag}) — {meta['subtitle']}\033[0m")
        for note in sync_res.get("notes", []):
            print(f"\033[33m  ⚠ {note}\033[0m")
    return 0


def cmd_live(root: Path, state: str, terminals: str) -> int:
    current = get_live_mode(root)
    if state == "on":
        nxt = True
    elif state == "off":
        nxt = False
    else:
        nxt = not current

    write_text_atomic(root / "live-mode.txt", "on" if nxt else "off")
    if nxt:
        write_text_atomic(root / "crt-mode.txt", "off")

    active = get_active_theme_id(root)
    sync_res = sync_terminals(root, active, resolve_target_terminals(terminals), hook=False)

    cur_term = detect_current_terminal()
    if nxt:
        if cur_term == "ghostty" or cur_term == "unknown":
            print(f"\033[35m✦ Live Dynamic Background: ON (60fps GLSL GPU shader for '{active}' in Ghostty)\033[0m")
        else:
            print(
                f"\033[35m✦ Live Dynamic Background: ON (enabled for Ghostty '{active}'; "
                f"'{cur_term}' uses static wallpaper)\033[0m"
            )
    else:
        print(f"\033[36m✦ Live Dynamic Background: OFF (static zero-GPU mode for '{active}')\033[0m")
    for note in sync_res.get("notes", []):
        print(f"\033[33m  ⚠ {note}\033[0m")
    return 0


def cmd_crt(root: Path, terminals: str) -> int:
    nxt = not get_crt_mode(root)
    write_text_atomic(root / "crt-mode.txt", "on" if nxt else "off")
    active = get_active_theme_id(root)
    sync_res = sync_terminals(root, active, resolve_target_terminals(terminals), hook=False)
    if nxt:
        print("\033[35m✦ Retro CRT Shader: ON (subtle phosphor bloom + scanlines in Ghostty)\033[0m")
    else:
        print("\033[36m✦ Retro CRT Shader: OFF (restored theme shader mode)\033[0m")
    for note in sync_res.get("notes", []):
        print(f"\033[33m  ⚠ {note}\033[0m")
    return 0


def cmd_font(root: Path, style: str, size: float | None, terminals: str) -> int:
    if style not in FONT_STYLE_MAP:
        print(f"\033[31m✖ Unknown font style '{style}'. Choose: 8bit | code | maple | modern\033[0m")
        return 1
    payload: dict[str, Any] = {"style": style}
    if size is not None:
        payload["size"] = size
    write_text_atomic(root / "font-override.json", json.dumps(payload))
    active = get_active_theme_id(root)
    sync_res = sync_terminals(root, active, resolve_target_terminals(terminals), hook=False)
    size_label = f"{size:g} pt" if size is not None else "theme default size"
    print(f"\033[36m✦ Terminal Font -> {style} ({size_label})\033[0m")
    for note in sync_res.get("notes", []):
        print(f"\033[33m  ⚠ {note}\033[0m")
    return 0


def cmd_bg(root: Path, value: str, terminals: str) -> int:
    val = value.strip().lower()
    active = get_active_theme_id(root)
    if val in ("off", "-off", "--off"):
        write_text_atomic(root / "live-mode.txt", "off")
        write_text_atomic(root / "bg-opacity.txt", "off")
        sync_res = sync_terminals(root, active, resolve_target_terminals(terminals), hook=False)
        print("\033[35m✦ Terminal Background: OFF\033[0m")
        for note in sync_res.get("notes", []):
            print(f"\033[33m  ⚠ {note}\033[0m")
        return 0

    try:
        op = float(val)
        if not (0.0 <= op <= 1.0):
            raise ValueError
    except ValueError:
        print("\033[31m✖ Usage: termbg <0.0..1.0 | off>\033[0m")
        return 1

    write_text_atomic(root / "bg-opacity.txt", f"{op:.2f}")
    sync_res = sync_terminals(root, active, resolve_target_terminals(terminals), hook=False)
    print(f"\033[35m✦ Terminal Background Opacity -> {op:.2f}\033[0m")
    for note in sync_res.get("notes", []):
        print(f"\033[33m  ⚠ {note}\033[0m")
    return 0


def cmd_unhook(root: Path) -> int:
    home = Path.home()
    removed: list[str] = []

    for p in [
        home / "Library" / "Application Support" / "com.mitchellh.ghostty" / "config.ghostty",
        home / "Library" / "Application Support" / "com.mitchellh.ghostty" / "config",
        home / ".config" / "ghostty" / "config.ghostty",
        home / ".config" / "ghostty" / "config",
        home / ".config" / "kitty" / "kitty.conf",
        home / ".zshrc",
        home / ".bashrc",
    ]:
        if remove_hook_block(p, HOOK_BEGIN, HOOK_END):
            removed.append(str(p))

    for p in [home / ".wezterm.lua", home / ".config" / "wezterm" / "wezterm.lua"]:
        if remove_hook_block(p, LUA_HOOK_BEGIN, LUA_HOOK_END):
            removed.append(str(p))

    iterm_dyn = home / "Library" / "Application Support" / "iTerm2" / "DynamicProfiles" / "kiseki-terminal.json"
    if iterm_dyn.is_file():
        iterm_dyn.unlink()
        removed.append(str(iterm_dyn))

    # Remove only shims that contain our marker
    shims_manifest = root / "installed-shims.txt"
    shim_candidates: set[Path] = set()
    if shims_manifest.is_file():
        for line in shims_manifest.read_text(encoding="utf-8").splitlines():
            if line.strip():
                shim_candidates.add(Path(line.strip()))
    for d in (home / ".local" / "bin", Path("/opt/homebrew/bin"), Path("/usr/local/bin")):
        for name in SHIM_COMMANDS:
            shim_candidates.add(d / name)
    for sp in sorted(shim_candidates):
        if sp.is_file():
            try:
                if SHIM_MARKER in sp.read_text(encoding="utf-8", errors="ignore"):
                    sp.unlink()
                    removed.append(str(sp))
            except OSError:
                pass
    if shims_manifest.is_file():
        shims_manifest.unlink()

    reload_ghostty(root)
    reload_kitty()

    if removed:
        print("\033[36m✦ Removed Kiseki Terminal hooks from:\033[0m")
        for r in removed:
            print(f"  - {r}")
    else:
        print("\033[90mNo Kiseki Terminal hooks found.\033[0m")
    return 0


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="kiseki-engine.py")
    sub = p.add_subparsers(dest="cmd", required=True)

    sub.add_parser("list")
    sub.add_parser("list-ids")
    sub.add_parser("active")
    sub.add_parser("detect")
    sub.add_parser("unhook")
    sub.add_parser("update-notice")
    sub.add_parser("update-bg-refresh")

    p_pick = sub.add_parser("pick")
    p_pick.add_argument("--terminals", default="auto")

    p_shims = sub.add_parser("install-shims")
    p_shims.add_argument("--bin-dir", default=None)

    p_sw = sub.add_parser("switch")
    p_sw.add_argument("theme")
    p_sw.add_argument("--static", action="store_true")
    p_sw.add_argument("--quiet", action="store_true")
    p_sw.add_argument("--hook", action="store_true")
    p_sw.add_argument("--bg-opacity", type=float, default=None)
    p_sw.add_argument("--terminals", default="auto")

    p_live = sub.add_parser("live")
    p_live.add_argument("state", nargs="?", default="toggle", choices=["toggle", "on", "off"])
    p_live.add_argument("--terminals", default="auto")

    p_crt = sub.add_parser("crt")
    p_crt.add_argument("--terminals", default="auto")

    p_font = sub.add_parser("font")
    p_font.add_argument("style")
    p_font.add_argument("size", nargs="?", type=float, default=None)
    p_font.add_argument("--terminals", default="auto")

    p_bg = sub.add_parser("bg")
    p_bg.add_argument("value")
    p_bg.add_argument("--terminals", default="auto")

    p_env = sub.add_parser("shell-env")
    p_env.add_argument("--shell", default="zsh", choices=["zsh", "bash"])
    p_env.add_argument("--theme", default=None)

    p_osc = sub.add_parser("osc")
    p_osc.add_argument("--theme", default=None)

    p_upd = sub.add_parser("update")
    p_upd.add_argument("action", nargs="?", default="")

    return p


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    root = get_root()

    if args.cmd == "list":
        return cmd_list(root)
    if args.cmd == "pick":
        return cmd_pick(root, args.terminals)
    if args.cmd == "install-shims":
        return cmd_install_shims(root, args.bin_dir)
    if args.cmd == "list-ids":
        print(" ".join(list_theme_ids(root)))
        return 0
    if args.cmd == "active":
        print(get_active_theme_id(root))
        return 0
    if args.cmd == "detect":
        print(json.dumps({"current": detect_current_terminal(), "installed": detect_installed_terminals()}))
        return 0
    if args.cmd == "switch":
        return cmd_switch(root, args)
    if args.cmd == "live":
        return cmd_live(root, args.state, args.terminals)
    if args.cmd == "crt":
        return cmd_crt(root, args.terminals)
    if args.cmd == "font":
        return cmd_font(root, args.style, args.size, args.terminals)
    if args.cmd == "bg":
        return cmd_bg(root, args.value, args.terminals)
    if args.cmd == "shell-env":
        tid = args.theme or get_active_theme_id(root)
        sys.stdout.write(emit_shell_env(root, tid, args.shell))
        return 0
    if args.cmd == "osc":
        tid = args.theme or get_active_theme_id(root)
        sys.stdout.write(emit_osc_palette(root, tid))
        return 0
    if args.cmd == "update":
        return cmd_update(root, args.action)
    if args.cmd == "update-notice":
        return cmd_update_notice(root)
    if args.cmd == "update-bg-refresh":
        return cmd_update_bg_refresh(root)
    if args.cmd == "unhook":
        return cmd_unhook(root)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
