# Credits — NERV UNIT-02 (`nerv-eva`)

## Theme definition

| Item | Origin | License |
| :--- | :--- | :--- |
| `manifest.json`, `tokens.json` (L1–L5 palette and semantic tokens) | Original work by the Kiseki Terminal contributors | MIT |
| `shaders/live.glsl`, `shaders/live.hlsl` (parameterized tactical templates) | Original work by the Kiseki Terminal contributors | MIT |
| `assets/logo.txt` (ANSI half-block badge) | Original work by the Kiseki Terminal contributors | MIT |
| `assets/wallpaper.png` (left code-safe blueprint / right Unit-02 artwork) | Original work by the Kiseki Terminal contributors | MIT |
| `previews/preview.png` (rendered preview capture) | Original work by the Kiseki Terminal contributors | MIT |

`manifest.json#provenance` declares `wallpaperOrigin: "original"` and
`shaderOrigin: "original"`, matching the table above.

## Inspiration

The palette direction — MAGI amber `#F06A22`, NERV crimson `#D8262E`, tactical
blue `#62C6F2` — is an original homage to the NERV / MAGI control-room
aesthetic of *Neon Genesis Evangelion*. No frames, stills, logos or other
copyrighted production assets from the series are redistributed in this pack.
The name "NERV UNIT-02" is used descriptively as a fan tribute.

## Fonts

This pack **does not** bundle any font binaries. `tokens.json#typography`
references Font Catalog IDs only:

| Font ID | Upstream family | Upstream license |
| :--- | :--- | :--- |
| `jetbrains-mono-nf` | JetBrains Mono Nerd Font | SIL Open Font License 1.1 |
| `maple-mono-nf-cn` | Maple Mono NF CN | SIL Open Font License 1.1 |

Install them with `kiseki fonts install nerv-eva`; the engine downloads only
from the upstream release URLs pinned in the built-in `font-catalog.json` and
verifies each file's SHA256 before installation.

## Shader lineage

Both shader templates are written from scratch for Kiseki v2's parameterized
single-template pipeline. Every color, opacity and geometry constant now comes
from the injected `KISEKI_*` macro header rather than being duplicated per
quality tier, and every `smoothstep` edge pair is ordered low → high so there is
no undefined behaviour across GPU drivers.
