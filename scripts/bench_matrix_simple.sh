#!/usr/bin/env bash
set -euo pipefail

# Simplified local-only benchmark matrix runner (workaround for parsing issues).
# Generates datasets, encodes, damages, repairs, and validates via `parx hashcat`.
# Outputs JSONL results to _tgt/bench-results/bench-<timestamp>.jsonl

if [[ "${GITHUB_ACTIONS:-}" == "true" || "${CI:-}" == "true" ]]; then
  echo "This benchmark matrix is designed for local runs only. Skipping in CI." >&2
  exit 2
fi

ROOT_BASE="${1:-_tgt/bench-matrix-root}"
OUT_BASE="${2:-_tgt/bench-matrix-out}"
RES_DIR="${3:-_tgt/bench-results}"

mkdir -p "$ROOT_BASE" "$OUT_BASE" "$RES_DIR"
STAMP=$(date +%Y%m%d-%H%M%S)
RESULTS="$RES_DIR/bench-$STAMP.jsonl"

# Parameter sets (override via env vars)
K_SET_STR=${K_SET:-"8 16"}
PARITY_PCT_SET_STR=${PARITY_PCT_SET:-"25 50"}
CHUNK_SET_STR=${CHUNK_SET:-"65536 1048576"} # 64 KiB, 1 MiB
INTERLEAVE_SET_STR=${INTERLEAVE_SET:-"off on"}
GPU_SET_STR=${GPU_SET:-"off"}
SCENARIOS_STR=${SCENARIOS:-"many-small many-large single mixed"}
read -r -a K_SET <<<"$K_SET_STR"
read -r -a PARITY_PCT_SET <<<"$PARITY_PCT_SET_STR"
read -r -a CHUNK_SET <<<"$CHUNK_SET_STR"
read -r -a INTERLEAVE_SET <<<"$INTERLEAVE_SET_STR"
read -r -a GPU_SET <<<"$GPU_SET_STR"
read -r -a SCENARIOS <<<"$SCENARIOS_STR"

# Scenario sizes (override via env)
SMALL_COUNT=${SMALL_COUNT:-1000}
SMALL_SIZE=${SMALL_SIZE:-4096}
LARGE_COUNT=${LARGE_COUNT:-4}
LARGE_SIZE=${LARGE_SIZE:-16777216}
SINGLE_SIZE=${SINGLE_SIZE:-67108864}
MIXED_SMALL=${MIXED_SMALL:-200}
MIXED_LARGE=${MIXED_LARGE:-2}

ts_ms() {
  if date +%s%3N >/dev/null 2>&1; then date +%s%3N; else python3 - <<'PY'
import time; print(int(time.time()*1000))
PY
  fi
}

gen_dataset() {
  local scenario="$1"; local root="$2";
  rm -rf "$root"; mkdir -p "$root"
  case "$scenario" in
    many-small)
      mkdir -p "$root/s"
      for ((i=1;i<=SMALL_COUNT;i++)); do
        head -c "$SMALL_SIZE" </dev/urandom >"$root/s/f$(printf %06d "$i").bin"
      done
      ;;
    many-large)
      mkdir -p "$root/l"
      for ((i=1;i<=LARGE_COUNT;i++)); do
        head -c "$LARGE_SIZE" </dev/urandom >"$root/l/L$(printf %03d "$i").bin"
      done
      ;;
    single)
      head -c "$SINGLE_SIZE" </dev/urandom >"$root/single.bin"
      ;;
    mixed)
      mkdir -p "$root/s" "$root/l" "$root/m"
      for ((i=1;i<=MIXED_SMALL;i++)); do
        head -c "$SMALL_SIZE" </dev/urandom >"$root/s/s$(printf %05d "$i").bin"
      done
      for ((i=1;i<=MIXED_LARGE;i++)); do
        head -c "$LARGE_SIZE" </dev/urandom >"$root/l/L$(printf %03d "$i").bin"
      done
      for i in 1 2 3; do
        head -c $((SMALL_SIZE*128)) </dev/urandom >"$root/m/M$i.bin"
      done
      ;;
    *) echo "Unknown scenario: $scenario" >&2; return 1;;
  esac
}

damage_dataset() {
  local scenario="$1"; local root="$2";
  case "$scenario" in
    many-small)
      find "$root/s" -type f | shuf -n $((SMALL_COUNT/100 + 1)) | xargs -r rm -f -- ;;
    many-large)
      rm -f -- "$(find "$root/l" -type f | head -n1)" ;;
    single)
      python3 - "$root/single.bin" <<'PY'
import os,sys,random
p=sys.argv[1]
sz=os.path.getsize(p)
off=max(0, random.randrange(max(1, sz-4096)))
with open(p,'r+b') as f:
  f.seek(off); f.write(os.urandom(4096))
PY
      ;;
    mixed)
      find "$root/s" -type f | shuf -n $((MIXED_SMALL/50 + 1)) | xargs -r rm -f -- ;;
  esac
}

echo "Writing results to: $RESULTS"
# Emit environment meta line (ignore failure)
bash scripts/bench_env_info.sh >> "$RESULTS" || true

for scenario in "${SCENARIOS[@]}"; do
  for K in "${K_SET[@]}"; do
    for PP in "${PARITY_PCT_SET[@]}"; do
      for CHUNK in "${CHUNK_SET[@]}"; do
        for IL in "${INTERLEAVE_SET[@]}"; do
          for GPU in "${GPU_SET[@]}"; do
            ROOT="$ROOT_BASE/$scenario-K$K-P$PP-C$CHUNK-IL$IL"
            OUT="$OUT_BASE/$scenario-K$K-P$PP-C$CHUNK-IL$IL"
            rm -rf "$OUT"
            gen_dataset "$scenario" "$ROOT"

            BASE_JSON=$(cargo run -q -p parx-cli -- hashcat --json "$ROOT")
            BASE_HASH=$(echo "$BASE_JSON" | jq -r .dataset_hash_hex)
            TOTAL_BYTES=$(echo "$BASE_JSON" | jq -r .total_bytes)

            ILOPTS=""; if [[ "$IL" == "on" ]]; then ILOPTS="--interleave-files"; fi

            t0=$(ts_ms)
            cargo run -q -p parx-cli -- create --parity "$PP" --stripe-k "$K" $ILOPTS --chunk-size "$CHUNK" --gpu "$GPU" --output "$OUT" --volume-sizes 64M,64M,64M "$ROOT" || true
            t1=$(ts_ms)
            enc_ms=$((t1 - t0))

            PARITY_BYTES=$(find "$OUT" -maxdepth 1 -name 'vol-*.parxv' -printf '%s\n' | awk '{s+=$1} END{print s+0}')

            damage_dataset "$scenario" "$ROOT"
            t2=$(ts_ms)
            RJSON=$(cargo run -q -p parx-cli -- repair --json "$OUT/manifest.json" "$ROOT" || true)
            # Remove repair backups before computing post hash to compare dataset content
            find "$ROOT" -type f -name '*.parx.bak' -print0 2>/dev/null | xargs -0 -r rm -f --
            t3=$(ts_ms)
            rep_ms=$((t3 - t2))
            repaired=$(echo "$RJSON" | jq -r '.repaired_chunks // 0' 2>/dev/null || echo 0)
            failed=$(echo "$RJSON" | jq -r '.failed_chunks // 0' 2>/dev/null || echo 0)

            POST_HASH=$(cargo run -q -p parx-cli -- hashcat --hash-only "$ROOT")
            ok=false; [[ "$BASE_HASH" == "$POST_HASH" ]] && ok=true

            # Emit JSON line using Python for robust quoting/types
            SCENARIO="$scenario" K="$K" PP="$PP" CHUNK="$CHUNK" IL="$IL" GPU="$GPU" \
            TOTAL_BYTES="$TOTAL_BYTES" PARITY_BYTES="$PARITY_BYTES" ENC_MS="$enc_ms" REP_MS="$rep_ms" \
            REPAIRED="$repaired" FAILED="$failed" OK="$ok" ROOT="$ROOT" OUT="$OUT" \
            BASE_HASH="$BASE_HASH" POST_HASH="$POST_HASH" \
            python3 - "$RESULTS" <<'PY'
import json, os, sys, time
out=sys.argv[1]
obj={
  'type':'result', 'ts': int(time.time()),
  'scenario': os.environ['SCENARIO'],
  'k': int(os.environ['K']),
  'parity_pct': int(os.environ['PP']),
  'chunk': int(os.environ['CHUNK']),
  'interleave': os.environ['IL'],
  'gpu': os.environ['GPU'],
  'total_bytes': int(os.environ['TOTAL_BYTES']),
  'parity_bytes': int(os.environ['PARITY_BYTES']),
  'encode_ms': int(os.environ['ENC_MS']),
  'repair_ms': int(os.environ['REP_MS']),
  'repaired_chunks': int(os.environ['REPAIRED'] or 0),
  'failed_chunks': int(os.environ['FAILED'] or 0),
  'ok': os.environ['OK'] == 'true',
  'root': os.environ['ROOT'],
  'out': os.environ['OUT'],
  'base_hash': os.environ['BASE_HASH'],
  'post_hash': os.environ['POST_HASH'],
}
with open(out,'a') as w:
  w.write(json.dumps(obj)+'\n')
PY
            echo "[${scenario}] K=$K P=$PP C=$CHUNK IL=$IL GPU=$GPU => ok=$ok enc=${enc_ms}ms rep=${rep_ms}ms"
          done
        done
      done
    done
  done
done

echo "Done. Results: $RESULTS"
