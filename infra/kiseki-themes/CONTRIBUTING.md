# Contributing to Kiseki Themes Gallery / 投稿指南

Thanks for sharing your theme with the Kiseki community!

> **Never edit `README.md` or `README.zh.md` by hand.** Both are auto-generated from `data/themes/*.yml` after merge so concurrent PRs never conflict.

## Submit a Theme (1 File PR)

Create `data/themes/<id>.yml` where `<id>` is your theme's flat short ID (matching `manifest.json#id`):

```yaml
id: tokyo-midnight
repo: alice/kiseki-tokyo-midnight
subpath: .
track: releases
description:
  en: Deep indigo city-pop palette with subtle live shader glow.
  zh: 深靛蓝都市夜景配色，带柔和动态流光 Shader。
```

### Fields

- `id` (**required**): Kebab-case short ID (`^[a-z0-9][a-z0-9-]{2,47}$`). Must match filename `<id>.yml` and your repo's `manifest.json#id`. Official IDs (`nerv-eva`, `kyoto-sakura`, `shinkai-rain`, `starlight-8bit`) and `kiseki-*` / `official-*` prefixes are reserved.
- `repo` (**required**): Public GitHub repository `owner/repo`.
- `subpath` (optional, default `.`): Subdirectory inside a monorepo containing `manifest.json` + `tokens.json`.
- `track` (optional, default `releases`): `releases` tracks your latest SemVer GitHub Release/Tag (falling back to default branch if no release exists yet); `branch` always tracks your default branch.
- `description.en` / `description.zh` (optional): Short single-line summary (falls back to `manifest.json#subtitle`).

### Automated Gate (No Waiting Period, Loose Content Review)

- **No repository age wait**: newly created repos can be submitted immediately as long as they pass `kiseki theme validate`.
- **Automated security gate**: CI downloads your repository and runs `kiseki theme validate --json` (file whitelist, zero scripts/symlinks, `naga` shader validation across eco/balanced/ultra, WCAG AA contrast gate, mandatory `LICENSE` & `CREDITS.md`).
- **Recommended**: add `previews/preview.png` (terminal screenshot) so the storefront displays your real wallpaper/shader aesthetics. Without it, the Gallery automatically renders a live terminal preview card from your `tokens.json`.
- **Zero-PR updates**: after merge, simply tag a new release (or push to default branch) in your own repo. The scheduled probe automatically validates and publishes the update; if a release fails validation, your previous good version stays live untouched until you push a fix.
