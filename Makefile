# Build the Rust dev-tools and link the binaries into bin/ (for local use).
#
#   make build   release build, link into bin/
#   make dev     fast debug build, linked into bin/ (the dev loop)
#   make test    cargo test across the workspace
#   make fmt     cargo fmt
#   make clippy  cargo clippy with warnings denied

# Binary crates that produce a CLI. Add new tools here.
TOOLS := aws-switch feature wt-gc

BIN_DIR := $(abspath bin)

.PHONY: build dev test fmt clippy clean

build:
	cargo build --release
	@mkdir -p "$(BIN_DIR)"
	@for t in $(TOOLS); do \
		ln -sf "$(abspath target/release)/$$t" "$(BIN_DIR)/$$t"; \
		echo "    linked $(BIN_DIR)/$$t -> target/release/$$t"; \
	done

dev:
	cargo build
	@mkdir -p "$(BIN_DIR)"
	@for t in $(TOOLS); do \
		ln -sf "$(abspath target/debug)/$$t" "$(BIN_DIR)/$$t"; \
		echo "    linked $(BIN_DIR)/$$t -> target/debug/$$t"; \
	done

test:
	cargo test

fmt:
	cargo fmt

clippy:
	cargo clippy --all-targets -- -D warnings

clean:
	cargo clean
