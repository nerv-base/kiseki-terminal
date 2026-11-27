# Kiseki Terminal v2 benchmark

Comparison target: **v1.1.0** (`main`, `746ccc6`) — the Python engine v2 replaces.
v2 is measured at `82e3dd1` (after Issues #1–#4).

| | |
| :--- | :--- |
| Machine | Apple M5 (10 cores, 24.0 GB) |
| OS | Darwin 27.3.0 (arm64), macOS 27.3 |
| Toolchains | rustc 1.99.1 (58cf8f9c3 2026-10-13) (Homebrew) · Python 3.13.13 |
| v2 binary | 8.43 MiB single static-ish binary, `opt-level=z` + LTO |
| v1 payload | 102 KiB Python + a `python3` runtime (~25 MB RSS) |
| Date | 2026-11-27T03:55:05Z |

## Method

* **In-process micro benchmarks** (`crates/kiseki-bench`, release, installed from this workspace):
  9 rounds per scenario, adaptive batching (each round ~120 ms of work), per-round cost =
  wall time ÷ calls, reported as the median of the 9 rounds. `KISEKI_HOME` is pinned to a
  scratch directory, so no user configuration is read or written.
* **End-to-end CLI benchmarks** (`benchmarks/compare.py`): both versions run from isolated
  homes against the same `$HOME/.config/ghostty/config` + `~/.zshrc`, restricted to the
  `ghostty` adapter, alternating between `nerv-eva` and `kyoto-sakura` so every run does
  real work. 15 timed runs after 2 warmups; RSS from `/usr/bin/time -l`.
* Both versions therefore pay for the same work: parse the pack, resolve tokens, generate
  the terminal config, mount the host config, write state.

## Headline numbers

| question | v2 | v1.1.0 | ratio |
| :--- | ---: | ---: | ---: |
| process startup (`--version` vs `--help`) | 4.5 ms | 120.3 ms | **×26.75 faster** |
| list themes | 5.1 ms | 112.2 ms | **×22.06 faster** |
| read active theme (`status --json`) | 6.7 ms | 112.2 ms | **×16.82 faster** |
| warm theme switch | 70.0 ms | 260.4 ms | **×3.72 faster** |
| first switch after install | 363.0 ms | 240.2 ms | ×0.66 (slower) |
| `theme <id>` incl. shell re-apply | 71.2 ms | 346.5 ms | **×4.86 faster** |
| peak RSS (switch) | 5.0 MB | 26.0 MB | ×5.2 lighter |
| per-prompt hook | 60.0 µs | n/a (no prompt hook) | — |

## v2 before (`fe5fc82`) vs after (Issues #1–#4)

| scenario | v2 before (`fe5fc82`) | v2 after | speedup | peak RSS |
| :--- | ---: | ---: | ---: | ---: |
| first switch after fresh install | 452.43 ms | **363.00 ms** | **×1.25** | 11.6 MB → 5.0 MB |
| read active theme | 28.69 ms | **6.67 ms** | **×4.30** | 5.2 MB → 4.5 MB |
| switch theme (ghostty only) | 263.32 ms | **69.99 ms** | **×3.76** | 11.5 MB → 5.0 MB |
| switch + shell env (v1 `theme` cmd) | 265.97 ms | **71.21 ms** | **×3.73** | 11.5 MB → 4.9 MB |
| background opacity | 291.26 ms | **65.89 ms** | **×4.42** | 11.6 MB → 5.5 MB |
| live shader toggle | 244.04 ms | **56.15 ms** | **×4.35** | 11.5 MB → 4.8 MB |

* Materialised pack load (`pkg / load (materialised) · nerv-eva`): `7.888 ms` → **618.8 µs** (**×12.75** faster), while uncached 3-profile gate (`pkg / validate pack dir`) remains 8.03 ms.
* In-process batched transactions (`Durability::Batch`): `tx apply (generated only)` = **26.79 ms**, `tx apply (full)` = **33.20 ms**.

## v2 end-to-end vs v1.1.0

| scenario | v2 median | v2 p90 | v2 RSS | v1.1.0 median | v1.1.0 p90 | v1.1.0 RSS | ratio |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| first switch after fresh install | 363.0 ms | 363.0 ms | 5.0 MB | 240.2 ms | 240.2 ms | 25.9 MB | slower ×1.51 |
| version / startup floor | 4.5 ms | 4.9 ms | 2.8 MB | 120.3 ms | 149.9 ms | 25.0 MB | **faster** ×26.75 |
| list themes | 5.1 ms | 5.5 ms | 3.3 MB | 112.2 ms | 116.4 ms | 25.0 MB | **faster** ×22.06 |
| read active theme | 6.7 ms | 6.9 ms | 4.5 MB | 112.2 ms | 117.7 ms | 24.7 MB | **faster** ×16.82 |
| switch theme (ghostty only) | 70.0 ms | 97.2 ms | 5.0 MB | 260.4 ms | 363.2 ms | 26.0 MB | **faster** ×3.72 |
| switch + shell env (v1 `theme` cmd) | 71.2 ms | 88.8 ms | 4.9 MB | 346.5 ms | 360.1 ms | 25.9 MB | **faster** ×4.86 |
| shell init payload | 4.5 ms | 4.9 ms | 3.1 MB | 121.0 ms | 158.4 ms | 25.0 MB | **faster** ×26.68 |
| background opacity | 65.9 ms | 75.6 ms | 5.5 MB | 242.1 ms | 254.1 ms | 25.6 MB | **faster** ×3.67 |
| live shader toggle | 56.2 ms | 61.1 ms | 4.8 MB | 252.8 ms | 308.0 ms | 26.0 MB | **faster** ×4.50 |

## v2 in-process hot paths

| hot path | median | p90 | throughput | cv |
| :--- | ---: | ---: | ---: | ---: |
| ThemeLibrary::open | 352 ns | 464 ns | 2.84M/s | 24.1% |
| library list (4 official) | 28.6 µs | 29.3 µs | 35k/s | 1.3% |
| load_readonly (dry-run) · nerv-eva | 34.7 µs | 35.8 µs | 29k/s | 4.5% |
| load (materialised) · nerv-eva | 618.8 µs | 631.9 µs | 2k/s | 1.6% |
| validate pack dir · nerv-eva | 8.03 ms | 8.14 ms | 125/s | 1.7% |
| L1→L2 balanced · nerv-eva | 2.9 µs | 3.7 µs | 348k/s | 14.0% |
| L1→L2 accessibility · nerv-eva | 87.9 µs | 88.3 µs | 11k/s | 0.4% |
| WCAG contrast report · nerv-eva | 1.3 µs | 1.4 µs | 772k/s | 5.1% |
| GLSL naga validate · nerv-eva | 1.76 ms | 1.85 ms | 567/s | 4.5% |
| HLSL naga validate · nerv-eva | 1.01 ms | 1.03 ms | 990/s | 1.6% |
| artifacts balanced · nerv-eva | 3.02 ms | 3.41 ms | 331/s | 6.2% |
| artifacts static · nerv-eva | 181.7 µs | 185.0 µs | 6k/s | 2.1% |
| registry detect_all (6 adapters) | 241.8 µs | 251.3 µs | 4k/s | 1.9% |
| tx apply (generated only) | 26.79 ms | 32.64 ms | 37/s | 9.4% |
| tx apply (full) | 33.20 ms | 37.21 ms | 30/s | 7.7% |
| State::load | 16.4 µs | 16.5 µs | 61k/s | 0.9% |
| State::read_gen (shell hot path) | 14.8 µs | 17.0 µs | 67k/s | 10.2% |
| State::save | 8.22 ms | 8.34 ms | 122/s | 1.0% |
| write 4 KiB (no fsync) | 62.2 µs | 71.3 µs | 16k/s | 8.1% |
| write 4 KiB (fsync only) | 4.07 ms | 4.69 ms | 245/s | 7.7% |
| durable atomic write (platform) | 8.35 ms | 8.75 ms | 120/s | 2.9% |

## Summary of optimizations (Issues #1–#4)

1. **Batched transaction durability (#1)**: `Durability::Batch` (default) skips backing up regenerable `generated/**` files (recompiled from `state_before` on rollback), replaces per-file `F_FULLFSYNC` (4.03 ms/call; `durable atomic write` 8.35 ms vs 62.2 µs buffered) with single directory `sync_dir` batches, skips rewriting unchanged host configs, and caches `ghostty --version` by binary `(path, len, mtime)` on macOS.
2. **Content-hashed validation cache & active-profile scoping (#2)**: `validate_theme_dir_cached` hashes the pack in a single pass into `<cache>/validation/<sha256>.json`, runtime loads run `naga` only for the active `RenderingProfile` (`ValidationScope::Profile`), and `compile` skips redundant `naga` passes when already validated for that profile; `theme validate`/`install` preserve full uncached 3-profile validation.
3. **Single-pass `status` & fast dependency probing (#3)**: `status` loads, resolves, and compiles the active theme once, deduplicates composition donors, skips `/home/linuxbrew` probes on macOS (`automountd` RPC trap), and caches `--version` / `oh-my-posh config export` results in `<cache>/deps-cache.json`.
4. **Shared font & validation cache across `fonts status` and `doctor` (#4)**: extracted `App::font_inventory` backed by `<cache>/font-cache.json` (with subdir cascade & empty-dir dirty tracking), deduplicated active theme loading via `ThemeLibrary::load_with_validation`, and cached passing `ghostty +validate-config` runs; `--dry-run` remains strictly zero-write across every path.

## Reproduce

```bash
cargo build --release -p kiseki-cli -p kiseki-bench
./target/release/kiseki-bench --json benchmarks/results/micro.json
python3 benchmarks/compare.py --runs 15        # writes benchmarks/results/e2e.json
python3 benchmarks/report.py                  # writes benchmarks/results/REPORT.md
```
