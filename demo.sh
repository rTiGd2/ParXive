#!/usr/bin/env bash
# ParX torture-test demo
# - Builds parx
# - Generates sample data (40 MiB total by default)
# - Creates a 35% parity set with K=64 (3 volumes, round-robin)
# - Runs multiple corruption scenarios and validates repairability end-to-end

set -euo pipefail

# ---------- tunables ----------
K=64                  # data shards per stripe (max per stripe; last stripe may be shorter)
PARITY_PCT=35         # % parity per stripe
CHUNK_SIZE=$((1<<20)) # 1 MiB chunks
VOLUME_SIZES="32M,32M,32M"  # ensure 3 volumes with ample capacity
GPU="off"             # auto|on|off (lowercase to match CLI)
DATA_DIR="demo_data"  # source files dir
PARX_DIR=".parx"      # parity output dir
VERIFY_ROOT="."       # IMPORTANT: manifest paths are relative to project root
BIN="./target/release/parx"
# ------------------------------

cecho(){ # colored echo
  local c="$1"; shift
  case "$c" in
    red)    printf "\033[31m%s\033[0m\n" "$*";;
    green)  printf "\033[32m%s\033[0m\n" "$*";;
    yellow) printf "\033[33m%s\033[0m\n" "$*";;
    blue)   printf "\033[34m%s\033[0m\n" "$*";;
    *)      printf "%s\n" "$*";;
  esac
}
step(){ cecho blue "==> $*"; }
ok(){ cecho green "✔ $*"; }
warn(){ cecho yellow "⚠ $*"; }
fail(){ cecho red "✖ $*"; exit 1; }

require_tools(){
  command -v cargo >/dev/null || fail "cargo not found"
  command -v dd >/dev/null || fail "dd not found"
  command -v stat >/dev/null || fail "stat not found"
}

build_bin(){
  if [[ ! -x "$BIN" ]]; then
    step "Building parx (release)…"
    cargo build --release
  else
    step "parx binary already built."
  fi
  ok "parx ready: $BIN"
}

clean_env(){
  step "Cleaning old artifacts…"
  rm -rf "$DATA_DIR" "$PARX_DIR"
  ok "Clean."
}

gen_sample_data(){
  step "Generating sample data in $DATA_DIR…"
  mkdir -p "$DATA_DIR"
  # 17 + 12 + 11 = 40 MiB → 40 chunks at 1 MiB
  dd if=/dev/urandom of="$DATA_DIR/fileA.bin" bs=1M count=17 status=none
  dd if=/dev/urandom of="$DATA_DIR/fileB.bin" bs=1M count=12 status=none
  dd if=/dev/urandom of="$DATA_DIR/fileC.bin" bs=1M count=11 status=none
  ok "Sample data generated (total ~40 MiB)."
}

create_parity(){
  step "Creating parity (K=$K, ${PARITY_PCT}%, chunk=$((CHUNK_SIZE/1024/1024))MiB, volumes=3)…"
  "$BIN" create \
    --parity "$PARITY_PCT" \
    --stripe-k "$K" \
    --chunk-size "$CHUNK_SIZE" \
    --output "$PARX_DIR" \
    --volume-sizes "$VOLUME_SIZES" \
    --gpu "$GPU" \
    --progress \
    "$DATA_DIR"
  ok "Parity created under $PARX_DIR"
}

quickcheck(){
  step "Quickcheck volumes…"
  set +e
  "$BIN" quickcheck "$PARX_DIR" || true
  set -e
}

verify_all(){
  step "Verifying source against manifest…"
  "$BIN" verify "$PARX_DIR/manifest.json" "$VERIFY_ROOT"
}

audit_all(){
  step "Auditing damage by stripe…"
  "$BIN" audit "$PARX_DIR/manifest.json" "$VERIFY_ROOT"
}

paritycheck(){
  step "Parity-aware audit…"
  set +e
  "$BIN" paritycheck "$PARX_DIR" || true
  set -e
}

# Corrupt N random 4 KiB pages inside the data files
corrupt_data_chunks(){
  local n="${1:-5}"
  cecho yellow "-- Corrupting $n random 4 KiB pages in data files --"
  local files=("$DATA_DIR"/*)
  local i file size blocks off_blocks
  for ((i=0;i<n;i++)); do
    file="${files[$RANDOM % ${#files[@]}]}"
    size=$(stat -c '%s' "$file")
    if (( size < 8192 )); then continue; fi
    # pick an offset aligned to 4 KiB but avoid file tail by 4 KiB
    blocks=$(( (size - 4096) / 4096 ))
    off_blocks=$(( blocks > 0 ? RANDOM % blocks : 0 ))
    dd if=/dev/urandom of="$file" bs=4096 seek="$off_blocks" count=1 conv=notrunc status=none
  done
  ok "Data corruption injected."
}

# Corrupt parity volume bytes *not* at the very end (to avoid trailer)
corrupt_parity_data(){
  local vol="$1"
  [[ -f "$vol" ]] || { warn "$vol missing"; return; }
  local size
  size=$(stat -c '%s' "$vol")
  if (( size < 4*1024*1024 )); then
    warn "$vol too small to safely corrupt (size=$size)"; return
  fi
  cecho yellow "-- Corrupting parity DATA region in $(basename "$vol") --"
  local off=$(( size/3 ))           # somewhere in the front-middle
  local seek_blocks=$(( off / 65536 ))
  dd if=/dev/urandom of="$vol" bs=65536 seek="$seek_blocks" count=2 conv=notrunc status=none
  ok "Parity data scrambled in $(basename "$vol")."
}

# Corrupt parity *trailer* (index) by overwriting last 16 KiB
corrupt_parity_index(){
  local vol="$1"
  [[ -f "$vol" ]] || { warn "$vol missing"; return; }
  local size
  size=$(stat -c '%s' "$vol")
  if (( size < 32768+4 )); then
    warn "$vol too small to corrupt trailer safely"; return
  fi
  cecho yellow "-- Corrupting parity INDEX (trailer) in $(basename "$vol") --"
  local tail_k=16
  local seek_blocks=$(( (size - tail_k*1024) / 1024 ))
  dd if=/dev/urandom of="$vol" bs=1024 seek="$seek_blocks" count="$tail_k" conv=notrunc status=none
  ok "Parity index (trailer) scrambled in $(basename "$vol")."
}

delete_one_parity_volume(){
  local vol="$1"
  if [[ -f "$vol" ]]; then
    cecho yellow "-- Deleting parity volume $(basename "$vol") --"
    rm -f "$vol"
    ok "Deleted $(basename "$vol")."
  else
    warn "$vol not found (nothing to delete)."
  fi
}

show_state(){
  step "Current volumes:"
  ls -lh "$PARX_DIR"/vol-*.parxv 2>/dev/null || true
  paritycheck
  quickcheck
  verify_all
}

# ---------- run ----------
require_tools
build_bin
clean_env
gen_sample_data
create_parity
show_state

# Torture 1: corrupt 5 data pages → audit → repair → verify
step "TORTURE 1: Data corruption, then repair"
corrupt_data_chunks 5
audit_all
step "Repairing…"
"$BIN" repair "$PARX_DIR/manifest.json" "$VERIFY_ROOT"
verify_all
ok "Torture 1 complete."

# Torture 2: delete one parity volume, then corrupt 3 data pages → repair → verify
step "TORTURE 2: Delete one parity volume, then repair data"
# Pick a middle volume if present, else the first found
VOL_TO_DELETE="$(ls "$PARX_DIR"/vol-*.parxv 2>/dev/null | sort | sed -n '2p' || true)"
[[ -z "${VOL_TO_DELETE:-}" ]] && VOL_TO_DELETE="$(ls "$PARX_DIR"/vol-*.parxv 2>/dev/null | head -n1 || true)"
delete_one_parity_volume "${VOL_TO_DELETE:-/nonexistent}"
show_state
corrupt_data_chunks 3
audit_all
step "Repairing…"
"$BIN" repair "$PARX_DIR/manifest.json" "$VERIFY_ROOT"
verify_all
ok "Torture 2 complete."

# Torture 3: corrupt parity DATA (not trailer) on one remaining volume → ensure repair still works
step "TORTURE 3: Corrupt parity DATA region on a volume"
VOL_FOR_DATA_CORRUPT="$(ls "$PARX_DIR"/vol-*.parxv 2>/dev/null | head -n1 || true)"
corrupt_parity_data "${VOL_FOR_DATA_CORRUPT:-/nonexistent}"
show_state
# Damage data again a bit and ensure we can still repair
corrupt_data_chunks 2
audit_all
step "Repairing…"
"$BIN" repair "$PARX_DIR/manifest.json" "$VERIFY_ROOT"
verify_all
ok "Torture 3 complete."

# Torture 4: corrupt parity INDEX (trailer) on another volume → ensure tool degrades gracefully
step "TORTURE 4: Corrupt parity INDEX (trailer) on a volume"
VOL_FOR_INDEX_CORRUPT="$(ls "$PARX_DIR"/vol-*.parxv 2>/dev/null | tail -n1 || true)"
# Avoid using the same volume twice if only one left
if [[ "${VOL_FOR_INDEX_CORRUPT:-}" == "${VOL_FOR_DATA_CORRUPT:-}" ]]; then
  VOL_FOR_INDEX_CORRUPT="$(ls "$PARX_DIR"/vol-*.parxv 2>/dev/null | head -n1 || true)"
fi
corrupt_parity_index "${VOL_FOR_INDEX_CORRUPT:-/nonexistent}"
paritycheck
quickcheck
# Damage data again—repair should still work if remaining parity is enough and indexes readable
corrupt_data_chunks 2
audit_all
step "Repairing…"
"$BIN" repair "$PARX_DIR/manifest.json" "$VERIFY_ROOT" || warn "Repair could not proceed (insufficient usable parity or indexes)."
verify_all

ok "All torture tests completed."

