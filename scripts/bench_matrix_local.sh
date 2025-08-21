#!/usr/bin/env bash
set -euo pipefail
# Thin wrapper delegating to the simplified, ShellCheck-friendly runner.
exec bash "$(dirname "$0")/bench_matrix_simple.sh" "$@"
