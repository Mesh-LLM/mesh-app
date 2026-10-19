# Separate builds: building both together unifies features, and GPUI's would
# link Metal into the tray (rejected by the engine host-dependency policy).
build: console-build
    rustup run stable cargo build --locked --release -p mesh-tray
    rustup run stable cargo build --locked --release -p mesh-panel

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

# Local Mesh.app with the Metal runtime built from the pinned engine commit,
# the same inputs CI's `runtime=source` path uses. Ad-hoc signed, not notarized.
# Needs: cmake, ninja, lld (brew install cmake ninja lld). First runtime build is slow.
engine_dir := "target/engine-src"

engine-src:
    #!/usr/bin/env bash
    set -euo pipefail
    sha=$(python3 scripts/release-inputs.py)
    if [ ! -d {{engine_dir}}/.git ]; then
        git clone --filter=blob:none https://github.com/Mesh-LLM/mesh-llm.git {{engine_dir}}
    fi
    git -C {{engine_dir}} fetch --quiet origin "$sha"
    git -C {{engine_dir}} checkout --quiet --detach "$sha"

runtime-build: engine-src
    #!/usr/bin/env bash
    set -euo pipefail
    lld_bin="$(brew --prefix lld 2>/dev/null)/bin"
    export PATH="$lld_bin:$PATH"
    cd {{engine_dir}}
    export MACOSX_DEPLOYMENT_TARGET="$(cat scripts/lib/macos-deployment-target.txt)"
    just release-runtime-build metal

app: build runtime-build
    #!/usr/bin/env bash
    set -euo pipefail
    sha=$(python3 scripts/release-inputs.py)
    archive={{engine_dir}}/dist/native-runtimes/meshllm-native-runtime-darwin-aarch64-metal.tar.gz
    version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)-local.$(git rev-parse --short=9 HEAD)"
    rm -rf dist/app
    python3 scripts/package-app.py target/release/mesh-tray "$archive" dist/app \
        --version "$version" --archive-sha256 "$(shasum -a 256 "$archive" | awk '{print $1}')" \
        --embedded --engine-commit "$sha" \
        --minimum-system-version "$(cat {{engine_dir}}/scripts/lib/macos-deployment-target.txt)"
    # Same order as release-macos.yml, with an ad-hoc identity.
    app=dist/app/Mesh.app
    find "$app/Contents/Resources/engine/native-runtimes" -type f \( -name '*.dylib' -o -perm -u+x \) -print0 \
        | xargs -0 -n1 codesign --force --sign -
    codesign --force --sign - "$app/Contents/MacOS/mesh-panel"
    python3 scripts/finalize-app.py dist/app runtime
    codesign --force --sign - --entitlements scripts/entitlements.plist "$app"
    codesign --verify --strict --deep "$app"
    python3 scripts/finalize-app.py dist/app final
    python3 scripts/verify-embedded-package.py dist/app
    echo "Built dist/app/Mesh.app — open it from Finder"
