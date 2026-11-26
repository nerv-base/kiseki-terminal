# Credits — Kyoto Sakura (`kyoto-sakura`)

## Theme definition

| Item | Origin | License |
| :--- | :--- | :--- |
| `manifest.json`, `tokens.json` (L1–L5 palette and semantic tokens) | Original work by the Kiseki Terminal contributors | MIT |
| `shaders/live.glsl`, `shaders/live.hlsl` (parameterized fantasy templates) | Original work by the Kiseki Terminal contributors | MIT |
| `assets/logo.txt` (ANSI half-block badge) | Original work by the Kiseki Terminal contributors | MIT |
| `assets/wallpaper.png` (left code-safe night sky / right Kyoto shrine sakura artwork) | Original work by the Kiseki Terminal contributors | MIT |
| `previews/preview.png` (rendered preview capture) | Original work by the Kiseki Terminal contributors | MIT |

`manifest.json#provenance` declares `wallpaperOrigin: "original"` and
`shaderOrigin: "original"`, matching the table above.

## Inspiration

Original Japanese night-sakura palette built around blossom pink `#FF9EC7`,
stone-lantern gold `#FFC971`, and temple wisteria `#C4A7FF` on deep midnight
plum `#0F0B19`.

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
