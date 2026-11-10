#!/bin/bash
# Local integration check: native WebKit, C ABI, App Sandbox and HTTPS proxy routing.
set -euo pipefail
sources="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d "${TMPDIR:-/tmp}/aiw-browser-test.XXXXXX")"
trap 'rm -rf "$work"' EXIT
app="$work/AiwBrowserTest.app"
mkdir -p "$app/Contents/MacOS"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.aiworkspace.browser-selftest</string>
<key>CFBundleExecutable</key><string>browser-selftest</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
PLIST
cat > "$work/entitlements.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>com.apple.security.app-sandbox</key><true/>
<key>com.apple.security.network.client</key><true/>
<key>com.apple.security.network.server</key><true/>
</dict></plist>
PLIST
xcrun swiftc -module-cache-path "$work/cache" -parse-as-library "$sources/Browser.swift" "$sources/Selftest.swift" -o "$app/Contents/MacOS/browser-selftest"
codesign --force --sign - --entitlements "$work/entitlements.plist" "$app"
"$app/Contents/MacOS/browser-selftest" "$@"
