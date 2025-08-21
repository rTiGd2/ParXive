#!/usr/bin/env bash
set -euo pipefail

# Local-only benchmark matrix runner.
# Generates datasets, encodes with various K/M/chunk/interleave settings,
# simulates damage, repairs, and validates via `parx hashcat`.
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
K_SET=(${K_SET:-8 16})
PARITY_PCT_SET=(${PARITY_PCT_SET:-25 50})
CHUNK_SET=(${CHUNK_SET:-65536 1048576}) # 64 KiB, 1 MiB
INTERLEAVE_SET=(${INTERLEAVE_SET:-off on})
# GPU modes for informational comparisons (requires GPU-capable build/hardware)
GPU_SET=(${GPU_SET:-off})
SCENARIOS=(${SCENARIOS:-many-small many-large single mixed})

# Scenario sizes (override via env): default is modest, adjust locally as needed
SMALL_COUNT=${SMALL_COUNT:-1000}      # many-small number of files
SMALL_SIZE=${SMALL_SIZE:-4096}        # bytes per small file
LARGE_COUNT=${LARGE_COUNT:-4}         # many-large files count
LARGE_SIZE=${LARGE_SIZE:-16777216}    # 16 MiB per large file
SINGLE_SIZE=${SINGLE_SIZE:-67108864}  # 64 MiB single file
MIXED_SMALL=${MIXED_SMALL:-200}       # mixed: small files count
MIXED_LARGE=${MIXED_LARGE:-2}         # mixed: large files count

ts_ms() {
  # millisecond timestamp
  if date +%s%3N >/dev/null 2>&1; then
    date +%s%3N
  else
    python3 - <<'PY'
import time
print(int(time.time()*1000))
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
      # a few medium files
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
      # delete ~1% of files
      find "$root/s" -type f | shuf -n $((SMALL_COUNT/100 + 1)) | xargs -r rm -f --
      ;;
    many-large)
      # delete one large file
      rm -f -- "$(find "$root/l" -type f | head -n1)"
      ;;
    single)
      # flip random 4 KiB region to simulate corruption
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
      # delete a handful of small files
      find "$root/s" -type f | shuf -n $((MIXED_SMALL/50 + 1)) | xargs -r rm -f --
      ;;
  esac
}

emit_result() {
  local obj="$1"; echo "$obj" >>"$RESULTS"
}

echo "Writing results to: $RESULTS"
# Emit environment meta line
scripts/bench_env_info.sh >> "$RESULTS" || true

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

          # Parity bytes on disk
          PARITY_BYTES=$(find "$OUT" -maxdepth 1 -name 'vol-*.parxv' -printf '%s\n' | awk '{s+=$1} END{print s+0}')

          # Damage and repair
          damage_dataset "$scenario" "$ROOT"
          t2=$(ts_ms)
          RJSON=$(cargo run -q -p parx-cli -- repair --json "$OUT/manifest.json" "$ROOT" || true)
          t3=$(ts_ms)
          rep_ms=$((t3 - t2))
          repaired=$(echo "$RJSON" | jq -r '.repaired_chunks // 0' 2>/dev/null || echo 0)
          failed=$(echo "$RJSON" | jq -r '.failed_chunks // 0' 2>/dev/null || echo 0)

          POST_HASH=$(cargo run -q -p parx-cli -- hashcat --hash-only "$ROOT")
          ok=$([[ "$BASE_HASH" == "$POST_HASH" ]] && echo true || echo false)

          emit_result "$(jq -n \
            --arg scenario "$scenario" \
            --argjson k "$K" \
            --argjson parity_pct "$PP" \
            --argjson chunk "$CHUNK" \
            --arg interleave "$IL" \
            --arg gpu_mode "$GPU" \
            --arg base_hash "$BASE_HASH" \
            --arg post_hash "$POST_HASH" \
            --argjson total_bytes "$TOTAL_BYTES" \
            --argjson parity_bytes "$PARITY_BYTES" \
            --argjson encode_ms "$enc_ms" \
            --argjson repair_ms "$rep_ms" \
            --argjson repaired_chunks "$repaired" \
            --argjson failed_chunks "$failed" \
            --arg root "$ROOT" --arg out "$OUT" \
            '{type:"result", ts: now, scenario: $scenario, k: $k, parity_pct: $parity_pct, chunk: $chunk, interleave: $interleave, gpu: $gpu_mode, total_bytes: $total_bytes, parity_bytes: $parity_bytes, encode_ms: $encode_ms, repair_ms: $repair_ms, repaired_chunks: $repaired_chunks, failed_chunks: $failed_chunks, ok: ($base_hash==$post_hash), root: $root, out: $out, base_hash: $base_hash, post_hash: $post_hash}' )"
          echo "[${scenario}] K=$K P=$PP C=$CHUNK IL=$IL GPU=$GPU => ok=$ok enc=${enc_ms}ms rep=${rep_ms}ms"
          done
        done
    done
  done
done

echo "Done. Results: $RESULTS"
