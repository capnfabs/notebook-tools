#!/usr/bin/env bash
# Clippy and the tests on Linux (arm64), in an apple/container VM, so the code macOS never compiles
# (the libheif decoder) gets built and run. `scripts/check.sh` runs this last. The image is rebuilt
# only when `scripts/linux/Containerfile` or `rust-toolchain.toml` changes; builds are cached in
# container volumes (`container volume rm notebook-tools-linux-target notebook-tools-linux-cargo`
# clears them). See `decisions/2026-11-15-local-linux-ci.md`.
set -uo pipefail
cd "$(dirname "$0")/.."

if ! command -v container >/dev/null; then
    echo "check-linux: apple/container isn't installed: https://github.com/apple/container/releases" >&2
    exit 1
fi
if ! container system status >/dev/null 2>&1; then
    echo "check-linux: the container service isn't running: run 'container system start'" >&2
    exit 1
fi

rust_version=$(sed -n 's/^channel = "\(.*\)"$/\1/p' rust-toolchain.toml)
hash=$(cat scripts/linux/Containerfile rust-toolchain.toml | shasum -a 256 | cut -c1-12)
image="notebook-tools-linux:$hash"
if ! container image inspect "$image" >/dev/null 2>&1; then
    echo "check-linux: building $image"
    container build --build-arg "RUST_VERSION=$rust_version" -t "$image" scripts/linux || exit 1
fi

for volume in notebook-tools-linux-target notebook-tools-linux-cargo; do
    container volume inspect "$volume" >/dev/null 2>&1 \
        || container volume create "$volume" >/dev/null || exit 1
done

# The source is read-only; builds go to the volumes. Runs every step, like check.sh.
container run --rm --cpus "$(sysctl -n hw.ncpu)" --memory 8G \
    --mount "type=bind,source=$PWD,target=/src,readonly" \
    -v notebook-tools-linux-target:/target \
    -v notebook-tools-linux-cargo:/cargo \
    -e CARGO_TARGET_DIR=/target -e CARGO_HOME=/cargo -e PATH=/usr/local/cargo/bin:/usr/bin:/bin \
    -w /src "$image" bash -c '
        failed=()
        cargo clippy --locked --workspace --all-targets -- -D warnings || failed+=("clippy")
        cargo test --locked --workspace || failed+=("test")
        if [ ${#failed[@]} -gt 0 ]; then
            echo "check-linux: FAILED: ${failed[*]}" >&2
            exit 1
        fi
        echo "check-linux: all passed"
    '
