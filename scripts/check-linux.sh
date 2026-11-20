#!/usr/bin/env bash
# Clippy and the tests on Linux (arm64), in an apple/container VM, so the code macOS never compiles
# (the libheif decoder) gets built and run. `scripts/check.sh` runs this last. The image is rebuilt
# only when `scripts/linux/Containerfile` or `rust-toolchain.toml` changes; builds are cached in
# container volumes, a fresh target volume whenever those or `Cargo.lock` change
# (`container volume rm` clears them). See `decisions/2026-11-15-local-linux-ci.md` and
# `decisions/2026-11-18-linux-target-volume-per-lockfile.md`.
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

# Cargo keeps every stale build beside the new one, and files deleted in a volume don't shrink it on
# the Mac, so each lockfile and toolchain gets its own target volume and the others are removed.
target_hash=$(cat scripts/linux/Containerfile rust-toolchain.toml Cargo.lock | shasum -a 256 | cut -c1-12)
target_volume="notebook-tools-linux-target-$target_hash"
container volume ls -q | grep '^notebook-tools-linux-target' | grep -vxF "$target_volume" \
    | while read -r stale; do
        echo "check-linux: removing stale build cache $stale"
        container volume rm "$stale" >/dev/null || echo "check-linux: couldn't remove $stale" >&2
    done

for volume in "$target_volume" notebook-tools-linux-cargo; do
    container volume inspect "$volume" >/dev/null 2>&1 \
        || container volume create "$volume" >/dev/null || exit 1
done

# The source is read-only; builds go to the volumes. Runs every step, like check.sh. Debug info is
# only line tables: Linux links it into every test binary, which made them 800 MB each.
container run --rm --cpus "$(sysctl -n hw.ncpu)" --memory 8G \
    --mount "type=bind,source=$PWD,target=/src,readonly" \
    -v "$target_volume":/target \
    -v notebook-tools-linux-cargo:/cargo \
    -e CARGO_TARGET_DIR=/target -e CARGO_HOME=/cargo -e PATH=/usr/local/cargo/bin:/usr/bin:/bin \
    -e CARGO_PROFILE_DEV_DEBUG=line-tables-only \
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
