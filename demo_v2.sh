#!/usr/bin/env bash
set -euo pipefail

BIN=./target/release/parx
ROOT=demo_data
PARX_DIR=.parx

echo "==> Ensuring parx binary (release)…"
if [[ ! -x "$BIN" ]]; then
  cargo build --release -p parx-cli
fi
echo "✔ parx ready: $BIN"

echo "==> Cleaning old artifacts…"
rm -rf "$ROOT" "$PARX_DIR"
mkdir -p "$ROOT"
echo "✔ Clean."

echo "==> Generating sample data (~160 MiB across 4 files)…"
# 4 files x 40 MiB -> 160 chunks at 1 MiB; with K=64 => stripes = ceil(160/64) = 3
for i in 0 1 2 3; do
  dd if=/dev/urandom of="$ROOT/file-$i.bin" bs=1M count=40 status=none
done
du -sh "$ROOT"
echo "✔ Sample data generated."

echo "==> Creating parity (K=64, 35%, chunk=1MiB, volumes=3, outer_parity=4)…"
"$BIN" create   --parity 35   --stripe-k 64   --chunk-size 1048576   --output "$PARX_DIR"   --volume-sizes 32M,32M,32M   --outer-parity 4   "$ROOT"
echo "✔ Parity created under $PARX_DIR"

echo "==> Quickcheck volumes…"
"$BIN" quickcheck "$PARX_DIR" || true

echo "==> Parity-aware audit…"
"$BIN" paritycheck "$PARX_DIR" || true

echo "==> Verify source against manifest…"
"$BIN" verify "$PARX_DIR/manifest.json" . || true

echo "==> TORTURE 1: Random data corruption then repair (should be repairable)"
# Corrupt 6 random 4 KiB pages across the data set
for i in {1..6}; do
  tgt=$(find "$ROOT" -type f | shuf -n1)
  size=$(stat -c%s "$tgt")
  # pick an offset aligned to 4096 within file
  if (( size > 8192 )); then
    off=$(( (RANDOM % (size - 4096)) / 4096 * 4096 ))
  else
    off=0
  fi
  dd if=/dev/urandom of="$tgt" bs=4096 seek=$((off/4096)) count=1 conv=notrunc status=none
done
echo "✔ Data corruption injected."

echo "==> Auditing damage by stripe…"
"$BIN" audit "$PARX_DIR/manifest.json" . || true

echo "==> Attempting repair…"
"$BIN" repair "$PARX_DIR/manifest.json" . || true

echo "==> Verifying after repair…"
"$BIN" verify "$PARX_DIR/manifest.json" . || true

echo "==> TORTURE 2: Delete one entire parity volume (forces outer-RS to reconstruct inner parity)"
# Delete one parity volume (choose the middle one if present)
VDEL=$(ls "$PARX_DIR"/vol-*.parxv | sort | sed -n '2p' || true)
if [[ -n "${VDEL:-}" ]]; then
  echo "-- Deleting $VDEL"
  rm -f "$VDEL"
else
  echo "-- No volume to delete (skipping)"
fi

echo "==> Parity-aware audit after deletion…"
"$BIN" paritycheck "$PARX_DIR" || true

echo "==> Inject small additional data damage (2 pages)…"
for i in 1 2; do
  tgt=$(find "$ROOT" -type f | shuf -n1)
  size=$(stat -c%s "$tgt")
  if (( size > 8192 )); then
    off=$(( (RANDOM % (size - 4096)) / 4096 * 4096 ))
  else
    off=0
  fi
  dd if=/dev/urandom of="$tgt" bs=4096 seek=$((off/4096)) count=1 conv=notrunc status=none
done

echo "==> Audit and repair again (outer-RS should help if inner parity is short)"
"$BIN" audit "$PARX_DIR/manifest.json" . || true
"$BIN" repair "$PARX_DIR/manifest.json" . || true
"$BIN" verify "$PARX_DIR/manifest.json" . || true

echo "==> TORTURE 3: Corrupt a volume index CRC (should show ERROR in quickcheck)"
VCRC=$(ls "$PARX_DIR"/vol-*.parxv | head -n1 || true)
if [[ -n "${VCRC:-}" ]]; then
  echo "-- Flipping last byte of $VCRC to break CRC"
  fsz=$(stat -c%s "$VCRC")
  # Flip the very last byte in the file
  python3 - "$VCRC" "$fsz" <<'PY'
import sys, os
p = sys.argv[1]; n = int(sys.argv[2])
with open(p, 'r+b') as f:
    f.seek(n-1); b = f.read(1)
    if not b: sys.exit(0)
    f.seek(n-1); f.write(bytes([b[0]^0xFF]))
PY
  echo "-- quickcheck should now report index ERROR for this volume"
  "$BIN" quickcheck "$PARX_DIR" || true
else
  echo "-- No volume to CRC-corrupt (skipping)"
fi

echo "==> Done."
