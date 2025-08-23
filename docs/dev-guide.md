# Developer Guide

Purpose: Help integrators use ParXive effectively while keeping choices open. Recommended (“optimal”) paths are justified with reasons, not rules.

Error handling
- Minimal: Bubble `ParxError` (typed) and map to your own codes or our scheme (docs/exit-codes.md). Pros: stable, small surface. Cons: less rich diagnostics.
- Rich: Wrap errors with `miette` or `error-stack` in your app. Pros: user-friendly, annotated. Cons: extra deps/setup.
- Why recommended: Typed errors in the library stabilize integrations, enable structured logs/telemetry, and make exit codes consistent. Rich wrappers stay app-side.

Configuration patterns
- Builder-style config (recommended): Use a builder (planned) or helpers to construct `EncoderConfig` to avoid invalid mixes. Clear defaults, validation upfront.
- Direct struct init (supported): Construct `EncoderConfig` directly for maximum control.

Interleaving vs sequential chunking
- Interleave (`--interleave-files`): Round-robin chunks across files per stripe for resilience to full-file loss when stripes span multiple files. Trade-off: slightly less cache locality for single-file workloads.
- Sequential (default): Simpler order; may be faster for single large files.

I/O models
- Blocking (current): Simple and portable; good for CLIs and batch tools.
- Async (planned): Expose async APIs where beneficial to integrate with async runtimes.
- Data providers (planned): Trait-based streaming sources/sinks for object stores/archives to reduce memory and support custom backends.

Concurrency and Performance
- Parallel stages: encode (RS per-stripe) and verify (per-file) run in parallel.
- Tuning knobs:
  - Global: `--threads N` (bound Rayon thread pool size for encode/verify/repair).
  - Niceness: `--nice <int>` best-effort process niceness via `renice`.
  - IO niceness: `--ionice <class[:prio]>` best-effort IO priority via `ionice`.
- Guidance:
  - SSD/NVMe: higher `--threads` often helps.
  - HDD: test lower `--threads` to reduce seek contention.
  - Co-tenant systems: lower `--nice`/use `--ionice be:6` to play nice.

Filesystem and portability
- Path policy: normalize to relative paths; block traversal; opt-in symlink following with containment checks.
- Platform specifics: Windows reserved names, illegal characters, case sensitivity, path length limits. See internal/implementation_plan.md and internal/roadmap.md gates.

Exit codes and JSON output
- Exit codes: docs/exit-codes.md — stable for automation.
- JSON: `--json` emits `{ code, kind, message, path?, op? }` when supported.

Examples to add in rustdoc
- Quick start: encode + verify with minimal error mapping.
- Optimal: builder config, interleave for full-file-loss resilience, atomic repair with backups, structured errors.
