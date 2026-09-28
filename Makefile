.PHONY: build test install uninstall

build:
	cargo build --release

test:
	cargo test --release

install:
	cargo install --path . --locked

uninstall:
	cargo uninstall nib
