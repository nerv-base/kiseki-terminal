# Kiseki Themes Registry (`nerv-base/kiseki-themes`)

Community theme index for **[Kiseki Terminal v2](https://github.com/nerv-base/kiseki-terminal)** (RFC-0006).

> **How it works**: Submit **one** minimal YAML file (`data/themes/<id>.yml`) via PR to list your theme. Once merged, every future release/commit you push to your own repository is automatically probed, validated with `kiseki theme validate`, and published to the Gallery index — **no follow-up PRs required**.

## Install Themes

```bash
# Search the community gallery
kiseki gallery search --sort trending

# Install any listed theme by flat short ID
kiseki theme install <id> && kiseki use <id>

# Update all installed gallery themes
kiseki theme update --all
```

Or press `Tab` inside `kiseki` (interactive TUI) to browse and install live.

## Submit Your Theme

1. Scaffold & validate locally:
   ```bash
   kiseki theme init my-theme --with-shader
   kiseki theme validate ./my-theme
   ```
2. Push your theme pack to a public GitHub repository (add `previews/preview.png` if your theme includes a wallpaper or shader so your storefront card looks great!).
3. Open a PR adding **only** `data/themes/<id>.yml` — see [CONTRIBUTING.md](./CONTRIBUTING.md).

## Listed Themes

<!-- BEGIN THEMES -->
_No community themes listed yet — open the first PR to `data/themes/<id>.yml`!_
<!-- END THEMES -->

## Disclaimer & Takedown

This registry indexes third-party public GitHub repositories. Each theme author is solely responsible for the assets and licenses declared in their repository (`LICENSE` & `CREDITS.md`). Rights holders may request immediate index & thumbnail removal via the [Takedown Issue Template](.github/ISSUE_TEMPLATE/takedown.yml).
