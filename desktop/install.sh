#!/bin/sh
# Install the Beehive desktop app for this user: the Electron runtime, a `beehive-desktop`
# launcher in ~/.local/bin, and an application-menu entry. Needs node + npm.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
dest="${XDG_DATA_HOME:-$HOME/.local/share}/beehive-desktop"
bin="$HOME/.local/bin"
apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"

mkdir -p "$dest" "$bin" "$apps"
cp "$here/main.js" "$here/package.json" "$here/package-lock.json" "$here/icon.svg" "$dest/"
(cd "$dest" && npm ci --no-audit --no-fund --loglevel=error)
# npm may skip dependency install scripts; Electron's fetches its runtime.
[ -x "$dest/node_modules/electron/dist/electron" ] || (cd "$dest" && node node_modules/electron/install.js)

cat > "$bin/beehive-desktop" <<EOF
#!/bin/sh
exec "$dest/node_modules/.bin/electron" "$dest" "\$@"
EOF
chmod +x "$bin/beehive-desktop"

cat > "$apps/beehive.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Beehive
Comment=Beehive agent harness
Exec=$bin/beehive-desktop
Icon=$dest/icon.svg
Terminal=false
Categories=Development;
StartupWMClass=Beehive
EOF
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps" || true
echo "Installed: run beehive-desktop, or open Beehive from your application menu."
