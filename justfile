default: check

check:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo test

test:
    cargo test

lint:
    cargo clippy --all-targets -- -D warnings

run:
    cargo run

fmt:
    cargo fmt
