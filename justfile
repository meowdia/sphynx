# SPDX-FileCopyrightText: 2026 Meowdia Community
# SPDX-License-Identifier: MIT OR Apache-2.0

_default:
    just --list

build:
    cargo build

test:
    cargo test

lint:
    cargo clippy
    cargo fmt --check

reuse:
    reuse lint

check: lint build test reuse

clean:
    cargo clean

fmt:
    cargo fmt
