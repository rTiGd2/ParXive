# Contributing to ParXive

Thanks for considering a contribution! This document explains how to get set up,
what quality checks to run, and the licensing terms for contributions.

## Getting started

- Rust: stable toolchain (install via [rustup](https://rustup.rs))
- Build everything:

  ```bash
  cargo build --workspace
  ```

- Run tests:

  ```bash
  cargo test --workspace
  ```

## Quality gates

Before opening a PR, please make sure the following commands succeed locally:

```bash
# Format
cargo fmt --all

# Lints (no warnings)
cargo clippy --workspace --all-targets -- -D warnings

# Tests
cargo test --workspace
```

Optional (if installed):

```bash
cargo audit          # security advisories
cargo deny check     # licenses / bans / sources policy
```

## Commit messages & PRs

- Keep commit messages descriptive (what changed and why).
- Small, focused PRs are easier to review.
- Add tests when fixing bugs or adding features.

## Code style

- Follow Rust's default `rustfmt` style.
- Prefer small, clear functions; avoid unnecessary unsafe.
- Use `?` for error propagation. In binaries, `anyhow` is fine; in library crates, prefer `thiserror` and typed errors.

## Developer references

- Developer Guide: `docs/dev-guide.md` — integration patterns and rationale, including error handling choices and interleaving guidance.
- Exit Codes: `docs/exit-codes.md` — stable CLI exit code mapping (sysexits-inspired).
- Error policy (libraries): see `internal/errors.md` for typed error design and best practices.

## Licensing

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in ParXive by you, as defined in the Apache-2.0 license, shall be
dual licensed as **MIT OR Apache-2.0** without any additional terms or
conditions.

You can include the following in your files:

```text
// SPDX-License-Identifier: MIT OR Apache-2.0
```

## Reporting security issues

Please do not open a public issue for security-sensitive reports. Instead,
contact the maintainers privately (add a SECURITY.md later if preferred).

## Hooks

- We use a pre-push hook to run fmt, clippy -D warnings, and tests. Enable hooks with:
  - `git config core.hooksPath .githooks`
