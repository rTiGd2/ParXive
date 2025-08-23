#!/usr/bin/env bash
set -euo pipefail

# Fair, reproducible comparison: ParXive vs par2cmdline (single-file scenario)
# Outputs one JSONL line with timings, sizes, and success flags

if [[ "${GITHUB_ACTIONS:-}" == "true" || "${CI:-}" == "true" ]]; then
  echo "This benchmark is designed for local runs only. Skipping in CI." >&2
  exit 2
fi

ROOT_BASE="${1:-_tgt/bench-compare/root}"
OUT_PARX="${2:-_tgt/bench-compare/parx}"
OUT_DIR="${3:-_tgt/bench-results}"
mkdir -p "${ROOT_BASE}" "${OUT_PARX}" "${OUT_DIR}"
STAMP=$(date +%Y%m%d-%H%M%S)
RESULTS="${OUT_DIR}/compare-${STAMP}.jsonl"

# Parameters (override via env)
PARITY_PCT=${PARITY_PCT:-35}
STRIPE_K=${STRIPE_K:-16}
CHUNK_SIZE=${CHUNK_SIZE:-1048576}      # 1 MiB
SINGLE_SIZE=${SINGLE_SIZE:-1073741824}  # 1 GiB

ts_ms() {
  if date +%s%3N >/dev/null 2>&1; then date +%s%3N; else python3 - <<'PY'
import time; print(int(time.time()*1000))
PY
  fi
}

# Locate par2cmdline (prefer 'par2', fallback to 'par2create')
PAR2_BIN=""
if command -v par2 >/dev/null 2>&1; then PAR2_BIN="par2"; fi
if [[ -z "$PAR2_BIN" ]] && command -v par2create >/dev/null 2>&1; then PAR2_BIN="par2create"; fi
if [[ -z "$PAR2_BIN" ]]; then
  echo "par2cmdline not found. Install it (e.g., sudo apt-get update && sudo apt-get install -y par2)" >&2
  exit 1
fi

BASE_DIR="${ROOT_BASE}/base"; PARX_DIR="${ROOT_BASE}/root_parx"; PAR2_DIR="${ROOT_BASE}/root_par2"
rm -rf "$BASE_DIR" "$PARX_DIR" "$PAR2_DIR" "$OUT_PARX"
mkdir -p "$BASE_DIR" "$PARX_DIR" "$PAR2_DIR" "$OUT_PARX"

# Dataset: single random file (from /dev/urandom)
dd if=/dev/urandom of="$BASE_DIR/single.bin" bs=1M count=$((SINGLE_SIZE/1048576)) status=none

# Clone identical copies for each tool
cp -a "$BASE_DIR/." "$PARX_DIR/"
cp -a "$BASE_DIR/." "$PAR2_DIR/"

# Baseline hash (deterministic catalogue)
BASE_JSON=$(cargo run -q -p parx-cli -- hashcat --json "$BASE_DIR")
BASE_HASH=$(echo "$BASE_JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["dataset_hash_hex"])')
TOTAL_BYTES=$(echo "$BASE_JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["total_bytes"])')

# Encode ParXive
t0=$(ts_ms)
cargo run -q -p parx-cli -- create \
  --parity "$PARITY_PCT" --stripe-k "$STRIPE_K" --chunk-size "$CHUNK_SIZE" \
  --output "$OUT_PARX" --volume-sizes 64M,64M,64M "$PARX_DIR"
t1=$(ts_ms)
PARX_MS=$((t1 - t0))
PARX_BYTES=$(find "$OUT_PARX" -maxdepth 1 -name 'vol-*.parxv' -printf '%s\n' | awk '{s+=$1} END{print s+0}')

# Encode par2cmdline (quiet). Prefer unified CLI if available; fall back to par2create.
pushd "$PAR2_DIR" >/dev/null
t2=$(ts_ms)
if [[ "$PAR2_BIN" == "par2" ]]; then
  par2 create -r"$PARITY_PCT" -q bench single.bin >/dev/null
else
  par2create -r"$PARITY_PCT" -q bench single.bin >/dev/null
fi
t3=$(ts_ms)
PAR2_MS=$((t3 - t2))
PAR2_BYTES=$(find . -maxdepth 1 -type f -name '*.par2' -printf '%s\n' | awk '{s+=$1} END{print s+0}')
popd >/dev/null

# Apply identical corruption to each copy (same offset, 4 KiB)
OFF=$(python3 - "$PARX_DIR/single.bin" "$PAR2_DIR/single.bin" <<'PY'
import os,sys
p1,p2=sys.argv[1],sys.argv[2]
sz=os.path.getsize(p1)
off=max(0, (sz//2)-2048)  # deterministic mid-file flip
with open(p1,'r+b') as f:
  f.seek(off); f.write(os.urandom(4096))
with open(p2,'r+b') as f:
  f.seek(off); f.write(os.urandom(4096))
print(off)
PY
)

# Repair ParXive
t4=$(ts_ms)
RJSON=$(cargo run -q -p parx-cli -- repair --json "$OUT_PARX/manifest.json" "$PARX_DIR" || true)
t5=$(ts_ms)
PARX_REP_MS=$((t5 - t4))
PARX_REPAIRED=$(python3 - <<'PY'
import json,sys,os
j=os.environ.get('RJSON','')
print(json.loads(j).get('repaired_chunks',0) if j else 0)
PY
)

# Cleanup backups before post-hash
find "$PARX_DIR" -type f -name '*.parx.bak' -print0 2>/dev/null | xargs -0 -r rm -f --

POST_HASH_PARX=$(cargo run -q -p parx-cli -- hashcat --hash-only "$PARX_DIR")
PARX_OK=false; [[ "$BASE_HASH" == "$POST_HASH_PARX" ]] && PARX_OK=true

# Repair par2cmdline
pushd "$PAR2_DIR" >/dev/null
t6=$(ts_ms)
if [[ "$PAR2_BIN" == "par2" ]]; then
  par2 repair -q bench.par2 >/dev/null || true
else
  par2repair -q bench.par2 >/dev/null || true
fi
t7=$(ts_ms)
PAR2_REP_MS=$((t7 - t6))
popd >/dev/null

POST_HASH_PAR2=$(cargo run -q -p parx-cli -- hashcat --hash-only "$PAR2_DIR")
PAR2_OK=false; [[ "$BASE_HASH" == "$POST_HASH_PAR2" ]] && PAR2_OK=true

# Emit JSON line
PARITY_PCT="$PARITY_PCT" STRIPE_K="$STRIPE_K" CHUNK_SIZE="$CHUNK_SIZE" TOTAL_BYTES="$TOTAL_BYTES" \
PARX_MS="$PARX_MS" PARX_BYTES="$PARX_BYTES" PARX_REP_MS="$PARX_REP_MS" PARX_OK="$PARX_OK" \
PAR2_MS="$PAR2_MS" PAR2_BYTES="$PAR2_BYTES" PAR2_REP_MS="$PAR2_REP_MS" PAR2_OK="$PAR2_OK" \
BASE_HASH="$BASE_HASH" POST_HASH_PARX="$POST_HASH_PARX" POST_HASH_PAR2="$POST_HASH_PAR2" RESULTS="$RESULTS" \
CORRUPT_OFF="$OFF" \
python3 - <<'PY'
import json,os,sys
obj=dict(
  type='compare', scenario='single', ts=int(__import__('time').time()),
  parity_pct=int(os.environ['PARITY_PCT']), stripe_k=int(os.environ['STRIPE_K']), chunk=int(os.environ['CHUNK_SIZE']),
  total_bytes=int(os.environ['TOTAL_BYTES']),
  parx_encode_ms=int(os.environ['PARX_MS']), parx_parity_bytes=int(os.environ['PARX_BYTES']),
  parx_repair_ms=int(os.environ['PARX_REP_MS']), parx_ok=(os.environ.get('PARX_OK','false')=='true'),
  par2_encode_ms=int(os.environ['PAR2_MS']), par2_parity_bytes=int(os.environ['PAR2_BYTES']),
  par2_repair_ms=int(os.environ['PAR2_REP_MS']), par2_ok=(os.environ.get('PAR2_OK','false')=='true'),
  base_hash=os.environ['BASE_HASH'], post_hash_parx=os.environ['POST_HASH_PARX'], post_hash_par2=os.environ['POST_HASH_PAR2'],
  corrupt_off=int(os.environ.get('CORRUPT_OFF','0')),
)
out=os.environ['RESULTS']
print(json.dumps(obj))
open(out,'a').write(json.dumps(obj)+'\n')
PY

echo "Done. Results: $RESULTS"

