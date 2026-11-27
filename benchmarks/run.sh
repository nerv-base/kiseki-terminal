#!/usr/bin/env bash
# Reproduces the v2 benchmark suite: in-process hot paths, v2-vs-v1 e2e, report.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== building =="
cargo build --release -p kiseki-cli -p kiseki-bench

echo
echo "== in-process micro benchmarks (release) =="
rm -rf target/bench-home
./target/release/kiseki-bench --json benchmarks/results/micro.json \
  | tee benchmarks/results/micro.txt

echo
echo "== end-to-end: v2 vs v1.1.0 (main) =="
python3 benchmarks/compare.py --runs "${RUNS:-15}" --out benchmarks/results

echo
echo "== report =="
python3 benchmarks/report.py
echo
echo "wrote benchmarks/results/REPORT.md and chart.html"
