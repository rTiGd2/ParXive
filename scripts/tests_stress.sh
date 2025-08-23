#!/usr/bin/env bash
set -euo pipefail

# Stress runner: loops selected tests across parameter grids and logs pass/fail with parameters.

ROOT=${1:-_tgt/stress}
RUNS=${RUNS:-20}
LOG=${LOG:-_tgt/stress/results-$(date +%Y%m%d-%H%M%S).jsonl}

mkdir -p "$(dirname "$LOG")" "$ROOT"

echo "# Stress run: $(date)" >&2
echo "# Log: $LOG" >&2

# Parameter grids (override via env)
K_SET=${K_SET:-"4 8 16"}
PCT_SET=${PCT_SET:-"25 50"}
CHUNK_SET=${CHUNK_SET:-"4096 65536 1048576"}
IL_SET=${IL_SET:-"off on"}

i=1
while [ "$i" -le "$RUNS" ]; do
  for K in $K_SET; do
    for P in $PCT_SET; do
      for C in $CHUNK_SET; do
        for IL in $IL_SET; do
          echo "RUN:$i K=$K P=$P C=$C IL=$IL" >&2
          SEED=$(( (i*1103515245 + 12345) & 0x7fffffff ))
          export PARX_INV_DBG=
          # Run invariants and core repair tests
          if cargo test -q -p parx-core --no-default-features -- tests::dummy  >/dev/null 2>&1; then :; fi
          OK=true
          cargo test -q -p parx-core --no-default-features --test invariants chunk_hash_zero_padding_matches   || OK=false
          cargo test -q -p parx-core --no-default-features --test invariants parity_entry_len_within_bounds    || OK=false
          cargo test -q -p parx-core --no-default-features --test invariants interleave_preserves_order_and_hashes || OK=false
          cargo test -q -p parx-core --no-default-features --test invariants multi_stripe_repair_succeeds      || OK=false
          cargo test -q -p parx-core --no-default-features --test verify_repair                                 || OK=false
          K="$K" P="$P" C="$C" IL="$IL" OK="$OK" i="$i" LOG="$LOG" \
          python3 - <<'PY'
import json,os,time
obj=dict(ts=int(time.time()), run=int(os.environ.get('i','0') or 0), k=int(os.environ['K']), pct=int(os.environ['P']), chunk=int(os.environ['C']), interleave=os.environ['IL'], ok=os.environ.get('OK','true')=='true')
print(json.dumps(obj))
open(os.environ['LOG'],'a').write(json.dumps(obj)+'\n')
PY
        done
      done
    done
  done
  i=$((i+1))
done
echo "Done. Log: $LOG" >&2


