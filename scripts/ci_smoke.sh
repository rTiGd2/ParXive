#!/usr/bin/env bash
set -euo pipefail

BIN=./target/release/parx
ROOT=ci_demo_data
PARX_DIR=.parx

echo "==> Build release parx"
cargo build --release -p parx-cli

echo "==> Clean demo dirs"
rm -rf "$ROOT" "$PARX_DIR"
mkdir -p "$ROOT"

echo "==> Generate small sample data (3 x 256 KiB)"
for i in 0 1 2; do
  dd if=/dev/urandom of="$ROOT/file-$i.bin" bs=64K count=4 status=none # 256 KiB each
done

echo "==> Create parity (K=8, 35%, chunk=64 KiB, 2 volumes, outer_parity=2)"
"$BIN" create   --parity 35   --stripe-k 8   --chunk-size 65536   --output "$PARX_DIR"   --volume-sizes 2M,2M   --outer-parity 2   "$ROOT"

echo "==> Verify should be OK"
out=$("$BIN" verify "$PARX_DIR/manifest.json" . | tail -n1)
echo "verify: $out"
test "$out" = "OK"

echo "==> Corrupt one random 4 KiB page"
tgt=$(find "$ROOT" -type f | shuf -n1)
size=$(stat -c%s "$tgt")
off=$(( (RANDOM % (size - 4096 + 1)) / 4096 * 4096 ))
dd if=/dev/urandom of="$tgt" bs=4096 seek=$((off/4096)) count=1 conv=notrunc status=none

echo "==> Repair"
"$BIN" repair "$PARX_DIR/manifest.json" .

echo "==> Verify again should be OK"
out=$("$BIN" verify "$PARX_DIR/manifest.json" . | tail -n1)
echo "verify: $out"
test "$out" = "OK"

echo "==> Delete one parity volume, corrupt another page, repair using outer-RS"
VDEL=$(find "$PARX_DIR" -maxdepth 1 -type f -name 'vol-*.parxv' -print0 2>/dev/null | xargs -0 -r -n1 basename | sort | sed -n '1p' || true)
if [[ -n "${VDEL:-}" ]]; then
  VDEL="$PARX_DIR/$VDEL"
fi
if [[ -n "${VDEL:-}" ]]; then
  rm -f "$VDEL"
fi
tgt=$(find "$ROOT" -type f | shuf -n1)
size=$(stat -c%s "$tgt")
off=$(( (RANDOM % (size - 4096 + 1)) / 4096 * 4096 ))
dd if=/dev/urandom of="$tgt" bs=4096 seek=$((off/4096)) count=1 conv=notrunc status=none
"$BIN" repair "$PARX_DIR/manifest.json" .
out=$("$BIN" verify "$PARX_DIR/manifest.json" . | tail -n1)
echo "verify: $out"
test "$out" = "OK"

echo "==> Break a volume index CRC, quickcheck should report index ERROR"
VCRC=$(find "$PARX_DIR" -maxdepth 1 -type f -name 'vol-*.parxv' -print0 2>/dev/null | xargs -0 -r -n1 | head -n1 || true)
if [[ -n "${VCRC:-}" ]]; then
  fsz=$(stat -c%s "$VCRC")
  python3 - "$VCRC" "$fsz" <<'PY'
import sys
p, n = sys.argv[1], int(sys.argv[2])
with open(p, 'r+b') as f:
    f.seek(n-1)
    b = f.read(1)
    if b:
        f.seek(n-1)
        f.write(bytes([b[0]^0xFF]))
PY
  qout=$("$BIN" quickcheck "$PARX_DIR" 2>&1 || true)
  echo "$qout"
  echo "$qout" | grep -q "index ERROR" || (echo "expected index ERROR in quickcheck" && exit 1)
fi

echo "==> CI smoke finished"
