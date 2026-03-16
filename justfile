_default:
    just --list

build:
    cargo build

test:
    cargo test

lint:
    cargo clippy
    cargo fmt --check

check: lint build test

clean:
    cargo clean

fmt:
    cargo fmt
