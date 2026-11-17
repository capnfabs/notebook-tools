# macOS: Developer ID signing and notarization

(maybe drop this, don't think we'll ever bother)

Needs an Apple Developer Program membership and a *Developer ID Application* certificate. Add to `scripts/bundle-macos.sh`: sign with it, `--options runtime` (hardened runtime; check `ant` still runs and the keychain still works), `--timestamp`, then `xcrun notarytool submit --wait` (credentials saved with `notarytool store-credentials`) and `xcrun stapler staple` on the app and the DMG. Fixes the keychain prompt after each update and the "Open Anyway" step. See `decisions/2026-11-15-macos-bundle.md`.
