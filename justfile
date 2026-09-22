build: console-build
    rustup run stable cargo build --locked --release

verify:
    rustup run stable cargo fmt --check
    rustup run stable cargo check --locked
    rustup run stable cargo test --locked
    rustup run stable cargo clippy --locked --all-targets -- -D warnings

clean:
    rustup run stable cargo clean

# Developer-only fresh profile probe, not an identity migration.
profile-probe root:
    rustup run stable cargo run --example profile_probe -- '{{root}}'

# Build assets in the exact Git dependency Cargo resolved, before Rust embeds them.
console-build:
    #!/usr/bin/env bash
    set -euo pipefail
    ui=$(rustup run stable cargo metadata --locked --format-version 1 | python3 -c 'import json,sys,pathlib; print(pathlib.Path(next(p["manifest_path"] for p in json.load(sys.stdin)["packages"] if p["name"] == "mesh-llm-ui")).parent)')
    MESH_LLM_BUILD_PROFILE=release "$ui/../../scripts/build-ui.sh" "$ui"
