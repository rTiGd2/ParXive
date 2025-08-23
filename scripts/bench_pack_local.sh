#!/usr/bin/env bash
set -euo pipefail

# Package local-only benchmark tools into a tarball (no binaries).

OUTDIR="${1:-_tgt/benchpkg}"
STAMP=$(date +%Y%m%d-%H%M%S)
PKG="$OUTDIR/ParXive-bench-$STAMP.tar.gz"

mkdir -p "$OUTDIR"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/scripts" "$tmp/docs"
cp -v scripts/bench_repair_smoke.sh "$tmp/scripts/" >/dev/null
cp -v scripts/bench_matrix_local.sh "$tmp/scripts/" >/dev/null
cp -v scripts/bench_env_info.sh "$tmp/scripts/" >/dev/null
cp -v scripts/bench_to_html.py "$tmp/scripts/" >/dev/null
cp -v docs/benchmarks.md "$tmp/docs/" >/dev/null

cat >"$tmp/README.txt" <<'TXT'
ParXive Local Benchmark Package
===============================

This package contains local-only scripts to generate datasets, run encode/repair benchmarks,
and verify results using deterministic dataset hashes.

Requirements: Rust toolchain, cargo, jq, Python 3, coreutils; optional: nvidia-smi for GPU info.

Scripts:
- scripts/bench_repair_smoke.sh  — quick smoke test
- scripts/bench_matrix_local.sh  — multi-scenario/matrix runner (writes JSONL results)
- scripts/bench_env_info.sh      — emits hardware/runtime metadata as JSON
- scripts/bench_to_html.py       — convert JSONL to HTML table summary

Usage:
- ./scripts/bench_repair_smoke.sh
- ./scripts/bench_matrix_local.sh
- python3 scripts/bench_to_html.py _tgt/bench-results/bench-<stamp>.jsonl report.html

Note: these scripts refuse to run in CI (CI/GITHUB_ACTIONS guards).
TXT

(cd "$tmp" && tar -czf "$PKG" .)
echo "Created: $PKG"

