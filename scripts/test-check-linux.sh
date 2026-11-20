#!/usr/bin/env bash
# Tests for check-linux.sh's build-cache volumes, in a throwaway copy, against a fake `container`
# that keeps its volumes in a file and records the volume it's asked to run with.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"

root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT
passed=0

repo="$root/repo"
mkdir -p "$root/bin" "$repo/scripts/linux"
cp "$here/scripts/check-linux.sh" "$repo/scripts/"
echo 'FROM scratch' >"$repo/scripts/linux/Containerfile"
echo 'channel = "1.0.0"' >"$repo/rust-toolchain.toml"
echo 'lock 1' >"$repo/Cargo.lock"

export FAKE_CONTAINER="$root/fake"
mkdir -p "$FAKE_CONTAINER"
cat >"$root/bin/container" <<'EOF'
#!/usr/bin/env bash
volumes="$FAKE_CONTAINER/volumes"
touch "$volumes"
case "$1 $2" in
"system status" | "image inspect") ;;
"volume inspect") grep -qxF "$3" "$volumes" ;;
"volume create") echo "$3" >>"$volumes" ;;
"volume ls") cat "$volumes" ;;
"volume rm")
    grep -qxF "$3" "$volumes" || exit 1
    grep -vxF "$3" "$volumes" >"$volumes.new" || true
    mv "$volumes.new" "$volumes"
    ;;
"run "*)
    echo "$*" >"$FAKE_CONTAINER/run-args"
    for arg; do case $arg in *:/target) echo "${arg%:/target}" >"$FAKE_CONTAINER/ran" ;; esac; done ;;
*)
    echo "fake container: unexpected: $*" >&2
    exit 1
    ;;
esac
EOF
chmod +x "$root/bin/container"
export PATH="$root/bin:$PATH"

run() { "$repo/scripts/check-linux.sh" >"$root/out" 2>&1 || fail "check-linux failed"; }
volumes() { sort "$FAKE_CONTAINER/volumes"; }
target() { grep '^notebook-tools-linux-target' "$FAKE_CONTAINER/volumes"; }
ok() { passed=$((passed + 1)); }
fail() { echo "FAIL: $*"; cat "$root/out" 2>/dev/null; exit 1; }

# The old unhashed target volume goes; unrelated volumes stay; the run uses the new one.
printf '%s\n' notebook-tools-linux-target notebook-tools-linux-cargo other >"$FAKE_CONTAINER/volumes"
run
[ "$(target | wc -l)" -eq 1 ] || fail "expected one target volume: $(volumes)"; ok
first=$(target)
[[ $first == notebook-tools-linux-target-* ]] || fail "unhashed target volume kept"; ok
grep -qx other "$FAKE_CONTAINER/volumes" || fail "unrelated volume removed"; ok
[ "$(cat "$FAKE_CONTAINER/ran")" = "$first" ] || fail "ran with $(cat "$FAKE_CONTAINER/ran")"; ok

# Debug info is only line tables in the VM.
grep -q -- "-e CARGO_PROFILE_DEV_DEBUG=line-tables-only" "$FAKE_CONTAINER/run-args" || fail "debug info not limited"; ok

# Nothing changed: the same volume, kept.
run
[ "$(target)" = "$first" ] || fail "volume changed without a lockfile change"; ok
grep -q "removing" "$root/out" && fail "removed a volume without a lockfile change"
ok

# A lockfile change replaces it; the cargo registry stays.
echo 'lock 2' >"$repo/Cargo.lock"
run
second=$(target)
[ "$second" != "$first" ] || fail "lockfile change kept $first"; ok
[ "$(target | wc -l)" -eq 1 ] || fail "stale target volume kept: $(volumes)"; ok
grep -qx notebook-tools-linux-cargo "$FAKE_CONTAINER/volumes" || fail "cargo volume removed"; ok
[ "$(cat "$FAKE_CONTAINER/ran")" = "$second" ] || fail "ran with the old volume"; ok

# So does a toolchain change.
echo 'channel = "1.0.1"' >"$repo/rust-toolchain.toml"
run
[ "$(target)" != "$second" ] || fail "toolchain change kept $second"; ok

echo "test-check-linux: $passed passed"
