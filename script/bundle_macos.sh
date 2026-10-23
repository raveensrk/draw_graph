#!/usr/bin/env bash
# bundle_macos.sh: build draw_graph.app and a .dmg from the release binary.
#
# The app is the egui window: one binary, no web view. The bundle is the
# binary, its icon (app/icons/icon.icns) and an Info.plist; macOS needs no
# more. Started from Finder with no file, the app asks for one.
#
#   script/bundle_macos.sh [--target TRIPLE] [--install]
#
# --target   build for that Rust target (aarch64-apple-darwin, x86_64-apple-darwin)
# --install  also copy draw_graph.app to ~/Applications and the draw_graph
#            command to ~/.local/bin (agent1's harness check runs it)
#
# Writes dist/draw_graph.app and dist/draw_graph_VERSION_ARCH.dmg.
# Exit: 0 built, 1 a step failed, 2 bad call.
set -euo pipefail

usage() { sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'; }
target="" install=0
while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help|help) usage; exit 0 ;;
    --target) target="${2:?--target needs a triple}"; shift 2 ;;
    --install) install=1; shift ;;
    *) echo "bundle_macos.sh: unknown argument $1 - fix: script/bundle_macos.sh --help" >&2; exit 2 ;;
  esac
done

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

if [ -n "$target" ]; then
  cargo build --release -p draw_graph --target "$target"
  bin="target/$target/release/draw_graph"
  arch="${target%%-*}"
else
  cargo build --release -p draw_graph
  bin="target/release/draw_graph"
  arch="$(uname -m)"
fi
[ "$arch" = "x86_64" ] && arch="x64"
[ "$arch" = "arm64" ] && arch="aarch64"

app="dist/draw_graph.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/draw_graph"
cp app/icons/icon.icns "$app/Contents/Resources/icon.icns"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>draw_graph</string>
  <key>CFBundleDisplayName</key><string>draw_graph</string>
  <key>CFBundleIdentifier</key><string>com.raveensrk.draw-graph</string>
  <key>CFBundleExecutable</key><string>draw_graph</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

dmg="dist/draw_graph_${version}_${arch}.dmg"
rm -f "$dmg"
hdiutil create -quiet -volname draw_graph -srcfolder "$app" -ov -format UDZO "$dmg"
echo "built $app and $dmg"

if [ "$install" = 1 ]; then
  mkdir -p "$HOME/Applications" "$HOME/.local/bin"
  rm -rf "$HOME/Applications/draw_graph.app"
  cp -R "$app" "$HOME/Applications/draw_graph.app"
  cp "$bin" "$HOME/.local/bin/draw_graph"
  echo "installed $HOME/Applications/draw_graph.app and $HOME/.local/bin/draw_graph"
fi
