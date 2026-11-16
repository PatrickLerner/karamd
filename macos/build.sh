#!/usr/bin/env bash
# Build KaramdBar.app into macos/build. --install copies it to ~/Applications and relaunches it.
set -euo pipefail
cd "$(dirname "$0")"

version=$(sed -n 's/^version = "\(.*\)"/\1/p' ../Cargo.toml | head -1)
if command -v xcodegen >/dev/null; then
  xcodegen generate --quiet
else
  nix shell nixpkgs#xcodegen -c xcodegen generate --quiet
fi
xcodebuild -project KaramdBar.xcodeproj -scheme KaramdBar -configuration Release \
  -derivedDataPath build -destination 'generic/platform=macOS' -quiet \
  ARCHS="arm64 x86_64" ONLY_ACTIVE_ARCH=NO MARKETING_VERSION="$version" build
app=build/Build/Products/Release/KaramdBar.app
echo "$app ($version)"

[[ "${1:-}" == --install ]] || exit 0
pkill -x KaramdBar && while pgrep -x KaramdBar >/dev/null; do sleep 0.1; done || true
mkdir -p ~/Applications
rm -rf ~/Applications/KaramdBar.app
cp -R "$app" ~/Applications/
# Drop Claude session markers so a build run from a Claude session does not leak them.
env $(env | grep -o "^CLAUDE[^=]*" | sed "s/^/-u /") open ~/Applications/KaramdBar.app
