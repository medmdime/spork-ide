#!/usr/bin/env bash
# Builds this OS's download into target/dist (release, unsigned):
#   macOS   Spork-<version>-macos-<arch>.zip      Spork.app (see package-macos.sh)
#   Linux   spork-<version>-linux-<arch>.tar.gz   bin/spork + a .desktop entry
#   Windows spork-<version>-windows-<arch>.zip    spork.exe
# shortcut: no code signing, installers or auto-update yet; they need certificates and a
# signing service, and come when 1.0 ships (P7).
set -euo pipefail
# the monorepo, whose workspace builds Spork
cd "$(dirname "$0")/../.."
version=$(cargo pkgid -p spork | sed 's/.*[#@]//')
arch=$(uname -m)
dist=target/dist
mkdir -p "$dist"
case "$(uname -s)" in
  Darwin)
    spork/scripts/package-macos.sh
    ditto -c -k --norsrc --noextattr --keepParent target/Spork.app "$dist/Spork-$version-macos-$arch.zip"
    ;;
  Linux)
    cargo build --release -p spork
    stage=target/spork-$version
    rm -rf "$stage"
    mkdir -p "$stage/bin" "$stage/share/applications" "$stage/share/icons/hicolor/512x512/apps"
    cp target/release/spork "$stage/bin/"
    cp crates/spork/icon/spork.png "$stage/share/icons/hicolor/512x512/apps/"
    cat > "$stage/share/applications/spork.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Spork
Comment=Git client
Exec=spork %F
Icon=spork
Terminal=false
Categories=Development;RevisionControl;
DESKTOP
    tar -C target -czf "$dist/spork-$version-linux-$arch.tar.gz" "spork-$version"
    ;;
  MINGW* | MSYS* | CYGWIN*)
    cargo build --release -p spork
    powershell -NoProfile -Command \
      "Compress-Archive -Force -Path target/release/spork.exe -DestinationPath $dist/spork-$version-windows-$arch.zip"
    ;;
  *)
    echo "package.sh: unsupported OS $(uname -s)" >&2
    exit 1
    ;;
esac
ls -l "$dist"
