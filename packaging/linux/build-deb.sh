#!/bin/sh
# Builds the Debian package: the app, the command line `tephra` and the sync server `tephra-server`.
#
#   packaging/linux/build-deb.sh <version> <app bundle> <folder with tephra and tephra-server> <output folder>
#
# The app bundle is the folder `flutter build linux --release` produces (app/build/linux/x64/release/bundle).
set -eu

version="$1"
bundle="$2"
programs="$3"
out="$4"
here="$(cd "$(dirname "$0")" && pwd)"
root="$here/../.."
id=io.github.poldion.tephra

pkg="$(mktemp -d)"
trap 'rm -rf "$pkg"' EXIT

install -d "$pkg/DEBIAN" "$pkg/opt/tephra" "$pkg/usr/bin" "$pkg/usr/share/applications" \
  "$pkg/usr/share/doc/tephra/examples"
cp -r "$bundle"/. "$pkg/opt/tephra/"
# The app finds its data next to the real program, so a link in the PATH is enough.
ln -s /opt/tephra/tephra-app "$pkg/usr/bin/tephra-app"
install -m 0755 "$programs/tephra" "$programs/tephra-server" "$pkg/usr/bin/"
install -m 0644 "$here/$id.desktop" "$pkg/usr/share/applications/$id.desktop"
for size in 48 64 128 256; do
  install -D -m 0644 "$here/icons/$size.png" "$pkg/usr/share/icons/hicolor/${size}x${size}/apps/$id.png"
done
install -m 0644 "$root/LICENSE" "$pkg/usr/share/doc/tephra/copyright"
install -m 0644 "$root/THIRD_PARTY_NOTICES.md" "$pkg/usr/share/doc/tephra/"
install -m 0644 "$root/deploy/tephra-server.service" "$pkg/usr/share/doc/tephra/examples/"
chmod -R u=rwX,go=rX "$pkg"

cat > "$pkg/DEBIAN/control" <<EOF
Package: tephra
Version: $version
Section: net
Priority: optional
Architecture: amd64
Maintainer: Pold <pold@poldion.cc>
Installed-Size: $(du -sk "$pkg" | cut -f1)
Depends: libc6 (>= 2.35), libstdc++6, libgtk-3-0 | libgtk-3-0t64, libglib2.0-0 | libglib2.0-0t64
Homepage: https://github.com/Poldion/Tephra
Description: SSH client with credential vault, in-app WireGuard tunnels and sync
 Tephra keeps hosts, credentials, SSH keys and VPN profiles in an encrypted
 vault. A host can be reached through a WireGuard tunnel that runs inside the
 program, without a network adapter or administrator rights.
 .
 The package contains the graphical app (tephra-app), the command line
 (tephra) and the sync server (tephra-server).
EOF

mkdir -p "$out"
dpkg-deb --build --root-owner-group "$pkg" "$out/tephra_${version}_amd64.deb" > /dev/null
echo "$out/tephra_${version}_amd64.deb"
