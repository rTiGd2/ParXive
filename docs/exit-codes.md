# Exit Codes

Status: Initial policy (to be expanded alongside typed error taxonomy)

ParXive follows a POSIX-style exit code scheme inspired by `sysexits.h`. Codes are stable for automation and scripting.

Core mapping
- 0 (OK): success
- 64 (EX_USAGE): CLI usage error (invalid flags/args) — note: `clap` currently exits with code 2 on parse errors; we will migrate to `try_parse` to normalize to 64.
- 65 (EX_DATAERR): data is invalid (e.g., manifest/index parse error, integrity mismatch)
- 66 (EX_NOINPUT): required input not found (missing file/dir)
- 69 (EX_UNAVAILABLE): feature not available (e.g., GPU backend not built)
- 70 (EX_SOFTWARE): unexpected internal error (bug)
- 71 (EX_OSERR): underlying OS error not otherwise classified
- 73 (EX_CANTCREAT): cannot create output (e.g., parity files/dirs)
- 74 (EX_IOERR): I/O error (read/write/seek/truncate/fdatasync)
- 77 (EX_NOPERM): permission denied (files or directories)
- 78 (EX_CONFIG): configuration error

ParXive-specific
- Future: explicit codes in 80–99 range for domain‑specific states if needed (e.g., verify “mismatch” distinct from parse errors). For now, use 65 for integrity/data errors.

CLI behavior
- Runtime errors map to the above (implemented in `parx-cli` main wrapper).
- Usage errors: `clap` currently exits with 2; we will switch to `try_parse` and map to 64.
- JSON mode: commands that support `--json` will emit structured error objects with `code`, `kind`, `message`, and optional `path`/`op`.

Library guidance
- Libraries should not exit; they return typed errors. `parx-core` will expose a `ParxError` with a stable `ErrorKind` enum and `to_exit_code()` helper for consumers.
- Public API will use `thiserror`-based enums; `anyhow` reserved for binaries/tests. Optional `backtrace` feature for diagnostics.

Documentation
- This document is referenced from README, man page, and `--help` extended docs.
- Changes to codes must be recorded here and treated as breaking for scripts.

