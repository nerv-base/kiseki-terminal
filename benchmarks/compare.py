#!/usr/bin/env python3
"""End-to-end CLI benchmark: Kiseki v2 (Rust) vs v1.1.0 (Python engine on `main`).

Both implementations run against isolated homes, in the same terminal scope
(`ghostty` only), doing the same user-visible work. Repeated runs alternate
between two themes so nothing is measured in a "first install" state.

Usage:
    python3 benchmarks/compare.py                 # full run, prints tables
    python3 benchmarks/compare.py --runs 21
    python3 benchmarks/compare.py --out benchmarks/results
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import re
import shutil
import statistics
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
V2_BIN = REPO / "target" / "release" / "kiseki"
V1_TREE = REPO / "target" / "bench" / "v1"
WORK = REPO / "target" / "bench" / "e2e"
GHOSTTY = "/Applications/Ghostty.app/Contents/MacOS"

THEMES = ("nerv-eva", "kyoto-sakura")


# --------------------------------------------------------------------- setup


def prepare() -> None:
    if shutil.which("python3") is None:
        sys.exit("python3 is required to run the v1 reference engine")
    if not V2_BIN.is_file():
        sys.exit(f"missing {V2_BIN}; run `cargo build --release -p kiseki-cli` first")

    if not (V1_TREE / "scripts" / "kiseki-engine.py").is_file():
        V1_TREE.mkdir(parents=True, exist_ok=True)
        subprocess.run(
            "git archive main | tar -x -C " + str(V1_TREE),
            shell=True,
            cwd=REPO,
            check=True,
        )

    for sub in ("home", "v1root", "v2home"):
        shutil.rmtree(WORK / sub, ignore_errors=True)
    home = WORK / "home"
    v1root = WORK / "v1root"
    v2home = WORK / "v2home"

    # Host files both implementations are allowed to mount into.
    (home / ".config" / "ghostty").mkdir(parents=True)
    (home / ".config" / "ghostty" / "config").write_text(
        "# user ghostty config\nfont-size = 13\n", encoding="utf-8"
    )
    (home / ".zshrc").write_text("# user zsh config\n", encoding="utf-8")
    for sub in ("v1root", "v2home"):
        (WORK / sub).mkdir(parents=True, exist_ok=True)

    # v1 reads themes/shaders from its root and writes state next to them.
    for name in ("themes", "shaders", "scripts"):
        os.symlink(V1_TREE / name, v1root / name)
    (v1root / "generated").mkdir()


def reset_runtime(kind: str) -> None:
    """Returns one implementation to a pristine first-run state."""
    home = WORK / "home"
    if kind == "v2":
        shutil.rmtree(WORK / "v2home", ignore_errors=True)
        (WORK / "v2home").mkdir(parents=True)
    else:
        shutil.rmtree(WORK / "v1root" / "generated", ignore_errors=True)
        (WORK / "v1root" / "generated").mkdir()
        for name in ("current-theme.txt", "live-mode.txt", "bg-opacity.txt", "font-override.json"):
            (WORK / "v1root" / name).unlink(missing_ok=True)
    (home / ".config" / "ghostty" / "config").write_text(
        "# user ghostty config\nfont-size = 13\n", encoding="utf-8"
    )
    (home / ".zshrc").write_text("# user zsh config\n", encoding="utf-8")


def cold_first_switch(v2_env: dict[str, str], v1_env: dict[str, str]) -> dict[str, object]:
    """The very first switch on a fresh install (v2 materialises the pack)."""
    row: dict[str, object] = {"label": "first switch after fresh install"}
    for name, command, env in (
        ("v2", [[str(V2_BIN), "use", THEMES[0], "--only", "ghostty", "-q"]], v2_env),
        ("v1", [v1(["switch", THEMES[0], "--terminals", "ghostty", "-q"])], v1_env),
    ):
        sample = Sample()
        for _ in range(3):
            reset_runtime(name)
            sample.add(run_timed(command, env))
        sample.rss_kb = peak_rss_kb(command, env)
        row[name] = {**sample.stats(), "rss_mb": round(sample.rss_kb / 1024, 2)}
    row["speedup"] = row["v1"]["median_ms"] / max(row["v2"]["median_ms"], 0.001)  # type: ignore[index]
    print(
        f"  {row['label']:<34} v2 {row['v2']['median_ms']:8.2f} ms   "  # type: ignore[index]
        f"v1 {row['v1']['median_ms']:8.2f} ms   "  # type: ignore[index]
        f"×{row['speedup']:5.2f}"
    )
    return row


def envs() -> tuple[dict[str, str], dict[str, str]]:
    home = str(WORK / "home")
    base = {
        "HOME": home,
        "PATH": f"{GHOSTTY}:/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin",
        "TERM": "xterm-256color",
        "LANG": "en_US.UTF-8",
        "NO_COLOR": "1",
        "KISEKI_NO_MACOS_DEFAULTS": "1",
    }
    v2 = dict(base, KISEKI_HOME=str(WORK / "v2home"))
    v1 = dict(base, KISEKI_ROOT=str(WORK / "v1root"))
    return v2, v1


def v1(args: list[str]) -> list[str]:
    return ["python3", str(WORK / "v1root" / "scripts" / "kiseki-engine.py"), *args]


# ------------------------------------------------------------------ measuring


@dataclass
class Sample:
    times_ms: list[float] = field(default_factory=list)
    rss_kb: int = 0

    def add(self, ms: float) -> None:
        self.times_ms.append(ms)

    def stats(self) -> dict[str, float]:
        values = sorted(self.times_ms)
        return {
            "n": len(values),
            "median_ms": statistics.median(values),
            "mean_ms": statistics.mean(values),
            "min_ms": values[0],
            "p90_ms": values[max(0, int(0.9 * (len(values) - 1)))],
            "max_ms": values[-1],
        }


RSS_RE = re.compile(r"(\d+)\s+maximum resident set size")


def peak_rss_kb(commands: list[list[str]], env: dict[str, str]) -> int:
    peak = 0
    for command in commands:
        proc = subprocess.run(
            ["/usr/bin/time", "-l", *command], capture_output=True, text=True, env=env
        )
        match = RSS_RE.search(proc.stderr)
        if match:
            peak = max(peak, int(match.group(1)) // 1024)
    return peak


def run_timed(commands: list[list[str]], env: dict[str, str]) -> float:
    """Times one logical operation, which may be several commands (v1 needs a
    second engine call to re-apply the shell env after a switch)."""
    start = time.perf_counter()
    for command in commands:
        proc = subprocess.run(command, capture_output=True, env=env)
        if proc.returncode != 0:
            raise RuntimeError(
                f"{' '.join(command)} failed ({proc.returncode}): "
                f"{proc.stderr.decode('utf-8', 'replace')[:400]}"
            )
    return (time.perf_counter() - start) * 1000.0


def measure(
    label: str,
    v2_cmd: list[list[str]],
    v1_cmd: list[list[str]],
    runs: int,
    v2_env: dict[str, str],
    v1_env: dict[str, str],
    alternate_v2: list[list[list[str]]] | None = None,
    alternate_v1: list[list[list[str]]] | None = None,
) -> dict[str, object]:
    """Times both implementations. `alternate_*` swaps the command per run so
    every iteration does real work (used for theme switching)."""
    out: dict[str, object] = {"label": label}
    cases = (
        ("v2", v2_cmd, v2_env, alternate_v2),
        ("v1", v1_cmd, v1_env, alternate_v1),
    )
    for name, command, env, alternate in cases:
        sample = Sample()
        warmup = 2
        for index in range(runs + warmup):
            cmd = command if alternate is None else alternate[index % len(alternate)]
            elapsed = run_timed(cmd, env)
            if index >= warmup:
                sample.add(elapsed)
        sample.rss_kb = peak_rss_kb(command, env)
        out[name] = {**sample.stats(), "rss_mb": round(sample.rss_kb / 1024, 2)}
    out["speedup"] = out["v1"]["median_ms"] / max(out["v2"]["median_ms"], 0.001)  # type: ignore[index]
    print(
        f"  {label:<34} v2 {out['v2']['median_ms']:8.2f} ms   "  # type: ignore[index]
        f"v1 {out['v1']['median_ms']:8.2f} ms   "  # type: ignore[index]
        f"×{out['speedup']:5.2f}"
    )
    return out


# ------------------------------------------------------------ prompt hot path


def prompt_hot_path(runs: int) -> dict[str, object]:
    """Per-prompt cost of the v2 precmd hook (state.gen read + compare).

    v1 has no per-prompt hook at all, so this only reports v2's added cost.
    """
    v2_env, _ = envs()
    subprocess.run(
        [str(V2_BIN), "use", THEMES[0], "--only", "ghostty", "-q"],
        env=v2_env,
        check=True,
        capture_output=True,
    )
    hook = subprocess.run(
        [str(V2_BIN), "shell-init", "zsh"], env=v2_env, check=True, capture_output=True, text=True
    ).stdout
    script = hook + "\n" + f"for i in {{1..{runs}}}; do _kiseki_sync_hook; done\n"
    start = time.perf_counter()
    subprocess.run(["zsh", "-f", "-c", script], env=v2_env, check=True, capture_output=True)
    total = (time.perf_counter() - start) * 1000.0
    # Subtract zsh's own startup floor.
    floor_start = time.perf_counter()
    subprocess.run(["zsh", "-f", "-c", "true"], env=v2_env, check=True)
    floor = (time.perf_counter() - floor_start) * 1000.0
    per_prompt_us = (total - floor) * 1000.0 / runs
    print(f"  v2 precmd hook ({runs} prompts)             {per_prompt_us:8.2f} µs/prompt")
    return {"runs": runs, "per_prompt_us": per_prompt_us, "total_ms": total, "zsh_floor_ms": floor}


# ------------------------------------------------------------------- tables


def markdown_table(results: list[dict[str, object]], prompt: dict[str, object]) -> str:
    lines = [
        "| scenario | v2 median | v2 p90 | v2 RSS | v1.1.0 median | v1.1.0 p90 | v1.1.0 RSS | speedup |",
        "| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for row in results:
        v2, v1 = row["v2"], row["v1"]  # type: ignore[index]
        lines.append(
            f"| {row['label']} | {v2['median_ms']:.2f} ms | {v2['p90_ms']:.2f} ms | {v2['rss_mb']:.1f} MB "
            f"| {v1['median_ms']:.2f} ms | {v1['p90_ms']:.2f} ms | {v1['rss_mb']:.1f} MB "
            f"| ×{row['speedup']:.2f} |"
        )
    lines.append("")
    lines.append(
        f"v2 per-prompt precmd hook: **{prompt['per_prompt_us']:.1f} µs** "
        "(v1 has no per-prompt hook; it re-runs the engine on every `theme` call instead)"
    )
    return "\n".join(lines)


def collect_meta() -> dict[str, object]:
    def out(command: list[str]) -> str:
        try:
            return subprocess.run(command, capture_output=True, text=True, check=True).stdout.strip()
        except Exception:  # noqa: BLE001
            return "unknown"

    return {
        "date": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "machine": out(["sysctl", "-n", "machdep.cpu.brand_string"]),
        "cpu_cores": out(["sysctl", "-n", "hw.ncpu"]),
        "memory_gb": round(int(out(["sysctl", "-n", "hw.memsize"]) or 0) / 1024**3, 1),
        "os": f"{platform.system()} {platform.release()} ({platform.machine()})",
        "macos": out(["sw_vers", "-productVersion"]),
        "python": out(["python3", "--version"]),
        "rustc": out(["rustc", "--version"]),
        "v2_commit": out(["git", "rev-parse", "--short", "HEAD"]),
        "v1_commit": out(["git", "rev-parse", "--short", "main"]),
        "v2_binary_bytes": V2_BIN.stat().st_size if V2_BIN.is_file() else 0,
        "v1_engine_bytes": (V1_TREE / "scripts" / "kiseki-engine.py").stat().st_size,
    }


# ---------------------------------------------------------------------- main


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--runs", type=int, default=15)
    parser.add_argument("--out", type=Path, default=REPO / "benchmarks" / "results")
    parser.add_argument("--prompt-runs", type=int, default=500)
    args = parser.parse_args()

    prepare()
    v2_env, v1_env = envs()
    print(f"v2: {V2_BIN}\nv1: {V1_TREE}/scripts/kiseki-engine.py\n")

    # Seed both sides so switch benchmarks measure a switch, not a first install.
    for theme in THEMES:
        subprocess.run(
            [str(V2_BIN), "use", theme, "--only", "ghostty", "-q"],
            env=v2_env, check=True, capture_output=True,
        )
        subprocess.run(
            v1(["switch", theme, "--terminals", "ghostty", "-q"]),
            env=v1_env, check=True, capture_output=True,
        )

    results: list[dict[str, object]] = []
    print(f"measuring ({args.runs} runs each, 2 warmups):")

    results.append(cold_first_switch(v2_env, v1_env))
    # Restore the seeded state used by every warm-path benchmark.
    for theme in THEMES:
        subprocess.run(
            [str(V2_BIN), "use", theme, "--only", "ghostty", "-q"],
            env=v2_env, check=True, capture_output=True,
        )
        subprocess.run(
            v1(["switch", theme, "--terminals", "ghostty", "-q"]),
            env=v1_env, check=True, capture_output=True,
        )

    results.append(
        measure(
            "version / startup floor",
            [[str(V2_BIN), "--version"]],
            [v1(["--help"])],
            args.runs, v2_env, v1_env,
        )
    )
    results.append(
        measure(
            "list themes",
            [[str(V2_BIN), "theme", "list", "--json"]],
            [v1(["list"])],
            args.runs, v2_env, v1_env,
        )
    )
    results.append(
        measure(
            "read active theme",
            [[str(V2_BIN), "status", "--json"]],
            [v1(["active"])],
            args.runs, v2_env, v1_env,
        )
    )
    switch_v2 = [
        [[str(V2_BIN), "use", THEMES[i % 2], "--only", "ghostty", "-q"]] for i in range(2)
    ]
    switch_v1 = [
        [v1(["switch", THEMES[i % 2], "--terminals", "ghostty", "-q"])] for i in range(2)
    ]
    results.append(
        measure(
            "switch theme (ghostty only)",
            switch_v2[0],
            switch_v1[0],
            args.runs, v2_env, v1_env,
            alternate_v2=switch_v2,
            alternate_v1=switch_v1,
        )
    )
    # What typing `theme <id>` actually costs in v1: switch + re-apply shell env.
    parity_v1 = [
        [v1(["switch", THEMES[i % 2], "--terminals", "ghostty", "-q"]), v1(["shell-env", "--shell", "zsh"])]
        for i in range(2)
    ]
    results.append(
        measure(
            "switch + shell env (v1 `theme` cmd)",
            switch_v2[0],
            parity_v1[0],
            args.runs, v2_env, v1_env,
            alternate_v2=switch_v2,
            alternate_v1=parity_v1,
        )
    )
    results.append(
        measure(
            "shell init payload",
            [[str(V2_BIN), "shell-init", "zsh"]],
            [v1(["shell-env", "--shell", "zsh"])],
            args.runs, v2_env, v1_env,
        )
    )
    results.append(
        measure(
            "background opacity",
            [[str(V2_BIN), "bg", "0.85", "-q"]],
            [v1(["bg", "0.85", "--terminals", "ghostty"])],
            args.runs, v2_env, v1_env,
        )
    )
    results.append(
        measure(
            "live shader toggle",
            [[str(V2_BIN), "live", "off", "-q"]],
            [v1(["live", "off", "--terminals", "ghostty"])],
            args.runs, v2_env, v1_env,
        )
    )

    prompt = prompt_hot_path(args.prompt_runs)
    meta = collect_meta()

    table = markdown_table(results, prompt)
    print()
    print(table)

    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "e2e.json").write_text(
        json.dumps({"meta": meta, "results": results, "prompt_hook": prompt}, indent=2)
    )
    (args.out / "e2e.md").write_text(table + "\n")
    print(f"\nwrote {args.out}/e2e.json and e2e.md")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
