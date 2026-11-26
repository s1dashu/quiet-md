#!/bin/zsh
# Replace the running hand-test build (dist-preview) with target/release. It
# runs as dist-preview/Quiet.md Preview.app (so it is never mistaken for the installed app) on the
# saved preview library, or the default one (~/Documents/Quiet.md if it is there, else
# ~/Documents/Quiet Notes, as the app picks). The app initializes the
# bundled 开始指南 on first launch, just like an installed build.
# When that library is the one an installed build opens (the default place, or where
# library.json says), the preview runs as installed: no QUIET_MD_HOME, so it finds the
# library the same way and renaming it is remembered for the installed app too. Any other
# library (a temporary or test one, or QUIET_MD_HOME given) is pinned with QUIET_MD_HOME.
# Only this checkout's own binaries are stopped:
# pkill matches this checkout's absolute path. Log: /tmp/quiet-md-preview.log
set -e
cd "$(dirname "$0")/.."
default_library="$HOME/Documents/Quiet Notes"; [ -d "$HOME/Documents/Quiet.md" ] && default_library="$HOME/Documents/Quiet.md"
preview_library="${QUIET_MD_HOME:-$(cat dist-preview/library-path 2>/dev/null || printf '%s' "$default_library")}"
# An empty path would put the library in the current directory.
[ -n "$preview_library" ] || { echo "preview.sh: no preview library" >&2; exit 1; }
located=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("path",""))' "$HOME/Library/Application Support/quiet-md/library.json" 2>/dev/null || true)
installed="${located:-$default_library}"
as_installed=
# the old default still named after a rename: library.json says where it went
if [ -z "${QUIET_MD_HOME:-}" ] && { [ "$preview_library" = "$installed" ] || { [ -n "$located" ] && [ "$preview_library" = "$HOME/Documents/Quiet.md" ]; }; }; then as_installed=1; fi
if [ -n "$as_installed" ]; then unset QUIET_MD_HOME; echo "preview.sh: the installed app's library ($installed), found as installed"; else export QUIET_MD_HOME="$preview_library"; fi
app="$PWD/dist-preview/Quiet.md Preview.app"
bin="$app/Contents/MacOS/quiet_md"
pkill -f "^$bin" || true
# The bundle's earlier name, from before it said Preview.
old="$PWD/dist-preview/quiet.md.app"
pkill -f "^$old/Contents/MacOS/quiet_md" || true
sleep 1
rm -rf "$old"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp assets/branding/app-icon-dev.png "$app/Contents/Resources/app-icon.png"
icon_tmp=$(mktemp -d)
iconset="$icon_tmp/app-icon.iconset"
mkdir -p "$iconset"
for s in 16 32 128 256 512; do
  sips -z $s $s assets/branding/app-icon-dev.png --out "$iconset/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) assets/branding/app-icon-dev.png --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/app-icon.icns"
rm -rf "$icon_tmp"
cp target/release/quiet_md "$bin.bin"
# Finder launches must use the same library as this script.
if [ -n "$as_installed" ]; then printf '#!/bin/zsh\nunset QUIET_MD_HOME\nexec "${0:A:h}/quiet_md.bin" "$@"\n' > "$bin"
else printf '#!/bin/zsh\nexport QUIET_MD_HOME=%q\nexec "${0:A:h}/quiet_md.bin" "$@"\n' "$preview_library" > "$bin"; fi
chmod +x "$bin"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Quiet.md Preview</string>
  <key>CFBundleDisplayName</key><string>Quiet.md Preview</string>
  <key>CFBundleIdentifier</key><string>local.quietmd.preview</string>
  <key>CFBundleExecutable</key><string>quiet_md</string>
  <key>CFBundleIconFile</key><string>app-icon.icns</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSDocumentsFolderUsageDescription</key><string>Quiet.md 把文档库放在「文稿」里的 Quiet Notes 文件夹。</string>
  <key>CFBundleDocumentTypes</key><array><dict>
    <key>CFBundleTypeName</key><string>Markdown</string>
    <key>CFBundleTypeRole</key><string>Editor</string>
    <key>LSHandlerRank</key><string>Alternate</string>
    <key>LSItemContentTypes</key><array><string>net.daringfireball.markdown</string></array>
    <key>CFBundleTypeExtensions</key><array><string>md</string><string>markdown</string><string>mdown</string></array>
  </dict><dict>
    <key>CFBundleTypeName</key><string>Folder</string>
    <key>CFBundleTypeRole</key><string>Editor</string>
    <key>LSHandlerRank</key><string>Alternate</string>
    <key>LSItemContentTypes</key><array><string>public.folder</string></array>
  </dict></array>
  <key>CFBundleURLTypes</key><array><dict>
    <key>CFBundleURLName</key><string>com.s1dashu.quietmd.open</string>
    <key>CFBundleURLSchemes</key><array><string>quietmd</string></array>
  </dict></array>
</dict></plist>
PLIST
codesign --force --sign - "$app" >/dev/null 2>&1 || true
# The Dock and Finder cache a bundle's icon by path: tell them it changed.
touch "$app"
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$app" 2>/dev/null || true
(nohup "$bin" >/tmp/quiet-md-preview.log 2>&1 &)
sleep 2; pgrep -fl "^$bin"
