#!/bin/sh
# Builds the AppImage of the app: one file that runs on most distributions without installing.
#
#   packaging/linux/build-appimage.sh <version> <app bundle> <appimagetool> <output file>
#
# GTK 3 is expected on the system, as on every common desktop; it is not bundled.
set -eu

version="$1"
bundle="$2"
tool="$3"
out="$4"
here="$(cd "$(dirname "$0")" && pwd)"
id=io.github.poldion.tephra

dir="$(mktemp -d)"
trap 'rm -rf "$dir"' EXIT
app="$dir/Tephra.AppDir"

install -d "$app/usr/lib/tephra" "$app/usr/share/applications" "$app/usr/share/icons/hicolor/256x256/apps"
cp -r "$bundle"/. "$app/usr/lib/tephra/"
install -m 0644 "$here/$id.desktop" "$app/usr/share/applications/$id.desktop"
install -m 0644 "$here/icons/256.png" "$app/usr/share/icons/hicolor/256x256/apps/$id.png"
# appimagetool looks for the desktop file and the icon at the top of the folder.
cp "$here/$id.desktop" "$app/$id.desktop"
cp "$here/icons/256.png" "$app/$id.png"
ln -s "$id.png" "$app/.DirIcon"
cat > "$app/AppRun" <<'EOF'
#!/bin/sh
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/lib/tephra/tephra-app" "$@"
EOF
chmod 0755 "$app/AppRun"

mkdir -p "$(dirname "$out")"
# Without --appimage-extract-and-run the tool needs FUSE, which build machines often lack.
ARCH=x86_64 VERSION="$version" "$tool" --appimage-extract-and-run "$app" "$out" > /dev/null
chmod 0755 "$out"
echo "$out"
