# Credits — Shinkai Rain (`shinkai-rain`)

## Theme definition

| Item | Origin | License |
| :--- | :--- | :--- |
| `manifest.json`, `tokens.json` (L1–L5 palette and semantic tokens) | Original work by the Kiseki Terminal contributors | MIT |
| `shaders/live.glsl`, `shaders/live.hlsl` (parameterized cinematic rain templates) | Original work by the Kiseki Terminal contributors | MIT |
| `assets/logo.txt` (ANSI half-block badge) | Original work by the Kiseki Terminal contributors | MIT |
| `assets/wallpaper.jpg` (left code-safe rain night / right Tokyo alley artwork) | Original work by the Kiseki Terminal contributors | MIT |
| `previews/preview.png` (rendered preview capture) | Original work by the Kiseki Terminal contributors | MIT |

`manifest.json#provenance` declares `wallpaperOrigin: "original"` and
`shaderOrigin: "original"`, matching the table above.

## Inspiration

Original cinematic Tokyo rain-night palette combining neon-aqua `#5CE6D0`,
warm konbini amber `#FFCB77`, and cool raindrop blue `#7FB8FF` on deep wet
indigo `#0C1823`.

## Fonts

This pack **does not** bundle any font binaries. `tokens.json#typography`
references Font Catalog IDs only:

| Font ID | Upstream family | Upstream license |
| :--- | :--- | :--- |
| `maple-mono-nf-cn` | Maple Mono NF CN | SIL Open Font License 1.1 |
| `jetbrains-mono-nf` | JetBrains Mono Nerd Font | SIL Open Font License 1.1 |

## Shader lineage

Both shader templates are written for Kiseki v2's parameterized single-template
pipeline with injected `KISEKI_*` macros, low → high `smoothstep` edge order,
and per-profile Eco / Balanced / Ultra quality tiers.
