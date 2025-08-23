#!/usr/bin/env bash
set -euo pipefail

# Scientific, reproducible benchmark: ParXive vs par2cmdline (WSL)
# - Generates random data via /dev/urandom
# - Measures encode, verify, repair for both tools
# - Applies identical corruption to both datasets
# - Emits one JSON line and a short human summary

if [[ "${GITHUB_ACTIONS:-}" == "true" || "${CI:-}" == "true" ]]; then
  echo "This benchmark is designed for local runs only. Skipping in CI." >&2
  exit 2
fi

ROOT_BASE="${1:-_tgt/bench/root}"
OUT_PARX="${2:-_tgt/bench/parx}"   # .parx dir location
OUT_DIR="${3:-_tgt/bench/results}"
mkdir -p "${ROOT_BASE}" "${OUT_PARX}" "${OUT_DIR}"
STAMP=$(date +%Y%m%d-%H%M%S)
RESULTS_JSON="${OUT_DIR}/run-${STAMP}.jsonl"

# Parameters (override via env)
PARITY_PCT=${PARITY_PCT:-35}
STRIPE_K=${STRIPE_K:-16}
CHUNK_SIZE=${CHUNK_SIZE:-1048576}       # 1 MiB
SINGLE_SIZE=${SINGLE_SIZE:-1073741824}   # 1 GiB

# Build release binary once and resolve path
cargo build -q --release -p parx-cli
PARX_BIN="$(pwd)/target/release/parx"
if [[ ! -x "$PARX_BIN" ]]; then
  echo "ERROR: parx binary not found at $PARX_BIN" >&2
  exit 1
fi

# Ensure par2cmdline present
PAR2_BIN=""
if command -v par2 >/dev/null 2>&1; then PAR2_BIN="par2"; fi
if [[ -z "$PAR2_BIN" ]] && command -v par2create >/dev/null 2>&1; then PAR2_BIN="par2create"; fi
if [[ -z "$PAR2_BIN" ]]; then
  echo "ERROR: par2cmdline not found (install: sudo apt-get update && sudo apt-get install -y par2)" >&2
  exit 1
fi

# Utilities
now_ms() { if date +%s%3N >/dev/null 2>&1; then date +%s%3N; else python3 - <<'PY'
import time; print(int(time.time()*1000))
PY
fi; }
json_escape() { python3 -c 'import json,sys; print(json.dumps(sys.stdin.read().strip()))'; }

# Layout
BASE_DIR="${ROOT_BASE}/base"; PARX_DIR="${ROOT_BASE}/root_parx"; PAR2_DIR="${ROOT_BASE}/root_par2"
rm -rf "$BASE_DIR" "$PARX_DIR" "$PAR2_DIR" "$OUT_PARX"
mkdir -p "$BASE_DIR" "$PARX_DIR" "$PAR2_DIR" "$OUT_PARX"

# Dataset (random)
dd if=/dev/urandom of="$BASE_DIR/single.bin" bs=1M count=$((SINGLE_SIZE/1048576)) status=none
cp -a "$BASE_DIR/." "$PARX_DIR/"
cp -a "$BASE_DIR/." "$PAR2_DIR/"

# Baseline hash
BASE_JSON=$("$PARX_BIN" hashcat --json "$BASE_DIR")
BASE_HASH=$(echo "$BASE_JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["dataset_hash_hex"])')
TOTAL_BYTES=$(echo "$BASE_JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["total_bytes"])')

# ParXive encode
t0=$(now_ms)
PARX_ENC_ERR=""
if ! "$PARX_BIN" create \
  --parity "$PARITY_PCT" --stripe-k "$STRIPE_K" --chunk-size "$CHUNK_SIZE" \
  --output "$OUT_PARX" --volume-sizes 64M,64M,64M "$PARX_DIR"; then
  PARX_ENC_ERR="encode_failed"
fi
t1=$(now_ms); PARX_ENC_MS=$((t1-t0))
PARX_PAR_BYTES=$(find "$OUT_PARX" -maxdepth 1 -name 'vol-*.parxv' -printf '%s\n' | awk '{s+=$1} END{print s+0}')

# ParXive verify (manifest paths are relative to CWD per CLI adjustment; use root='.')
t2=$(now_ms)
PARX_VER_ERR=""
if ! "$PARX_BIN" verify --json "$OUT_PARX/manifest.json" . >/dev/null; then
  PARX_VER_ERR="verify_failed"
fi
t3=$(now_ms); PARX_VER_MS=$((t3-t2))

# par2 encode
pushd "$PAR2_DIR" >/dev/null
t4=$(now_ms)
PAR2_ENC_ERR=""
if [[ "$PAR2_BIN" == "par2" ]]; then
  if ! par2 create -r"$PARITY_PCT" -q bench single.bin >/dev/null; then PAR2_ENC_ERR="encode_failed"; fi
else
  if ! par2create -r"$PARITY_PCT" -q bench single.bin >/dev/null; then PAR2_ENC_ERR="encode_failed"; fi
fi
t5=$(now_ms); PAR2_ENC_MS=$((t5-t4))
PAR2_PAR_BYTES=$(find . -maxdepth 1 -type f -name '*.par2' -printf '%s\n' | awk '{s+=$1} END{print s+0}')

# par2 verify
t6=$(now_ms)
PAR2_VER_ERR=""
if [[ "$PAR2_BIN" == "par2" ]]; then
  if ! par2 verify -q bench.par2 >/dev/null; then PAR2_VER_ERR="verify_failed"; fi
else
  if ! par2verify -q bench.par2 >/dev/null; then PAR2_VER_ERR="verify_failed"; fi
fi
t7=$(now_ms); PAR2_VER_MS=$((t7-t6))
popd >/dev/null

# Corrupt both identically
OFF=$(python3 - "$PARX_DIR/single.bin" "$PAR2_DIR/single.bin" <<'PY'
import os,sys
p1,p2=sys.argv[1],sys.argv[2]
sz=os.path.getsize(p1)
off=max(0,(sz//2)-2048)
blk=os.urandom(4096)
with open(p1,'r+b') as f:
  f.seek(off); f.write(blk)
with open(p2,'r+b') as f:
  f.seek(off); f.write(blk)
print(off)
PY
)

# ParXive repair (use root='.' per CLI manifest path adjustment)
PARX_REP_ERR=""; t8=$(now_ms)
RJSON=$("$PARX_BIN" repair --json "$OUT_PARX/manifest.json" . || true)
t9=$(now_ms); PARX_REP_MS=$((t9-t8))
PARX_REPAIRED=$(python3 - <<'PY'
import json,os
j=os.environ.get('RJSON','')
print(json.loads(j).get('repaired_chunks',0) if j else 0)
PY
)
find "$PARX_DIR" -type f -name '*.parx.bak' -print0 2>/dev/null | xargs -0 -r rm -f --
POST_HASH_PARX=$("$PARX_BIN" hashcat --hash-only "$PARX_DIR")
PARX_OK=false; [[ "$BASE_HASH" == "$POST_HASH_PARX" ]] && PARX_OK=true
[[ "$PARX_OK" == true ]] || PARX_REP_ERR="post_hash_mismatch"

# par2 repair
pushd "$PAR2_DIR" >/dev/null
PAR2_REP_ERR=""; t10=$(now_ms)
if [[ "$PAR2_BIN" == "par2" ]]; then
  par2 repair -q bench.par2 >/dev/null || PAR2_REP_ERR="repair_failed"
else
  par2repair -q bench.par2 >/dev/null || PAR2_REP_ERR="repair_failed"
fi
t11=$(now_ms); PAR2_REP_MS=$((t11-t10))
popd >/dev/null
POST_HASH_PAR2=$("$PARX_BIN" hashcat --hash-only "$PAR2_DIR")
PAR2_OK=false; [[ "$BASE_HASH" == "$POST_HASH_PAR2" ]] && PAR2_OK=true
[[ "$PAR2_OK" == true ]] || PAR2_REP_ERR=${PAR2_REP_ERR:-post_hash_mismatch}

# JSON emission
export PARITY_PCT STRIPE_K CHUNK_SIZE TOTAL_BYTES OFF \
       PARX_ENC_MS PARX_VER_MS PARX_REP_MS PARX_PAR_BYTES PARX_OK PARX_REPAIRED \
       PARX_ENC_ERR PARX_VER_ERR PARX_REP_ERR BASE_HASH POST_HASH_PARX \
       PAR2_ENC_MS PAR2_VER_MS PAR2_REP_MS PAR2_PAR_BYTES PAR2_OK \
       PAR2_ENC_ERR PAR2_VER_ERR PAR2_REP_ERR POST_HASH_PAR2

python3 - <<PY | tee -a "${RESULTS_JSON}"
import json,os,time
obj=dict(
  type='benchmark', ts=int(time.time()), scenario='single_random',
  parity_pct=int(os.environ['PARITY_PCT']), stripe_k=int(os.environ['STRIPE_K']), chunk=int(os.environ['CHUNK_SIZE']),
  total_bytes=int(os.environ['TOTAL_BYTES']), corrupt_off=int(os.environ['OFF']),
  parx=dict(encode_ms=int(os.environ['PARX_ENC_MS']), verify_ms=int(os.environ['PARX_VER_MS']), repair_ms=int(os.environ['PARX_REP_MS']),
            parity_bytes=int(os.environ['PARX_PAR_BYTES'] or 0), ok=os.environ.get('PARX_OK','false')=='true', repaired_chunks=int(os.environ['PARX_REPAIRED'] or 0),
            errors=dict(encode=os.environ.get('PARX_ENC_ERR',''), verify=os.environ.get('PARX_VER_ERR',''), repair=os.environ.get('PARX_REP_ERR','')),
            base_hash=os.environ['BASE_HASH'], post_hash=os.environ['POST_HASH_PARX']),
  par2=dict(encode_ms=int(os.environ['PAR2_ENC_MS']), verify_ms=int(os.environ['PAR2_VER_MS']), repair_ms=int(os.environ['PAR2_REP_MS']),
            parity_bytes=int(os.environ['PAR2_PAR_BYTES'] or 0), ok=os.environ.get('PAR2_OK','false')=='true',
            errors=dict(encode=os.environ.get('PAR2_ENC_ERR',''), verify=os.environ.get('PAR2_VER_ERR',''), repair=os.environ.get('PAR2_REP_ERR','')),
            post_hash=os.environ['POST_HASH_PAR2'])
)
print(json.dumps(obj))
PY

# Human summary
echo "== Summary (${STAMP})"
echo "ParXive: enc=${PARX_ENC_MS}ms ver=${PARX_VER_MS}ms rep=${PARX_REP_MS}ms ok=${PARX_OK} repaired=${PARX_REPAIRED}"
echo "par2:    enc=${PAR2_ENC_MS}ms ver=${PAR2_VER_MS}ms rep=${PAR2_REP_MS}ms ok=${PAR2_OK}"
echo "Results JSON: ${RESULTS_JSON}"
