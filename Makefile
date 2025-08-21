.PHONY: all debug release fmt clippy test bench-local bench-matrix
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
