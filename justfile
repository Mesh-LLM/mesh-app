build:
    rustup run stable cargo build --release

verify:
    rustup run stable cargo fmt --check
    rustup run stable cargo check
    rustup run stable cargo test
    rustup run stable cargo clippy --all-targets -- -D warnings

clean:
    rustup run stable cargo clean
