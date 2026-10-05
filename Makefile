.PHONY: check test release install uninstall package
BIN_DIR ?= $(HOME)/.local/bin
TARGET ?=

check:
	cargo fmt --check
	cargo check --locked --all-targets
	cargo clippy --locked --all-targets -- -D warnings
	for script_path in scripts/*.sh; do sh -n "$$script_path" || exit; done
test:
	cargo test --locked
release:
	sh scripts/build-release.sh $(if $(TARGET),--target "$(TARGET)")
install:
	binary_path=$$(sh scripts/build-release.sh) && sh scripts/install.sh --binary "$$binary_path" --bin-dir "$(BIN_DIR)"
uninstall:
	sh scripts/uninstall.sh --bin-dir "$(BIN_DIR)"
package:
	sh scripts/package.sh $(if $(TARGET),--target "$(TARGET)")
