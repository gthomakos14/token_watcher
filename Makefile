UUID = antigravity-token-watcher@github.com/gthomakos14
INSTALL_DIR = $(HOME)/.local/share/gnome-shell/extensions/$(UUID)

.PHONY: all build install enable disable clean test status

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
	cargo run -- --json

status:
	cargo run --release --

clean:
	cargo clean
	rm -rf extension/bin
	rm -f extension/schemas/gschemas.compiled
