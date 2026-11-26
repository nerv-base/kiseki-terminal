# kiseki-terminal

**Kiseki Terminal v2** — Universal anime-aesthetic terminal theming engine shipped as a zero-dependency Rust single binary.

- 4 bundled official themes offline out of the box (`nerv-eva`, `kyoto-sakura`, `shinkai-rain`, `starlight-8bit`)
- 5-layer semantic tokens + layer composition (`kiseki compose`)
- `naga`-validated GLSL / HLSL dynamic shaders + WCAG 2.1 contrast gates
- Non-invasive mounts (Ghostty `SIGUSR2` hot reload, lossless JSONC Windows Terminal patch, Zsh / PowerShell zero-fork sync)
- Transaction journal with instant `kiseki rollback` and `kiseki recover --safe`

## Install

```bash
# npm
npm install -g kiseki-terminal

# bun
bun add -g kiseki-terminal

# pnpm
pnpm add -g kiseki-terminal

# Run without installing globally
npx kiseki-terminal init
bunx kiseki-terminal init
```

After global installation, both `kiseki` and `kiseki-terminal` commands are available:

```bash
kiseki init
kiseki pick
kiseki use starlight-8bit
```

Platform native binaries are delivered via `optionalDependencies` (`darwin-arm64`, `darwin-x64`, `linux-arm64`, `linux-x64`, `win32-arm64`, `win32-x64`) with **zero `postinstall` scripts**.

## Links

- Documentation: https://kisekiterm.dev
- Source & Issues: https://github.com/nerv-base/kiseki-terminal
- License: MIT
