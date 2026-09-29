#!/usr/bin/env bash
# Construit le paquet Debian : dist/dashboard-crypto_<version>_all.deb
#
# Usage : packaging/debian/build_deb.sh [version]
# Sans argument, la version est lue dans le fichier VERSION. Un préfixe « v »
# (tag git v1.2.0) est retiré.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SRC="$ROOT/packaging/debian"
PKG=dashboard-crypto

VERSION="${1:-$(cat "$ROOT/VERSION")}"
VERSION="${VERSION#v}"
if ! [[ "$VERSION" =~ ^[0-9][A-Za-z0-9.+~-]*$ ]]; then
    echo "Version invalide : '$VERSION'" >&2
    exit 1
fi

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
chmod 755 "$STAGE"

APP="$STAGE/opt/$PKG"
install -d "$APP/app" "$APP/app/dune_queries"
for f in "$ROOT"/*.py; do
    [ "$(basename "$f")" = conftest.py ] && continue
    install -m 644 "$f" "$APP/app/"
done
install -m 644 "$ROOT/indicators.json" "$ROOT/requirements.txt" "$ROOT/README.md" "$ROOT/LICENSE" "$APP/app/"
install -m 644 "$ROOT"/dune_queries/*.sql "$APP/app/dune_queries/"
echo "$VERSION" > "$APP/app/VERSION"
install -m 644 "$SRC/launcher.py" "$APP/"

install -D -m 755 "$SRC/dashboard-crypto" "$STAGE/usr/bin/$PKG"
install -D -m 644 "$SRC/dashboard-crypto.desktop" "$STAGE/usr/share/applications/$PKG.desktop"
install -D -m 644 "$SRC/dashboard-crypto.svg" "$STAGE/usr/share/icons/hicolor/scalable/apps/$PKG.svg"
install -D -m 644 "$ROOT/LICENSE" "$STAGE/usr/share/doc/$PKG/copyright"

install -d "$STAGE/DEBIAN"
install -m 755 "$SRC/postinst" "$SRC/prerm" "$STAGE/DEBIAN/"
INSTALLED_SIZE="$(du -sk --exclude=DEBIAN "$STAGE" | cut -f1)"
cat > "$STAGE/DEBIAN/control" <<CONTROL
Package: $PKG
Version: $VERSION
Section: misc
Priority: optional
Architecture: all
Installed-Size: $INSTALLED_SIZE
Depends: python3 (>= 3.9), python3-venv, python3-gi, gir1.2-gtk-3.0, gir1.2-webkit2-4.1 | gir1.2-webkit2-4.0
Recommends: xdg-utils
Maintainer: neomars <neomars@users.noreply.github.com>
Homepage: https://github.com/neomars/dashboard_crypto
Description: Tableau de bord d'indicateurs Bitcoin et marchés financiers
 Application Streamlit (Fear & Greed, halvings, indicateurs on-chain, SOPR,
 Net Realized Profit/Loss, simulateur d'investissement...) affichée dans une
 fenêtre native. Les dépendances Python sont installées dans
 /opt/dashboard-crypto/venv lors de l'installation (connexion Internet requise).
CONTROL

mkdir -p "$ROOT/dist"
OUT="$ROOT/dist/${PKG}_${VERSION}_all.deb"
dpkg-deb --build --root-owner-group "$STAGE" "$OUT"
echo "Paquet créé : $OUT"
