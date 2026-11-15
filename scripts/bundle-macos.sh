#!/usr/bin/env bash
# Builds `target/bundle/Notebook Tools.app` and `target/bundle/Notebook Tools <version>.dmg` (the app
# and a link to /Applications), using only Apple's tools, Xcode's `actool` among them. The bundle's
# Info.plist and Icon Composer icon are in `crates/app/macos/`; both versions in the plist are the
# Cargo version.
#
#   bundle-macos.sh                  signed ad hoc: runs on this Mac; elsewhere, macOS asks first
#   bundle-macos.sh --identity NAME  signed with the code-signing certificate NAME, e.g. a
#                                    self-signed one, so the keychain trusts every build alike
#   bundle-macos.sh --no-dmg         just the app
#
# The app isn't notarized: on another Mac, opening it the first time needs System Settings →
# Privacy & Security → Open Anyway. See `decisions/2026-11-15-macos-bundle.md`.
set -euo pipefail
cd "$(dirname "$0")/.."

identity=-
dmg=true
while [ $# -gt 0 ]; do
    case $1 in
    --identity)
        identity=${2:?--identity needs a certificate name}
        shift 2
        ;;
    --no-dmg)
        dmg=false
        shift
        ;;
    *)
        echo "usage: $0 [--identity NAME] [--no-dmg]" >&2
        exit 2
        ;;
    esac
done

name="Notebook Tools"
binary=notebook-tools
pkgid=$(cargo pkgid -p notebook_app)
version=${pkgid##*@}
out=target/bundle
app="$out/$name.app"

cargo build --release --locked -p notebook_app

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "target/release/$binary" "$app/Contents/MacOS/"
plist="$app/Contents/Info.plist"
cp crates/app/macos/Info.plist "$plist"
plutil -replace CFBundleShortVersionString -string "$version" "$plist"
plutil -replace CFBundleVersion -string "$version" "$plist"
plutil -lint "$plist" >/dev/null

# The Icon Composer icon, compiled into Assets.car (macOS 26+, which masks and lights it itself)
# and AppIcon.icns (older macOS).
if ! xcrun --find actool >/dev/null 2>&1; then
    echo "bundle-macos: actool is needed for the icon: install Xcode, then run xcodebuild -runFirstLaunch" >&2
    exit 1
fi
partial=$(mktemp -d)
xcrun actool crates/app/macos/AppIcon.icon --compile "$app/Contents/Resources" \
    --app-icon AppIcon --include-all-app-icons --platform macosx --target-device mac \
    --minimum-deployment-target "$(plutil -extract LSMinimumSystemVersion raw "$plist")" \
    --output-partial-info-plist "$partial/Info.plist" \
    --errors --warnings --output-format human-readable-text >/dev/null
rm -rf "$partial"
for compiled in Assets.car AppIcon.icns; do
    if [ ! -f "$app/Contents/Resources/$compiled" ]; then
        echo "bundle-macos: actool didn't make $compiled" >&2
        exit 1
    fi
done

codesign --force --sign "$identity" --timestamp=none "$app"

# Check what was built: the signature covers everything, and the binary needs nothing outside the
# OS (as `crates/app/tests/linking.rs` checks of the debug build).
codesign --verify --strict --verbose=1 "$app"
outside=$(otool -L "$app/Contents/MacOS/$binary" | tail -n +2 | awk '{print $1}' |
    grep -vE '^/(System/Library|usr/lib)/' || true)
if [ -n "$outside" ]; then
    echo "bundle-macos: links libraries outside the OS:" >&2
    echo "$outside" >&2
    exit 1
fi
echo "bundle-macos: built $app ($version, signed with ${identity/#-/ad hoc identity})"

if $dmg; then
    stage=$(mktemp -d)
    cp -R "$app" "$stage/"
    ln -s /Applications "$stage/Applications"
    image="$out/$name $version.dmg"
    rm -f "$image"
    hdiutil create -quiet -volname "$name" -srcfolder "$stage" -format UDZO "$image"
    rm -rf "$stage"
    echo "bundle-macos: built $image"
fi
