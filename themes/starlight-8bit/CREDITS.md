# Credits — Starlight 8-Bit (`starlight-8bit`)

## Theme definition

| Item | Origin | License |
| :--- | :--- | :--- |
| `manifest.json`, `tokens.json` (L1–L5 palette and semantic tokens) | Original work by the Kiseki Terminal contributors | MIT |
| `shaders/live.glsl`, `shaders/live.hlsl` (parameterized 8-bit pixel templates) | Original work by the Kiseki Terminal contributors | MIT |
| `assets/logo.txt` (ANSI half-block badge) | Original work by the Kiseki Terminal contributors | MIT |
| `assets/wallpaper.png` (left code-safe starfield / right retro pixel artwork) | Original work by the Kiseki Terminal contributors | MIT |
| `previews/preview.png` (rendered preview capture) | Original work by the Kiseki Terminal contributors | MIT |

`manifest.json#provenance` declares `wallpaperOrigin: "original"` and
`shaderOrigin: "original"`, matching the table above.

## Inspiration

Original retro 8-bit pixel-art night sky palette featuring CRT cyan `#66F0FF`,
starlight pink `#FF82B8`, and pixel violet `#B48EFE` on deep midnight navy
`#0A0F25`.

## Fonts

This pack **does not** bundle any font binaries. `tokens.json#typography`
references Font Catalog IDs only:

| Font ID | Upstream family | Upstream license |
| :--- | :--- | :--- |
| `departure-mono-nf` | DepartureMono Nerd Font | SIL Open Font License 1.1 |
| `fusion-pixel-12-zh` | Fusion Pixel 12px Monospaced zh_hans | SIL Open Font License 1.1 |
| `jetbrains-mono-nf` | JetBrains Mono Nerd Font | SIL Open Font License 1.1 |

## Shader lineage

Both shader templates are written for Kiseki v2's parameterized single-template
pipeline with injected `KISEKI_*` macros, low → high `smoothstep` edge order,
and per-profile Eco / Balanced / Ultra quality tiers.
