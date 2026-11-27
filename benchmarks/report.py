#!/usr/bin/env python3
"""Turns `benchmarks/results/{micro,e2e}.json` into `benchmarks/results/REPORT.md`."""

from __future__ import annotations

import json
from pathlib import Path

RESULTS = Path(__file__).resolve().parent / "results"

GROUPS = {
    "parse": "L1 pack parsing (serde_json)",
    "resolve": "L1→L2 resolution + WCAG",
    "shader": "Shader compile + naga validation",
    "compile": "Artifact compilation (pure function)",
    "adapters": "Adapter detection / planning",
    "tx": "Transaction engine (durable batch)",
    "pkg": "Theme library (disk + validation)",
    "state": "State / config / env",
    "io": "Durable-write floor (this machine)",
}

HOT_PATHS = [
    ("ThemeLibrary::open", "pkg"),
    ("library list (4 official)", "pkg"),
    ("load_readonly (dry-run) · nerv-eva", "pkg"),
    ("load (materialised) · nerv-eva", "pkg"),
    ("validate pack dir · nerv-eva", "pkg"),
    ("L1→L2 balanced · nerv-eva", "resolve"),
    ("L1→L2 accessibility · nerv-eva", "resolve"),
    ("WCAG contrast report · nerv-eva", "resolve"),
    ("GLSL naga validate · nerv-eva", "shader"),
    ("HLSL naga validate · nerv-eva", "shader"),
    ("artifacts balanced · nerv-eva", "compile"),
    ("artifacts static · nerv-eva", "compile"),
    ("registry detect_all (6 adapters)", "adapters"),
    ("tx apply (generated only)", "tx"),
    ("tx apply (full)", "tx"),
    ("State::load", "state"),
    ("State::read_gen (shell hot path)", "state"),
    ("State::save", "state"),
    ("write 4 KiB (no fsync)", "io"),
    ("write 4 KiB (fsync only)", "io"),
    ("durable atomic write (platform)", "io"),
]

# Recorded v2 baseline before Issues #1-#4 (commit fe5fc82 on the same Apple M5 host).
BASELINE_COMMIT = "fe5fc82"
BASELINE_E2E = {
    "first switch after fresh install": (452.43, 11.59),
    "read active theme": (28.69, 5.19),
    "switch theme (ghostty only)": (263.32, 11.50),
    "switch + shell env (v1 `theme` cmd)": (265.97, 11.53),
    "background opacity": (291.26, 11.55),
    "live shader toggle": (244.04, 11.50),
}
BASELINE_LOAD_MS = 7.888


def human_ns(ns: float) -> str:
    if ns >= 1_000_000:
        return f"{ns / 1e6:.2f} ms"
    if ns >= 1_000:
        return f"{ns / 1e3:.1f} µs"
    return f"{ns:.0f} ns"


def ops(ns: float) -> str:
    value = 1e9 / max(ns, 1.0)
    if value >= 1_000_000:
        return f"{value / 1e6:.2f}M/s"
    if value >= 1_000:
        return f"{value / 1e3:.0f}k/s"
    return f"{value:.0f}/s"


def headline_ratio(speedup: float) -> str:
    if speedup >= 1.0:
        return f"**×{speedup:.2f} faster**"
    return f"×{speedup:.2f} (slower)"


def micro_table(micro: dict) -> str:
    rows = {(r["scenario"]): r for r in micro["results"]}
    lines = ["| hot path | median | p90 | throughput | cv |", "| :--- | ---: | ---: | ---: | ---: |"]
    for name, group in HOT_PATHS:
        row = rows.get(name)
        if row is None or row["group"] != group:
            continue
        lines.append(
            f"| {name} | {human_ns(row['median_ns'])} | {human_ns(row['p90_ns'])} "
            f"| {ops(row['median_ns'])} | {row['cv_pct']:.1f}% |"
        )
    return "\n".join(lines)


def group_table(micro: dict) -> str:
    by_group: dict[str, list[dict]] = {}
    for row in micro["results"]:
        by_group.setdefault(row["group"], []).append(row)
    lines = ["| group | scenarios | median range | slowest |", "| :--- | ---: | ---: | :--- |"]
    for group, title in GROUPS.items():
        rows = by_group.get(group, [])
        if not rows:
            continue
        medians = [r["median_ns"] for r in rows]
        slowest = max(rows, key=lambda r: r["median_ns"])
        lines.append(
            f"| {title} | {len(rows)} | {human_ns(min(medians))} – {human_ns(max(medians))} "
            f"| {slowest['scenario']} ({human_ns(slowest['median_ns'])}) |"
        )
    return "\n".join(lines)


def e2e_table(e2e: dict) -> str:
    lines = [
        "| scenario | v2 median | v2 p90 | v2 RSS | v1.1.0 median | v1.1.0 p90 | v1.1.0 RSS | ratio |",
        "| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for row in e2e["results"]:
        v2, v1 = row["v2"], row["v1"]
        ratio = row["speedup"]
        label = "**faster** ×{:.2f}".format(ratio) if ratio >= 1 else "slower ×{:.2f}".format(1 / ratio)
        lines.append(
            f"| {row['label']} | {v2['median_ms']:.1f} ms | {v2['p90_ms']:.1f} ms | {v2['rss_mb']:.1f} MB "
            f"| {v1['median_ms']:.1f} ms | {v1['p90_ms']:.1f} ms | {v1['rss_mb']:.1f} MB | {label} |"
        )
    return "\n".join(lines)


def before_after_table(e2e: dict) -> str:
    rows = {r["label"]: r for r in e2e["results"]}
    lines = [
        f"| scenario | v2 before (`{BASELINE_COMMIT}`) | v2 after | speedup | peak RSS |",
        "| :--- | ---: | ---: | ---: | ---: |",
    ]
    for label, (before_ms, before_rss) in BASELINE_E2E.items():
        row = rows.get(label)
        if row is None:
            continue
        after_ms = row["v2"]["median_ms"]
        after_rss = row["v2"]["rss_mb"]
        gain = before_ms / max(after_ms, 0.001)
        lines.append(
            f"| {label} | {before_ms:.2f} ms | **{after_ms:.2f} ms** | **×{gain:.2f}** "
            f"| {before_rss:.1f} MB → {after_rss:.1f} MB |"
        )
    return "\n".join(lines)


def chart_html(e2e: dict, micro: dict) -> str:
    del micro
    rows = e2e["results"]
    max_ms = max(max(r["v2"]["median_ms"], r["v1"]["median_ms"]) for r in rows)
    bars = []
    for row in rows:
        v2, v1 = row["v2"]["median_ms"], row["v1"]["median_ms"]
        bars.append(
            f'<div class="row"><div class="name">{row["label"]}</div>'
            f'<div class="track"><div class="bar v2" style="width:{v2 / max_ms * 100:.1f}%">'
            f'<span>{v2:.1f}</span></div></div>'
            f'<div class="track"><div class="bar v1" style="width:{v1 / max_ms * 100:.1f}%">'
            f'<span>{v1:.1f}</span></div></div></div>'
        )
    e2e_by_label = {r["label"]: r for r in rows}
    max_before = max(ms for ms, _ in BASELINE_E2E.values())
    ba_bars = []
    for label, (before_ms, _) in BASELINE_E2E.items():
        row = e2e_by_label.get(label)
        if row is None:
            continue
        after_ms = row["v2"]["median_ms"]
        ba_bars.append(
            f'<div class="row"><div class="name">{label}</div>'
            f'<div class="track"><div class="bar v2" style="width:{after_ms / max_before * 100:.1f}%">'
            f'<span>{after_ms:.1f}</span></div></div>'
            f'<div class="track"><div class="bar base" style="width:{before_ms / max_before * 100:.1f}%">'
            f'<span>{before_ms:.1f}</span></div></div></div>'
        )
    return f"""<!doctype html><html><head><meta charset="utf-8"><style>
:root {{ color-scheme: light dark; }}
* {{ box-sizing: border-box; }}
body {{ margin: 0; font-family: var(--font-sans, ui-sans-serif, system-ui); color: var(--foreground); }}
h2 {{ font-size: 15px; margin: 0 0 4px; }}
p {{ margin: 0 0 14px; font-size: 12px; color: var(--muted); }}
.legend {{ display: flex; gap: 14px; font-size: 12px; margin: 0 0 10px; align-items: center; }}
.dot {{ width: 9px; height: 9px; border-radius: 3px; display: inline-block; margin-right: 5px; }}
.v2 {{ background: var(--chart-1, #4ba3ff); }}
.v1 {{ background: var(--chart-2, #f59f00); }}
.base {{ background: var(--border, #868e96); }}
.row {{ display: grid; grid-template-columns: 190px 1fr; grid-template-rows: 12px 12px;
        row-gap: 3px; align-items: center; margin-bottom: 11px; }}
.name {{ grid-row: 1 / span 2; font-size: 11.5px; line-height: 1.25; padding-right: 10px; }}
.track {{ background: var(--secondary, rgba(127,127,127,.13)); border-radius: 5px; height: 12px; overflow: hidden; }}
.bar {{ height: 100%; border-radius: 5px; position: relative; min-width: 2px; }}
.bar span {{ position: absolute; right: 5px; top: -1px; font-size: 9.5px; line-height: 13px;
            color: #fff; text-shadow: 0 0 3px rgba(0,0,0,.5); }}
</style></head><body>
<h2>v2 vs v1.1.0 · median wall time per command (ms)</h2>
<p>Same host, same isolated config, ghostty adapter only.</p>
<div class="legend"><span><i class="dot v2"></i>v2 (Rust)</span><span><i class="dot v1"></i>v1.1.0 (Python)</span></div>
{''.join(bars)}

<h2 style="margin-top:22px">v2 optimization impact · before ({BASELINE_COMMIT}) vs after (ms)</h2>
<p>Batch durability, SHA-256 validation cache, single-pass status, and host tool/font caches (Issues #1–#4).</p>
<div class="legend"><span><i class="dot v2"></i>v2 after (#1–#4)</span><span><i class="dot base"></i>v2 before ({BASELINE_COMMIT})</span></div>
{''.join(ba_bars)}
</body></html>"""


def main() -> int:
    micro = json.loads((RESULTS / "micro.json").read_text())
    e2e = json.loads((RESULTS / "e2e.json").read_text())
    meta = e2e["meta"]
    prompt = e2e["prompt_hook"]

    micro_rows = {r["scenario"]: r for r in micro["results"]}
    durable_ns = micro_rows["durable atomic write (platform)"]["median_ns"]
    plain_ns = micro_rows["write 4 KiB (no fsync)"]["median_ns"]
    sync_ns = micro_rows["write 4 KiB (sync_all, F_FULLFSYNC)"]["median_ns"]
    validate_ns = micro_rows["validate pack dir · nerv-eva"]["median_ns"]
    load_ns = micro_rows["load (materialised) · nerv-eva"]["median_ns"]
    tx_gen_ns = micro_rows.get("tx apply (generated only)", {}).get("median_ns", 0.0)
    tx_full_ns = micro_rows.get("tx apply (full)", {}).get("median_ns", 0.0)

    rows = {r["label"]: r for r in e2e["results"]}
    startup = rows["version / startup floor"]
    listing = rows["list themes"]
    warm = rows["switch theme (ghostty only)"]
    cold = rows["first switch after fresh install"]
    parity = rows["switch + shell env (v1 `theme` cmd)"]
    status_row = rows["read active theme"]
    load_speedup = (BASELINE_LOAD_MS * 1_000_000.0) / max(load_ns, 1.0)

    report = f"""# Kiseki Terminal v2 benchmark

Comparison target: **v1.1.0** (`main`, `{meta['v1_commit']}`) — the Python engine v2 replaces.
v2 is measured at `{meta['v2_commit']}` (after Issues #1–#4).

| | |
| :--- | :--- |
| Machine | {meta['machine']} ({meta['cpu_cores']} cores, {meta['memory_gb']} GB) |
| OS | {meta['os']}, macOS {meta['macos']} |
| Toolchains | {meta['rustc']} · {meta['python']} |
| v2 binary | {meta['v2_binary_bytes'] / 1024 / 1024:.2f} MiB single static-ish binary, `opt-level=z` + LTO |
| v1 payload | {meta['v1_engine_bytes'] / 1024:.0f} KiB Python + a `python3` runtime (~25 MB RSS) |
| Date | {meta['date']} |

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
| process startup (`--version` vs `--help`) | {startup['v2']['median_ms']:.1f} ms | {startup['v1']['median_ms']:.1f} ms | {headline_ratio(startup['speedup'])} |
| list themes | {listing['v2']['median_ms']:.1f} ms | {listing['v1']['median_ms']:.1f} ms | {headline_ratio(listing['speedup'])} |
| read active theme (`status --json`) | {status_row['v2']['median_ms']:.1f} ms | {status_row['v1']['median_ms']:.1f} ms | {headline_ratio(status_row['speedup'])} |
| warm theme switch | {warm['v2']['median_ms']:.1f} ms | {warm['v1']['median_ms']:.1f} ms | {headline_ratio(warm['speedup'])} |
| first switch after install | {cold['v2']['median_ms']:.1f} ms | {cold['v1']['median_ms']:.1f} ms | {headline_ratio(cold['speedup'])} |
| `theme <id>` incl. shell re-apply | {parity['v2']['median_ms']:.1f} ms | {parity['v1']['median_ms']:.1f} ms | {headline_ratio(parity['speedup'])} |
| peak RSS (switch) | {warm['v2']['rss_mb']:.1f} MB | {warm['v1']['rss_mb']:.1f} MB | ×{warm['v1']['rss_mb'] / warm['v2']['rss_mb']:.1f} lighter |
| per-prompt hook | {prompt['per_prompt_us']:.1f} µs | n/a (no prompt hook) | — |

## v2 before (`{BASELINE_COMMIT}`) vs after (Issues #1–#4)

{before_after_table(e2e)}

* Materialised pack load (`pkg / load (materialised) · nerv-eva`): `{BASELINE_LOAD_MS:.3f} ms` → **{human_ns(load_ns)}** (**×{load_speedup:.2f}** faster), while uncached 3-profile gate (`pkg / validate pack dir`) remains {human_ns(validate_ns)}.
* In-process batched transactions (`Durability::Batch`): `tx apply (generated only)` = **{human_ns(tx_gen_ns)}**, `tx apply (full)` = **{human_ns(tx_full_ns)}**.

## v2 end-to-end vs v1.1.0

{e2e_table(e2e)}

## v2 in-process hot paths

{micro_table(micro)}

## Summary of optimizations (Issues #1–#4)

1. **Batched transaction durability (#1)**: `Durability::Batch` (default) skips backing up regenerable `generated/**` files (recompiled from `state_before` on rollback), replaces per-file `F_FULLFSYNC` ({human_ns(sync_ns)}/call; `durable atomic write` {human_ns(durable_ns)} vs {human_ns(plain_ns)} buffered) with single directory `sync_dir` batches, skips rewriting unchanged host configs, and caches `ghostty --version` by binary `(path, len, mtime)` on macOS.
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
"""
    (RESULTS / "REPORT.md").write_text(report)
    (RESULTS / "chart.html").write_text(chart_html(e2e, micro))
    print(f"wrote {RESULTS / 'REPORT.md'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
