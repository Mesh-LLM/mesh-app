build:
    rustup run stable cargo build --release

verify:
    rustup run stable cargo fmt --check
    rustup run stable cargo check
    rustup run stable cargo test
    rustup run stable cargo clippy --all-targets -- -D warnings

clean:
    rustup run stable cargo clean

# Developer-only fresh profile probe, not an identity migration.
profile-probe root:
    rustup run stable cargo run --example profile_probe -- '{{root}}'

# Fresh app profiles only; bounded full-node admission probe, no model download.
released-pool-probe binary root:
    rustup run stable cargo run --example released_pool_probe -- '{{binary}}' '{{root}}'
