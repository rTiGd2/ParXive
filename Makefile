.PHONY: all debug release fmt clippy test
all: release
debug: ; cargo build
release: ; cargo build --release
fmt: ; cargo fmt
clippy: ; cargo clippy -- -D warnings
test: ; cargo test -- --nocapture

