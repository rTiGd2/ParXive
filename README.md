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
./target/release/parx create \
  --parity 35 \
  --stripe-k 64 \
  --chunk-size 1048576 \
  --output .parx \
  --volume-sizes 32M,32M,32M \
  demo_data

## Usage

`parx` provides parity creation and diagnostics. Current subcommands:

- `create` — Create parity volumes and manifest
  - `--parity <PCT>`: Parity percent (e.g., 35 means M ≈ ceil(K * 0.35)).
  - `--stripe-k <K>`: Data shards per stripe.
  - `--chunk-size <BYTES>`: Chunk size; accepts bytes (e.g., 1048576).
  - `--output <DIR>`: Output directory for `.parx` set and volumes.
  - `--volume-sizes <CSV>`: Determines number of volumes by count of CSV entries (e.g., `2M,2M,2M`).
  - `--outer-group`, `--outer-parity`: Reserved for future outer RS.
  - `--gpu`: `off` (default), `on`, or `auto` (GPU integration planned).
  - Example:
    - `parx create --parity 50 --stripe-k 8 --chunk-size 65536 --output .parx --volume-sizes 2M,2M,2M ./data`

- `quickcheck` — Summarize volume indices; prints entry counts.
  - `parx quickcheck .parx`

- `paritycheck` — Parity-aware index check; prints per-volume status.
  - `parx paritycheck .parx`

- `verify` — Verify files against manifest (currently prints `OK`; Stage 2 will implement full verification).
  - `parx verify .parx/manifest.json .`

- `audit` — Audit damage by stripe (currently prints `Repairable: YES`; Stage 2 will implement full audit).
  - `parx audit .parx/manifest.json .`

- `repair` — Attempt repair (stub; Stage 2 will implement reconstruction).
  - `parx repair .parx/manifest.json .`

- `outer-decode` — Inspect a file for a ParX index trailer and validate CRC.
  - `parx outer-decode file.bin`

- `split` — Split a file into N parts as `part-XXX.bin` in an output dir.
  - `parx split input.bin ./out 8`

## Examples

1) Create parity for a dataset with moderate protection

```
parx create \
  --parity 35 \
  --stripe-k 16 \
  --chunk-size 1048576 \
  --output .parx \
  --volume-sizes 16M,16M,16M \
  ./my_data

parx quickcheck .parx
parx paritycheck .parx
```

2) Small dataset (fast) with 50% parity and small chunks

```
parx create --parity 50 --stripe-k 8 --chunk-size 65536 --output .parx --volume-sizes 2M,2M,2M ./demo_data
parx verify .parx/manifest.json .
```

3) Diagnose a volume file

```
parx outer-decode .parx/vol-000.parxv
```

Notes
- ParX stores a compressed, CRC-protected index at the end of each volume file.
- The manifest includes per-chunk BLAKE3 hashes and a dataset Merkle root.
- Outer RS (parity-of-parity) is planned; GPU acceleration is optional.
