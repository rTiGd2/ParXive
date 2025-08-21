# Security Policy

Supported Versions
- Pre-1.0.0: latest minor release only (0.x.y). Older minors may receive critical fixes at our discretion.
- Post-1.0.0 (future): semantic versioning with security backports to supported minors.

Report a Vulnerability
- Preferred: open a GitHub Security Advisory (if available on the public repo), or email: security@please-set-domain.example
- Alternative: create a private issue titled "[SECURITY] ...". Do not include exploit details in public issues.
- Please provide:
  - Affected version(s) and environment
  - Reproduction steps and impact assessment
  - Proof of concept or crash logs (sanitized)
  - Your disclosure preference and a contact

Coordinated Disclosure
- We aim to acknowledge within 3 business days and provide a triage result within 7 business days.
- We prefer a 90-day disclosure window (can be shorter/longer by mutual agreement based on impact and fix readiness).

Scope & Threat Model (initial)
- Inputs are untrusted: manifest, indices, trailers, CLI arguments, and file paths
- Denial of service and memory safety concerns
- Path traversal, symlink escapes, and unsafe file writes
- Decompression bombs (zstd) and oversized indices
- Concurrency races (repair operations) and partial writes

Current Mitigations
- Strict path validation: reject absolute and parent traversal; default do not follow symlinks; override requires containment under root
- Index parsing: CRC verification and bounded decompression; entry/size limits
- Repairs: advisory locking (global and per-file), backups by default, fsync and atomic rename when possible
- Pre-commit/CI: clippy -D warnings, tests, and (future) cargo-deny; fuzzing planned for parsers

Hardening Roadmap
- Fuzz parsers (trailer/index/manifest) and RS boundaries
- cargo-deny for dependency audit; SBOM generation
- CI matrix across platforms and MSRV policy
- Optional signing of manifests/indices

