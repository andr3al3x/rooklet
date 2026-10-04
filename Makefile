.PHONY: check test release demo install package
check:
	cargo fmt --check
	cargo check --locked --all-targets
	cargo clippy --locked --all-targets -- -D warnings
test:
	cargo test --locked
release:
	cargo build --release --locked
demo:
	cargo run -- --demo
install:
	cargo install --path . --locked

package:
	sh scripts/package.sh
