# ParX

Reed–Solomon parity + integrity (BLAKE3 + Merkle) for large file sets.  
Fast CPU path, robust volume format, and a clean library/CLI split.

## Workspace

- `parx-core` — library (encoding, layout, I/O, hashing)
- `parx-cli`  — CLI using the library

## Build & Test

```bash
cargo build --release -p parx-cli
cargo test --workspace
```

## Quick Start

```bash
# Create parity (35% over stripes of 64, 1 MiB chunks)
./target/release/parx create   --parity 35 --stripe-k 64 --chunk-size 1048576   --output .parx --volume-sizes 32M,32M,32M demo_data

# Verify source files against manifest
./target/release/parx verify .parx/manifest.json .

# Audit damage by stripe (how many chunks missing per stripe)
./target/release/parx audit .parx/manifest.json .

# Attempt repair (uses per-stripe RS + parity entries in volumes)
./target/release/parx repair .parx/manifest.json .
```

## Why ParX (vs PAR2)

- **Per-stripe RS**: targets real damage patterns and limits blast radius.
- **Integrity-first**: BLAKE3 per-chunk + Merkle root in the manifest.
- **Robust volume index**: compressed index trailer; header hints; parity-aware audit.
- **Round-robin parity placement**: losing one volume hurts less.
- **Library-first**: embed ParX in other Rust tools; CLI is thin veneer.
- **GPU path (scaffolded)**: CUDA backend hooks ready for batched stripes.

## Roadmap

- i18n via Fluent (en-GB default) across CLI messages.
- TUI with interactive/create/verify/repair flows.
- Outer RS (parity-of-parity) decode path.
- Optional CUDA batched RS kernels for big sets.
- PAR2 interop (reader/writer) as a separate crate.

## License

Dual-licensed under **MIT** and **Apache-2.0** — pick one or both. See `LICENSE-MIT` and `LICENSE-APACHE`.
