# ============================================================
# ★ Kiseki Terminal — macOS / Unix Shell Engine (zsh & bash)
# Supports Ghostty (60fps GLSL + CRT), iTerm2, Kitty, WezTerm,
# and universal shell styling (Oh My Posh, Fastfetch, fzf, eza, bat,
# zoxide, zsh-syntax-highlighting, zsh-autosuggestions).
# ============================================================

# Avoid loading twice in non-interactive shells
case "$-" in
  *i*) ;;
  *) return 0 2>/dev/null || exit 0 ;;
esac

export KISEKI_ROOT="${KISEKI_ROOT:-$HOME/.config/kiseki-terminal}"
_KISEKI_ENGINE="$KISEKI_ROOT/scripts/kiseki-engine.py"

if [ ! -f "$_KISEKI_ENGINE" ]; then
  return 0 2>/dev/null || exit 0
fi

_kiseki_shell_name() {
  if [ -n "${ZSH_VERSION:-}" ]; then
    echo "zsh"
  elif [ -n "${BASH_VERSION:-}" ]; then
    echo "bash"
  else
    echo "sh"
  fi
}

_kiseki_python() {
  if command -v python3 >/dev/null 2>&1; then
    command -v python3
  elif [ -x /usr/bin/python3 ]; then
    echo /usr/bin/python3
  else
    return 1
  fi
}

_kiseki_apply_terminal_runtime() {
  # Emit live escape sequences for terminals that switch profiles/palettes in-band
  local py theme_id
  py="$(_kiseki_python)" || return 0
  theme_id="${1:-${KISEKI_ACTIVE_THEME:-}}"

  if [ "${TERM_PROGRAM:-}" = "iTerm.app" ] || [ -n "${ITERM_SESSION_ID:-}" ]; then
    # Switch active iTerm2 tab immediately via Dynamic Profile name
    if [ -n "${KISEKI_ACTIVE_NAME:-}" ] && [ -w /dev/tty ]; then
      printf '\033]1337;SetProfile=Kiseki: %s\007' "$KISEKI_ACTIVE_NAME" >/dev/tty 2>/dev/null || true
    fi
  elif [ -z "${GHOSTTY_RESOURCES_DIR:-}" ] && [ "${TERM_PROGRAM:-}" != "ghostty" ] \
    && [ -z "${KITTY_WINDOW_ID:-}" ] && [ "${TERM_PROGRAM:-}" != "WezTerm" ] \
    && [ -z "${TMUX:-}" ] && [ -w /dev/tty ]; then
    # Generic fallback (Terminal.app, Alacritty, Warp, etc.): apply OSC 4/10/11/12 colors
    "$py" "$_KISEKI_ENGINE" osc --theme "$theme_id" >/dev/tty 2>/dev/null || true
  fi
}

_kiseki_apply_shell_theme() {
  local py sh_name env_code
  py="$(_kiseki_python)" || return 1
  sh_name="$(_kiseki_shell_name)"

  env_code="$("$py" "$_KISEKI_ENGINE" shell-env --shell "$sh_name" 2>/dev/null)" || return 1
  eval "$env_code"

  # Oh My Posh (rotate POSH_SESSION_ID so OMP never serves stale cached theme)
  if command -v oh-my-posh >/dev/null 2>&1 && [ -f "${KISEKI_OMP_CONFIG:-}" ]; then
    if command -v uuidgen >/dev/null 2>&1; then
      export POSH_SESSION_ID="$(uuidgen)"
    else
      export POSH_SESSION_ID="kiseki-$$-$(date +%s 2>/dev/null || echo 0)"
    fi
    if [ "$sh_name" = "zsh" ]; then
      # If Starship was initialized earlier in ~/.zshrc, detach its hooks so OMP owns the prompt cleanly
      eval 'precmd_functions=(${precmd_functions:#starship_precmd})' 2>/dev/null || true
      eval 'preexec_functions=(${preexec_functions:#starship_preexec})' 2>/dev/null || true
      eval "$(oh-my-posh init zsh --config "$KISEKI_OMP_CONFIG")"
    elif [ "$sh_name" = "bash" ]; then
      eval "$(oh-my-posh init bash --config "$KISEKI_OMP_CONFIG")"
    fi
  fi
}

# --- User Commands (Exact parity with Windows Kiseki Terminal) ---

themes() {
  local py chosen
  py="$(_kiseki_python)" || { echo "kiseki: python3 is required" >&2; return 1; }
  if [ "${1:-}" = "--list" ] || [ "${1:-}" = "-l" ] || [ ! -t 0 ] || [ ! -t 1 ]; then
    "$py" "$_KISEKI_ENGINE" list
    return $?
  fi
  chosen="$("$py" "$_KISEKI_ENGINE" pick)" || return 0
  if [ -n "$chosen" ]; then
    theme "$chosen"
  fi
}

theme() {
  local py name action
  py="$(_kiseki_python)" || { echo "kiseki: python3 is required" >&2; return 1; }
  name="${1:-}"

  if [ -z "$name" ]; then
    themes
    return $?
  fi

  if [ "$name" = "update" ]; then
    action="${2:-}"
    "$py" "$_KISEKI_ENGINE" update "$action" || return $?
    if [ "$action" != "check" ] && [ "$action" != "mute" ] && [ "$action" != "unmute" ]; then
      # Hot-reload updated shell script & re-apply theme
      # shellcheck disable=SC1090
      . "$KISEKI_ROOT/scripts/kiseki.sh"
    fi
    return 0
  fi

  shift
  "$py" "$_KISEKI_ENGINE" switch "$name" "$@" || return $?
  _kiseki_apply_shell_theme
  _kiseki_apply_terminal_runtime "$name"

  # Show Fastfetch status card unless --quiet was passed
  case " $* " in
    *" --quiet "*|*" -q "*) ;;
    *) fetch ;;
  esac
}

live() {
  local py state
  py="$(_kiseki_python)" || { echo "kiseki: python3 is required" >&2; return 1; }
  state="${1:-toggle}"
  "$py" "$_KISEKI_ENGINE" live "$state"
}

motion() {
  live "$@"
}

crt() {
  local py
  py="$(_kiseki_python)" || { echo "kiseki: python3 is required" >&2; return 1; }
  "$py" "$_KISEKI_ENGINE" crt
}

termfont() {
  local py
  py="$(_kiseki_python)" || { echo "kiseki: python3 is required" >&2; return 1; }
  if [ -z "${1:-}" ]; then
    echo "Usage: termfont <8bit|code|maple|modern> [size]"
    return 1
  fi
  "$py" "$_KISEKI_ENGINE" font "$@" || return $?
  if [ "${TERM_PROGRAM:-}" = "iTerm.app" ] && [ -w /dev/tty ]; then
    printf '\033]1337;SetProfile=Kiseki Active\007' >/dev/tty 2>/dev/null || true
  fi
}

termbg() {
  local py
  py="$(_kiseki_python)" || { echo "kiseki: python3 is required" >&2; return 1; }
  if [ -z "${1:-}" ]; then
    echo "Usage: termbg <0.0..1.0 | off>"
    return 1
  fi
  "$py" "$_KISEKI_ENGINE" bg "$1" || return $?
  if [ "${TERM_PROGRAM:-}" = "iTerm.app" ] && [ -w /dev/tty ]; then
    printf '\033]1337;SetProfile=Kiseki Active\007' >/dev/tty 2>/dev/null || true
  fi
}

fetch() {
  if command -v fastfetch >/dev/null 2>&1; then
    local cfg="${KISEKI_FASTFETCH_CONFIG:-$KISEKI_ROOT/themes/starlight-8bit/fastfetch.jsonc}"
    if [ -f "$cfg" ]; then
      fastfetch --config "$cfg" "$@"
    else
      fastfetch "$@"
    fi
  fi
}
alias neofetch='fetch'

# --- Tab Completion (zsh & bash) ---

if [ -n "${ZSH_VERSION:-}" ]; then
  _kiseki_theme_complete() {
    local py
    py="$(_kiseki_python)" || return 0
    if [ "$CURRENT" -eq 2 ]; then
      local -a items
      items=(update ${=$( "$py" "$_KISEKI_ENGINE" list-ids 2>/dev/null )})
      compadd -a items
    elif [ "$CURRENT" -eq 3 ] && [ "${words[2]}" = "update" ]; then
      local -a sub
      sub=(check force edge mute unmute)
      compadd -a sub
    fi
  }
  _kiseki_font_complete() {
    local -a styles
    styles=(8bit code maple modern)
    compadd -a styles
  }
  _kiseki_live_complete() {
    local -a states
    states=(toggle on off)
    compadd -a states
  }
  if (( $+functions[compdef] )); then
    compdef _kiseki_theme_complete theme
    compdef _kiseki_font_complete termfont
    compdef _kiseki_live_complete live motion
  fi
elif [ -n "${BASH_VERSION:-}" ]; then
  _kiseki_theme_bash_complete() {
    local cur prev py
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"
    py="$(_kiseki_python)" || return 0
    if [ "$COMP_CWORD" -eq 1 ]; then
      COMPREPLY=( $(compgen -W "update $("$py" "$_KISEKI_ENGINE" list-ids 2>/dev/null)" -- "$cur") )
    elif [ "$COMP_CWORD" -eq 2 ] && [ "$prev" = "update" ]; then
      COMPREPLY=( $(compgen -W "check force edge mute unmute" -- "$cur") )
    fi
  }
  complete -F _kiseki_theme_bash_complete theme
  complete -W "8bit code maple modern" termfont
  complete -W "toggle on off" live motion
fi

# --- Modern CLI Integrations (Bat, Eza, Zoxide, FZF) ---

export BAT_CONFIG_PATH="${BAT_CONFIG_PATH:-$HOME/.config/bat/config}"
if command -v bat >/dev/null 2>&1 && [ -z "${KISEKI_NO_ALIASES:-}" ]; then
  alias cat='bat'
fi

if command -v eza >/dev/null 2>&1 && [ -z "${KISEKI_NO_ALIASES:-}" ]; then
  alias ls='eza --icons=auto --group-directories-first'
  alias ll='eza -l --icons=auto --group-directories-first --git --time-style=relative'
  alias la='eza -la --icons=auto --group-directories-first --git --time-style=relative'
  alias lt='eza --tree --level=2 --icons=auto --group-directories-first'
fi

if command -v zoxide >/dev/null 2>&1; then
  if [ -n "${ZSH_VERSION:-}" ]; then
    eval "$(zoxide init zsh)"
  elif [ -n "${BASH_VERSION:-}" ]; then
    eval "$(zoxide init bash)"
  fi
fi

# --- Apply Active Theme to Current Shell Session ---

_kiseki_apply_shell_theme
_kiseki_apply_terminal_runtime "${KISEKI_ACTIVE_THEME:-}"

# --- Auto-Greeting & Zero-Lag Update Notice (Top-Level Interactive Terminals) ---

if [ -t 1 ] && [ -z "${NO_FETCH:-}" ] && [ -z "${KISEKI_NO_FETCH:-}" ] \
  && [ -z "${KISEKI_GREETED:-}" ] && [ -z "${VSCODE_INJECTION:-}" ] \
  && [ "${TERM_PROGRAM:-}" != "vscode" ]; then
  export KISEKI_GREETED=1
  if [ "${TERM_PROGRAM:-}" = "ghostty" ] || [ -n "${GHOSTTY_RESOURCES_DIR:-}" ] \
    || [ "${TERM_PROGRAM:-}" = "iTerm.app" ] || [ -n "${ITERM_SESSION_ID:-}" ] \
    || [ -n "${KITTY_WINDOW_ID:-}" ] || [ "${TERM_PROGRAM:-}" = "WezTerm" ] \
    || [ "${TERM_PROGRAM:-}" = "Apple_Terminal" ] || [ "${TERM_PROGRAM:-}" = "WarpTerminal" ]; then
    fetch
    _kiseki_py="$(_kiseki_python)" && "$_kiseki_py" "$_KISEKI_ENGINE" update-notice 2>/dev/null || true
  fi
fi
