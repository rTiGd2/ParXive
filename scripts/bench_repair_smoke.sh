#!/usr/bin/env bash
set -euo pipefail

# Simple smoke benchmark: create dataset, encode, delete a subset, repair, verify via hashcat

ROOT=${1:-"_tgt/bench-dataset"}
OUT=${2:-"_tgt/bench-out"}
INTERLEAVE=${3:-"--interleave-files"}

rm -rf "$ROOT" "$OUT"
mkdir -p "$ROOT" "$OUT"

echo "Generating dataset under $ROOT ..."
mkdir -p "$ROOT/small" "$ROOT/large"
for i in $(seq 1 100); do
  printf "file %04d\n" "$i" > "$ROOT/small/s_$i.txt"
done

# Large files (~1 MiB each)
dd if=/dev/zero of="$ROOT/large/L1.bin" bs=1M count=2 status=none
dd if=/dev/zero of="$ROOT/large/L2.bin" bs=1M count=4 status=none

echo "Baseline hash catalogue..."
BASE_HASH=$(cargo run -q -p parx-cli -- hashcat --hash-only "$ROOT")
echo "BASE: $BASE_HASH"

echo "Encoding with parity..."
time cargo run -q -p parx-cli -- create --parity 50 --stripe-k 8 $INTERLEAVE --chunk-size 65536 --output "$OUT" --volume-sizes 16M,16M,16M "$ROOT"

echo "Simulating damage (delete 10 small files)..."
for i in $(seq 1 10); do rm -f "$ROOT/small/s_$i.txt"; done

echo "Repairing..."
time cargo run -q -p parx-cli -- repair "$OUT/manifest.json" "$ROOT"

echo "Post-repair hash catalogue..."
POST_HASH=$(cargo run -q -p parx-cli -- hashcat --hash-only "$ROOT")
echo "POST: $POST_HASH"

if [[ "$BASE_HASH" == "$POST_HASH" ]]; then
  echo "OK: Dataset hash matches baseline after repair."
  exit 0
else
  echo "FAIL: Dataset hash mismatch after repair." >&2
  exit 1
fi

