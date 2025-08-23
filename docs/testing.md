# Testing and Benchmarking Guide

Status: Living document. This describes how to run ParXive tests and the black-box harness, how to stress them, and how the planned built-in benchmarking will work.

## Test Layers

- Unit/invariants (cargo test)
  - Encode/verify/repair invariants (zero-padding, interleave + Merkle, multi-stripe repair)
  - Portable, fast; no external tools required
- Black-box harness (`scripts/test_run.sh`)
  - Drives the release/debug CLI end-to-end: create → corrupt → repair → verify
  - Generates datasets, logs to JSONL, writes a summary, preserves artifacts on request
- Stress (parameter grid)
  - Repeat runs across K / parity% / chunk / interleave and scenarios
  - Writes structured logs suitable for aggregation in CI

## Quick Start

Run a small matrix on release binary:
```bash
./scripts/test_run.sh --scenarios small,mixed --runs 1 --build release
```

View latest summary:
```bash
f=$(ls -1dt _tgt/tests/* 2>/dev/null | head -n1)
[ -n "$f" ] && (cat "$f/summary.json" 2>/dev/null || tail -n 100 "$f/results.jsonl")
```

Run a specific tuple and keep artifacts:
```bash
./scripts/test_run.sh --scenario single --build debug --runs 1 \
  --k 8 --pct 50 --chunk 65536 --interleave on --preserve
```

Help:
```bash
./scripts/test_run.sh --help
```

## Scenarios

- `single`: ~400 MiB file
- `small`: ~20 MiB file
- `many-medium`: 50 × 16 MiB files
- `mixed`: mixed small/medium plus a single large file

## Parameters

- `--k` (stripe K): e.g., `4,8,16`
- `--pct` (parity percent): e.g., `25,50`
- `--chunk` (chunk size): e.g., `4096,65536,1048576`
- `--interleave` (on/off): distribute chunks across files per stripe
- `--runs` N: repeat each tuple N times
- `--build` release|debug: which binary to use
- `--preserve`: keep generated datasets and artifacts
- `--out DIR`: output root (default `_tgt/tests`)

## Logging & Artifacts

- Per run JSONL: `_tgt/tests/<ts>/results.jsonl`
  - Fields: timestamp, profile, scenario, k, pct, chunk, interleave, run, ok, stage
- Summary (if `jq` present): `_tgt/tests/<ts>/summary.json`
  - Aggregates passes/fails per (scenario, k, pct, chunk, interleave)
- Artifacts (on failure or when `--preserve`): `_tgt/tests/<ts>/artifacts/`
  - `create-*.stderr`, `repair-*.json`, `repair-*.stderr`

## Defaults & System Friendliness

- CLI runs with background-friendly priority by default (policy applied in the binary)
- Threads default to auto; user can override with `--threads N`

## First-Run Benchmark (Planned)

- On first start, prompt to run a quick benchmark if no prior results exist:
  - Detect CPU/GPU/disk
  - CPU: RS encode and BLAKE3 hashing throughput vs threads
  - Disk: sequential read/write throughput using a temp file in a chosen folder
  - GPU (optional, feature-gated): RS encode throughput per device, basic transfer bandwidth
- Store results in OS-appropriate prefs dir:
  - Windows: `%APPDATA%/ParXive/bench.json`
  - Linux: `$XDG_CONFIG_HOME/ParXive/bench.json` or `~/.config/ParXive/bench.json`
  - macOS: `~/Library/Application Support/ParXive/bench.json`
- Use results to choose default threads, priority/background mode, and (when enabled) GPU device
- Users can rerun: `parx bench [--profile quick|full] [--disk <path>] [--cpu-only|--gpu-only]`

## CI Notes

- CI should run a time-bounded matrix (e.g., `--runs 1` for a small subset) on Windows and Linux
- Upload `results.jsonl` and `summary.json` as artifacts for later inspection

## Troubleshooting

- Missing summary: check `results.jsonl` in the latest `_tgt/tests/<ts>`
- Reproduce a failing tuple with `--runs 1 --preserve` and inspect artifacts
- Enable internal repair debug logs in tests by setting `PARX_INV_DBG=1` (unit tests only)
