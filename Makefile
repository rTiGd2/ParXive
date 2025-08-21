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

# Collate all JSONL files into one and produce an HTML summary
bench-collate-html:
	@set -e; \
	OUTDIR=_tgt/bench-results; mkdir -p $$OUTDIR; \
	STAMP=$$(date +%Y%m%d-%H%M%S); \
	OUTJSON=$$OUTDIR/all-$$STAMP.jsonl; \
	python3 scripts/bench_collate.py $$OUTJSON $$(ls -1 $$OUTDIR/bench-*.jsonl 2>/dev/null) && \
	python3 scripts/bench_to_html.py $$OUTJSON $$OUTDIR/summary_all.html && \
	echo "Combined summary: $$OUTDIR/summary_all.html"
