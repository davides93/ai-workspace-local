#!/bin/bash
# Local check of the folders the person grants (Mac App Store build): the real App Sandbox, the C ABI and the
# security-scoped bookmarks, in a throw-away app signed ad hoc with the entitlements of the store build.
#
#   bash selftest.sh           what needs no person (also a second launch that reopens a saved permission)
#   bash selftest.sh --pick    also opens the real macOS panel: choose a folder (try ~/Downloads); the app lists it
#                              from inside the sandbox, saves its permission, and a second launch reopens it
#                              WITHOUT the panel. This is the one part that needs a click.
#
# Does not touch the installed app. Leaves nothing but the sandbox container of the test app
# (~/Library/Containers/dev.aiworkspace.folders-selftest), which macOS creates for every sandboxed app.
set -euo pipefail
sources="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d "${TMPDIR:-/tmp}/aiw-folders-test.XXXXXX")"
trap 'rm -rf "$work"' EXIT
app="$work/AiwFoldersTest.app"
mkdir -p "$app/Contents/MacOS"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.aiworkspace.folders-selftest</string>
<key>CFBundleExecutable</key><string>folders-selftest</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
PLIST
# The entitlements of the store build that matter here (apps/desktop/src-tauri/Entitlements.mas.plist).
cat > "$work/entitlements.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>com.apple.security.app-sandbox</key><true/>
<key>com.apple.security.files.user-selected.read-write</key><true/>
<key>com.apple.security.files.bookmarks.app-scope</key><true/>
</dict></plist>
PLIST
xcrun swiftc -module-cache-path "$work/cache" -parse-as-library "$sources/Folders.swift" "$sources/Selftest.swift" -o "$app/Contents/MacOS/folders-selftest"
codesign --force --sign - --entitlements "$work/entitlements.plist" "$app"
bin="$app/Contents/MacOS/folders-selftest"
container="$HOME/Library/Containers/dev.aiworkspace.folders-selftest/Data"
rm -f "$container/bookmark.txt" 2>/dev/null || true

if [[ "${1:-}" == "--pick" ]]; then
  "$bin" --pick
  echo "--- second launch: the permission must reopen without the panel"
  "$bin" --load --pick
else
  "$bin" --save
  echo "--- second launch: the saved permission must reopen"
  "$bin" --load
fi
echo "OK"
