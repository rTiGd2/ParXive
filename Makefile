.PHONY: all debug release fmt clippy test bench-local bench-matrix bench-html bench-pack
all: release
debug: ; cargo build
release: ; cargo build --release
fmt: ; cargo fmt
clippy: ; cargo clippy -- -D warnings
test: ; cargo test -- --nocapture

# Local-only quick smoke benchmark (heavy): refuses to run in CI
bench-local:
	./scripts/bench_repair_smoke.sh || true

# Local-only full matrix benchmark (heavy): refuses to run in CI
bench-matrix:
	./scripts/bench_matrix_local.sh || true

# Convert latest JSONL results to HTML (local)
bench-html:
	python3 scripts/bench_to_html.py "$$(ls -1t _tgt/bench-results/bench-*.jsonl | head -n1)" _tgt/bench-results/summary.html

# Package benchmark scripts (no binaries)
bench-pack:
	./scripts/bench_pack_local.sh
