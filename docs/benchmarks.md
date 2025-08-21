# Benchmark Plan

Objective: Provide a fair, repeatable, and unbiased performance and capability comparison between ParXive and widely used PAR2 implementations. When our Rust + GPU par1/par2 library is available, include it as an additional subject and also compare our par2 vs ParXive.

Principles
- Reproducible: fully scripted runs with fixed seeds and pinned versions.
- Representative: datasets spanning single large files and many small files; various chunk sizes and stripe-K.
- Unbiased: pre-register hypotheses, publish raw data and code; use identical hardware and isolation for each run.
- Transparent: report methodology, environment, inputs, and full results; include error bars and confidence intervals where applicable.

Datasets
- Single large file (e.g., 20 GB);
- Many medium files (e.g., 20 × 1 GB);
- Many small files (e.g., 100k files totaling ~20 GB);
- Mixed file tree with nested directories and sparse files (where supported).

Metrics
- Encode throughput (MiB/s), wall-clock time, CPU %, memory peak;
- Verify throughput and detection coverage;
- Repair success rate vs. loss patterns (random chunk loss, full-file loss, volume loss);
- Parity overhead vs. configuration (K, parity%).

Outputs
- Human-friendly HTML report (with charts and narrative) and a PDF version.
- Machine-readable CSV/JSON raw results for third-party analysis.

Tooling
- `bench/` scripts to generate datasets, run tools, and collect metrics.
- Optional: use `hyperfine` for timing, `perf` or `pidstat` for CPU/mem samples.
- Report generator (Python or Rust) to render HTML (and PDF via wkhtmltopdf/Pandoc).

Anti-bias measures
- Include all raw outputs in the repo or release artifact;
- Use consistent flags that are advantageous to each tool (not one-size-fits-none);
- Validate parity integrity with tool-native verifiers;
- Predefine scoring and plots before running;
- Document any anomalies and reruns.

Local-only runners
- `scripts/bench_repair_smoke.sh` — quick sanity: create → delete subset → repair; asserts dataset hash matches baseline via `parx hashcat`.
- `scripts/bench_matrix_local.sh` — matrix over scenarios and parameters (K, parity%, chunk size, interleave). Writes JSONL to `_tgt/bench-results/bench-<timestamp>.jsonl`.

Usage (local)
- `make bench-local`
- `make bench-matrix`
- Customize via env, e.g.:
  `K_SET="8 16" PARITY_PCT_SET="25 50" CHUNK_SET="65536 1048576" INTERLEAVE_SET="off on" SCENARIOS="many-small mixed" ./scripts/bench_matrix_local.sh`

Hash catalogue
- `parx hashcat <root>` emits per-file BLAKE3 and a deterministic dataset hash; use `--hash-only` for baseline/post comparisons.

CI policy
- Heavy benchmarks are guarded and refuse to run in CI (`CI`/`GITHUB_ACTIONS`).
