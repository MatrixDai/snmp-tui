.PHONY: all build test clippy fmt-check

all: fmt-check clippy test build

build:
	cargo build

test:
	RUST_MIN_STACK=8388608 cargo test

clippy:
	cargo clippy -- -D warnings

fmt-check:
	cargo fmt --check
