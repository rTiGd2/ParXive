# ParX

Reed–Solomon parity + integrity (BLAKE3, Merkle) for large file sets.

## Workspace
- `parx-core`: library (encoding, layout, I/O)
- `parx-cli`: CLI using the library

## Build & test
```bash
cargo build --release -p parx-cli
cargo test --workspace

## Quick start
```bash
./target/release/parx create --parity 35 --stripe-k 64 --chunk-size 1048576 \
  --output .parx --volume-sizes 32M,32M,32M demo_data
./target/release/parx verify .parx/manifest.json .


---

# parx-core: property/unit tests

## `parx-core/Cargo.toml` (add dev-deps)

```toml
[dev-dependencies]
proptest = "1"
rand = "0.8"

