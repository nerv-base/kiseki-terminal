| scenario | v2 median | v2 p90 | v2 RSS | v1.1.0 median | v1.1.0 p90 | v1.1.0 RSS | speedup |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| first switch after fresh install | 363.00 ms | 363.00 ms | 5.0 MB | 240.22 ms | 240.22 ms | 25.9 MB | ×0.66 |
| version / startup floor | 4.50 ms | 4.86 ms | 2.8 MB | 120.28 ms | 149.90 ms | 25.0 MB | ×26.75 |
| list themes | 5.09 ms | 5.46 ms | 3.3 MB | 112.18 ms | 116.38 ms | 25.0 MB | ×22.06 |
| read active theme | 6.67 ms | 6.88 ms | 4.5 MB | 112.18 ms | 117.73 ms | 24.7 MB | ×16.82 |
| switch theme (ghostty only) | 69.99 ms | 97.17 ms | 5.0 MB | 260.36 ms | 363.15 ms | 26.0 MB | ×3.72 |
| switch + shell env (v1 `theme` cmd) | 71.21 ms | 88.82 ms | 4.9 MB | 346.45 ms | 360.08 ms | 25.9 MB | ×4.86 |
| shell init payload | 4.54 ms | 4.93 ms | 3.1 MB | 121.04 ms | 158.37 ms | 25.0 MB | ×26.68 |
| background opacity | 65.89 ms | 75.59 ms | 5.5 MB | 242.06 ms | 254.10 ms | 25.6 MB | ×3.67 |
| live shader toggle | 56.15 ms | 61.05 ms | 4.8 MB | 252.75 ms | 308.03 ms | 26.0 MB | ×4.50 |

v2 per-prompt precmd hook: **60.0 µs** (v1 has no per-prompt hook; it re-runs the engine on every `theme` call instead)
