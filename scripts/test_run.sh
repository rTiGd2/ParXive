#!/usr/bin/env bash
set -euo pipefail

# ParXive black-box test harness
# - Generates datasets (single/small/many-medium/mixed)
# - Runs create -> corrupt -> repair -> verify using the parx CLI
# - Logs JSONL results and writes a summary
#
# Usage examples:
#   ./scripts/test_run.sh --scenario single --k 8 --pct 50 --chunk 1048576 --interleave off
#   ./scripts/test_run.sh --scenarios all --runs 10
#   ./scripts/test_run.sh --scenarios small,mixed --build debug --runs 5 --preserve

SCENARIOS=""
RUNS=1
BUILD=release   # or debug
THREADS="auto" # or an integer
PRESERVE=false
K_SET="4 8 16"
PCT_SET="25 50"
CHUNK_SET="4096 65536 1048576"
IL_SET="off on"
OUT_ROOT="_tgt/tests"

die() { echo "error: $*" >&2; exit 1; }

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --scenario) SCENARIOS="$2"; shift 2;;
      --scenarios) SCENARIOS="$2"; shift 2;;
      --runs) RUNS="$2"; shift 2;;
      --build) BUILD="$2"; shift 2;;
      --preserve) PRESERVE=true; shift 1;;
      --threads) THREADS="$2"; shift 2;;
      --k) K_SET=$(echo "$2" | tr ',' ' '); shift 2;;
      --pct) PCT_SET=$(echo "$2" | tr ',' ' '); shift 2;;
      --chunk) CHUNK_SET=$(echo "$2" | tr ',' ' '); shift 2;;
      --interleave) IL_SET=$(echo "$2" | tr ',' ' '); shift 2;;
      --out) OUT_ROOT="$2"; shift 2;;
      -h|--help)
        cat <<EOF
Usage: $0 [options]
  --scenario/--scenarios  single,small,many-medium,mixed or all (default: all)
  --runs N                repeat each tuple N times (default: 1)
  --build release|debug   CLI build profile (default: release)
  --k "4,8,16"            stripe K set (default: 4,8,16)
  --pct "25,50"           parity percent set (default: 25,50)
  --chunk "4096,65536,1048576" chunk sizes (default)
  --interleave "off,on"    interleave flag set (default)
  --preserve              keep generated data/artifacts
  --out DIR               output root (default: _tgt/tests)
EOF
        exit 0;;
      *) die "unknown arg: $1";;
    esac
  done
}

parse_args "$@"

# Resolve scenarios
if [[ -z "${SCENARIOS}" || "${SCENARIOS}" == "all" ]]; then
  SCENARIOS="single,small,many-medium,mixed"
fi
IFS=',' read -r -a SCN_ARR <<<"$SCENARIOS"

TS=$(date +%Y%m%d-%H%M%S)
OUT_DIR="${OUT_ROOT}/${TS}"
LOG="${OUT_DIR}/results.jsonl"
ART="${OUT_DIR}/artifacts"
mkdir -p "$OUT_DIR" "$ART"

# Build selected profile and resolve parx binary
if [[ "$BUILD" == "release" ]]; then
  cargo build -q --release -p parx-cli
  PARX="$(pwd)/target/release/parx"
elif [[ "$BUILD" == "debug" ]]; then
  cargo build -q -p parx-cli
  PARX="$(pwd)/target/debug/parx"
else
  die "--build must be release or debug"
fi
[[ -x "$PARX" ]] || die "parx binary not found at $PARX"

echo "# Harness start: $(date)" | tee "${OUT_DIR}/console.log" >&2
echo "# Profile: $BUILD  Scenarios: ${SCENARIOS}  Runs: ${RUNS}" | tee -a "${OUT_DIR}/console.log" >&2

# Dataset generators (fast, deterministic IO to avoid generation bottlenecks)
gen_single() { # 400 MiB
  local root="$1"; mkdir -p "$root"; dd if=/dev/zero of="$root/single.bin" bs=1M count=400 status=none || true
}
gen_small() { # 20 MiB
  local root="$1"; mkdir -p "$root"; dd if=/dev/zero of="$root/small.bin" bs=1M count=20 status=none || true
}
gen_many_medium() { # 50 x 16 MiB
  local root="$1"; mkdir -p "$root/med"; for i in $(seq 1 50); do dd if=/dev/zero of="$root/med/m$(printf %03d $i).bin" bs=1M count=16 status=none || true; done
}
gen_mixed() { # small+medium+one large
  local root="$1"; mkdir -p "$root/s" "$root/m" "$root/l";
  for i in $(seq 1 100); do dd if=/dev/zero of="$root/s/s$(printf %03d $i).bin" bs=1M count=1 status=none || true; done
  for i in $(seq 1 10); do dd if=/dev/zero of="$root/m/m$(printf %03d $i).bin" bs=1M count=8 status=none || true; done
  dd if=/dev/zero of="$root/l/L.bin" bs=1M count=128 status=none || true
}

corrupt_one() {
  local file="$1"; python3 - "$file" <<'PY'
import os,sys
p=sys.argv[1]
sz=os.path.getsize(p)
off=max(0,(sz//2)-2048)
with open(p,'r+b') as f:
    f.seek(off); f.write(os.urandom(4096))
PY
}

run_tuple() {
  local scn="$1"; local k="$2"; local pct="$3"; local chunk="$4"; local il="$5"; local runn="$6"
  local root="${OUT_DIR}/root-${scn}-${k}-${pct}-${chunk}-${il}-${runn}"
  local outp="${OUT_DIR}/out-${scn}-${k}-${pct}-${chunk}-${il}-${runn}"
  rm -rf "$root" "$outp"; mkdir -p "$root" "$outp"
  case "$scn" in
    single) gen_single "$root";;
    small) gen_small "$root";;
    many-medium) gen_many_medium "$root";;
    mixed) gen_mixed "$root";;
    *) die "unknown scenario $scn";;
  esac
  local ilopt=""; [[ "$il" == "on" ]] && ilopt="--interleave-files"
  local thopt=""; if [[ "$THREADS" == "auto" ]]; then thopt="--threads $(nproc)"; else thopt="--threads $THREADS"; fi
  local ok=true stage=""
  # Create
  if ! "$PARX" create $thopt --parity "$pct" --stripe-k "$k" --chunk-size "$chunk" $ilopt --output "$outp" --volume-sizes 64M,64M,64M "$root" >/dev/null 2>"$ART/create-${scn}-${k}-${pct}-${chunk}-${il}-${runn}.stderr"; then
    ok=false; stage=create
  fi
  # Baseline hash before any corruption
  local BH=""
  if $ok; then
    BH=$("$PARX" hashcat --hash-only "$root" 2>/dev/null || true)
  fi
  # Corrupt
  if $ok; then
    # pick a file deterministically if many
    tgt=$(find "$root" -type f | sort | head -n1 || true)
    [[ -n "$tgt" ]] && corrupt_one "$tgt"
  fi
  # Repair
  if $ok; then
    RUST_BACKTRACE=1 "$PARX" repair $thopt --json "$outp/manifest.json" "$root" >"$ART/repair-${scn}-${k}-${pct}-${chunk}-${il}-${runn}.json" 2>"$ART/repair-${scn}-${k}-${pct}-${chunk}-${il}-${runn}.stderr" || { ok=false; stage=repair; }
  fi
  # Verify by hash (compare to baseline)
  if $ok; then
    PH=$("$PARX" hashcat --hash-only "$root" 2>/dev/null || true)
    [[ -n "$BH" && -n "$PH" && "$BH" == "$PH" ]] || { ok=false; stage=hash; }
  fi
  OK_VAL="$ok" profile="$BUILD" scenario="$scn" K="$k" PCT="$pct" CHUNK="$chunk" IL="$il" RUN="$runn" TS="$TS" stage="$stage" \
  python3 - <<'PY' >>"$LOG"
import json,os
ok = os.environ.get('OK_VAL','false').lower()=='true'
out = dict(ts=os.environ.get('TS'), profile=os.environ.get('profile'), scenario=os.environ.get('scenario'),
           k=int(os.environ['K']), pct=int(os.environ['PCT']), chunk=int(os.environ['CHUNK']), il=os.environ['IL'],
           run=int(os.environ['RUN']), ok=ok, stage=os.environ.get('stage',''))
print(json.dumps(out))
PY
  if [[ "$PRESERVE" != true ]]; then rm -rf "$root" "$outp"; fi
}

for scn in "${SCN_ARR[@]}"; do
  for r in $(seq 1 "$RUNS"); do
    for K in $K_SET; do
      for P in $PCT_SET; do
        for C in $CHUNK_SET; do
          for IL in $IL_SET; do
            echo "RUN: scn=$scn k=$K pct=$P chunk=$C il=$IL r=$r" | tee -a "${OUT_DIR}/console.log" >&2
            if ! run_tuple "$scn" "$K" "$P" "$C" "$IL" "$r"; then
              echo "tuple failed: scn=$scn k=$K pct=$P chunk=$C il=$IL r=$r" | tee -a "${OUT_DIR}/console.log" >&2
            fi
          done
        done
      done
    done
  done
done

# Write a quick summary (jq if available)
if command -v jq >/dev/null 2>&1; then
  jq -s 'group_by([.scenario,.k,.pct,.chunk,.il]) | map({scenario:.[0].scenario,k:.[0].k,pct:.[0].pct,chunk:.[0].chunk,il:.[0].il,runs:length,passes:(map(select(.ok))|length),fails:(map(select(.ok|not))|length)}) | sort_by(-.fails)' "$LOG" >"${OUT_DIR}/summary.json" || true
fi
echo "Done. Log: $LOG" | tee -a "${OUT_DIR}/console.log" >&2


