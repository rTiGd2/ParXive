#!/usr/bin/env bash
set -euo pipefail

# Emit a single JSON object with host/hardware/runtime info.

HOST=$(hostname || echo unknown)
OS=$(uname -s || echo unknown)
KERNEL=$(uname -r || echo unknown)
ARCH=$(uname -m || echo unknown)

CPU_MODEL=$(lscpu 2>/dev/null | awk -F: '/Model name/ {print trim($2)} function trim(s){gsub(/^ +| +$/,"",s);print s}' | head -n1 || true)
if [[ -z "${CPU_MODEL:-}" ]]; then
  CPU_MODEL=$(grep -m1 'model name' /proc/cpuinfo 2>/dev/null | cut -d: -f2- | sed 's/^ //')
fi
CPU_CORES=$(nproc --all 2>/dev/null || getconf _NPROCESSORS_ONLN 2>/dev/null || echo 0)

MEM_TOTAL=$(awk '/MemTotal/ {print $2*1024}' /proc/meminfo 2>/dev/null || echo 0)

GPU_INFO=""
if command -v nvidia-smi >/dev/null 2>&1; then
  GPU_INFO=$(nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader 2>/dev/null | paste -sd'|' -)
fi

RUSTC=$(rustc --version 2>/dev/null || echo "")
CARGO=$(cargo --version 2>/dev/null || echo "")
PARX_VER=$(cargo run -q -p parx-cli -- --version 2>/dev/null || echo "parx")
GIT_COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "")
GIT_BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "")

jq -n \
  --arg host "$HOST" \
  --arg os "$OS" \
  --arg kernel "$KERNEL" \
  --arg arch "$ARCH" \
  --arg cpu_model "$CPU_MODEL" \
  --argjson cpu_cores "${CPU_CORES:-0}" \
  --argjson mem_total_bytes "${MEM_TOTAL:-0}" \
  --arg gpu "$GPU_INFO" \
  --arg rustc "$RUSTC" \
  --arg cargo "$CARGO" \
  --arg parx "$PARX_VER" \
  --arg git_commit "$GIT_COMMIT" \
  --arg git_branch "$GIT_BRANCH" \
  '{type:"meta", host:$host, os:$os, kernel:$kernel, arch:$arch, cpu_model:$cpu_model, cpu_cores:$cpu_cores, mem_total_bytes:$mem_total_bytes, gpu:$gpu, rustc:$rustc, cargo:$cargo, parx:$parx, git_commit:$git_commit, git_branch:$git_branch, ts: now}'

