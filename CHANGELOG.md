# Changelog

All notable changes to this project will be documented in this file.

## [0.6.0-beta.0] - 2025-08-21

Highlights
- Parallel encode (per-stripe RS) with significant speedups on multi-core.
- Parallel verify (per-file hashing).
- Parallel repair (per-stripe) with atomic write semantics preserved.
- Global performance flags: `--threads`, `--nice`, `--ionice`.
- Benchmark improvements: fixed scripts, added `make benchmarks` and `make nightly`.
- Documentation updates: performance tuning guidance and BOOTSTRAP refresh.

Notes
- HDDs (“spinning rust”): We have not optimized for high seek-latency disks yet. On HDDs, consider lower `--threads` and use `--nice/--ionice` to play nice. SSD/NVMe is recommended for testing this beta.
- Windows: priority flags are best-effort (`--nice/--ionice` may be no-ops). Core functionality is supported.

Breaking Changes
- None expected in core CLI for this beta. APIs remain source-compatible.

Migration
- No action required. For performance tuning, try `--threads $(nproc)` on SSD/NVMe; reduce on HDD.

