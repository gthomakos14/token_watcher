UUID = antigravity-token-watcher@gthomakos14.github.com
INSTALL_DIR = $(HOME)/.local/share/gnome-shell/extensions/$(UUID)

.PHONY: all build install enable disable clean test status bench bench-local bench-network

all: build

build:
	cargo build --release
	mkdir -p extension/bin
	cp target/release/token-watcher extension/bin/token-watcher
	glib-compile-schemas extension/schemas/

install: build
	mkdir -p $(INSTALL_DIR)
	cp -r extension/* $(INSTALL_DIR)/
	glib-compile-schemas $(INSTALL_DIR)/schemas/
	@echo "Extension installed to $(INSTALL_DIR)"
	@echo "Run 'make enable' or restart GNOME Shell (Alt+F2 -> r on X11, or log out/in on Wayland) if enabling for the first time."

enable:
	gnome-extensions enable $(UUID)
	@echo "Extension $(UUID) enabled!"

disable:
	gnome-extensions disable $(UUID)
	@echo "Extension $(UUID) disabled."

test:
	cargo test
	node --test extension-tests/*.test.mjs
	cargo run -- --json

# Benchmarks (require hyperfine: cargo install hyperfine). Results are written to bench/.
BIN = target/release/token-watcher
DB = $(HOME)/.config/Antigravity/User/globalStorage/state.vscdb

bench: bench-local bench-network

# Local paths: cache hits and the SQLite reader. --ttl 3600 keeps every run a cache hit.
bench-local: build
	mkdir -p bench
	$(BIN) --json --provider claude >/dev/null
	$(BIN) --json --provider antigravity >/dev/null
	hyperfine -N --warmup 20 --min-runs 200 --export-markdown bench/local.md \
		-n 'auto-select, cache hit' '$(BIN) --json --ttl 3600' \
		-n 'claude, cache hit' '$(BIN) --json --provider claude --ttl 3600' \
		-n 'antigravity, cache hit' '$(BIN) --json --provider antigravity --ttl 3600' \
		-n 'sqlite (--db)' '$(BIN) --json --db $(DB)'

# Fresh fetches call the claude and agy CLIs over the network, so keep the run count low.
bench-network: build
	mkdir -p bench
	hyperfine -N --runs 5 --export-markdown bench/network.md \
		-n 'claude, fresh fetch' '$(BIN) --json --provider claude --refresh' \
		-n 'antigravity, fresh fetch' '$(BIN) --json --provider antigravity --refresh'

status:
	cargo run --release --

clean:
	cargo clean
	rm -rf extension/bin
	rm -f extension/schemas/gschemas.compiled
