#!/bin/sh
# Lanceur macOS et Linux de simp2puml.
# 1. Utilise un Python >= 3.11 avec Tkinter s'il existe déjà.
# 2. Sinon installe uv (version fixée, sommes SHA-256 vérifiées par son installateur), puis un Python
#    géré par uv, le tout dans le dossier de données de l'utilisateur, sans droits administrateur.
# 3. Crée une application (~/Applications, macOS) ou une entrée de menu (Linux), puis ouvre l'interface.
# Désinstallation : supprimer le dossier de données indiqué ci-dessous et le raccourci créé.
set -eu

UV_VERSION=0.13.7
PYTHON_VERSION=3.12

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
START="$ROOT/launchers/start.pyw"
ASSETS="$ROOT/src/simp2puml/assets"
case "$(uname -s)" in
    Darwin) SYSTEM=macos; DATA="$HOME/Library/Application Support/simp2puml" ;;
    *) SYSTEM=linux; DATA="${XDG_DATA_HOME:-$HOME/.local/share}/simp2puml" ;;
esac
LOG="$DATA/derniere-execution.log"

say() { printf '%s\n' "$*"; }

# Message visible même sans terminal (lancement depuis le Finder ou le menu des applications).
alert() {
    say "Erreur : $1" >&2
    [ -t 1 ] && return 0
    # shellcheck disable=SC1003  # Supprime guillemets et barres obliques inverses (littéraux voulus).
    text=$(printf '%s' "$1" | tr -d '"\\')
    if [ "$SYSTEM" = macos ]; then
        osascript -e "display alert \"simp2puml\" message \"$text\"" >/dev/null 2>&1 || true
    elif command -v zenity >/dev/null 2>&1; then
        zenity --error --title=simp2puml --text="$text" >/dev/null 2>&1 || true
    elif command -v kdialog >/dev/null 2>&1; then
        kdialog --title simp2puml --error "$text" >/dev/null 2>&1 || true
    elif command -v notify-send >/dev/null 2>&1; then
        notify-send simp2puml "$text" >/dev/null 2>&1 || true
    fi
}

fail() {
    alert "$1"
    if [ -t 0 ]; then
        printf 'Appuyez sur Entrée pour fermer… '
        read -r _ || true
    fi
    exit 1
}

# Python >= 3.11 dont le module Tkinter charge bien Tcl/Tk.
good_python() {
    "$1" -c 'import sys, tkinter; tkinter.Tcl(); sys.exit(sys.version_info < (3, 11))' >/dev/null 2>&1
}

# Échappement pour une chaîne entre apostrophes dans un script shell généré.
quote() {
    printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

download() {
    if command -v curl >/dev/null 2>&1; then
        curl --proto '=https' --tlsv1.2 -LsSf "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget --https-only -qO- "$1"
    else
        fail "curl ou wget est nécessaire pour la première installation."
    fi
}

find_python() {
    [ -n "${SIMP2PUML_FORCE_MANAGED:-}" ] && return 0
    for candidate in python3.14 python3.13 python3.12 python3.11 python3; do
        path=$(command -v "$candidate" 2>/dev/null) || continue
        # Sur macOS, /usr/bin/python3 est trop ancien et peut proposer d'installer les outils Xcode.
        [ "$SYSTEM" = macos ] && [ "$path" = /usr/bin/python3 ] && continue
        if good_python "$path"; then
            PYTHON=$path
            return 0
        fi
    done
    return 0
}

managed_python() {
    export UV_PYTHON_INSTALL_DIR="$DATA/python" UV_CACHE_DIR="$DATA/cache" UV_PYTHON_PREFERENCE=only-managed
    uv="$DATA/bin/uv"
    if [ ! -x "$uv" ]; then
        say "Première utilisation : installation de uv $UV_VERSION (gestionnaire Python d'Astral)…"
        mkdir -p "$DATA/bin"
        installer=$(download "https://astral.sh/uv/$UV_VERSION/install.sh") \
            || fail "Téléchargement de uv impossible. Vérifiez la connexion Internet."
        printf '%s\n' "$installer" | UV_UNMANAGED_INSTALL="$DATA/bin" sh \
            || fail "Installation de uv impossible."
    fi
    if ! PYTHON=$("$uv" python find "$PYTHON_VERSION" 2>/dev/null); then
        say "Installation de Python $PYTHON_VERSION pour simp2puml (une seule fois)…"
        "$uv" python install "$PYTHON_VERSION" || fail "Installation de Python impossible."
        PYTHON=$("$uv" python find "$PYTHON_VERSION") || fail "Python $PYTHON_VERSION introuvable après installation."
    fi
    good_python "$PYTHON" || fail "Le Python installé ne fournit pas Tkinter."
}

install_linux_entry() {
    apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
    entry="$apps/simp2puml.desktop"
    [ -f "$entry" ] || created=1
    mkdir -p "$apps"
    # shellcheck disable=SC2016  # Échappement littéral pour la clé Exec du fichier .desktop.
    exec_path=$(printf '%s' "$ROOT/simp2puml-linux.sh" | sed 's/\\/\\\\/g; s/"/\\"/g; s/`/\\`/g; s/\$/\\$/g')
    cat >"$entry.tmp" <<EOF
[Desktop Entry]
Type=Application
Name=simp2puml
GenericName=Convertisseur de diagrammes
Comment=Convertir des diagrammes SIMP, PlantUML et Atelier Diagrammes
Exec=sh "$exec_path" %f
Icon=$ASSETS/icon-256.png
Terminal=false
Categories=Development;Graphics;
EOF
    mv "$entry.tmp" "$entry"
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps" >/dev/null 2>&1 || true
    if [ -n "${created:-}" ]; then
        say "Entrée « simp2puml » ajoutée au menu des applications."
    fi
}

install_macos_app() {
    app="$HOME/Applications/simp2puml.app"
    [ -d "$app" ] || created=1
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    icns="$app/Contents/Resources/simp2puml.icns"
    if [ ! -f "$icns" ] && command -v iconutil >/dev/null 2>&1; then
        set_dir="$DATA/simp2puml.iconset"
        mkdir -p "$set_dir"
        cp "$ASSETS/icon-16.png" "$set_dir/icon_16x16.png"
        cp "$ASSETS/icon-32.png" "$set_dir/icon_16x16@2x.png"
        cp "$ASSETS/icon-32.png" "$set_dir/icon_32x32.png"
        cp "$ASSETS/icon-64.png" "$set_dir/icon_32x32@2x.png"
        cp "$ASSETS/icon-128.png" "$set_dir/icon_128x128.png"
        cp "$ASSETS/icon-256.png" "$set_dir/icon_128x128@2x.png"
        cp "$ASSETS/icon-256.png" "$set_dir/icon_256x256.png"
        iconutil -c icns "$set_dir" -o "$icns" >/dev/null 2>&1 || true
    fi
    cat >"$app/Contents/Info.plist.tmp" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>simp2puml</string>
    <key>CFBundleDisplayName</key><string>simp2puml</string>
    <key>CFBundleIdentifier</key><string>local.simp2puml.gui</string>
    <key>CFBundleExecutable</key><string>simp2puml</string>
    <key>CFBundleIconFile</key><string>simp2puml</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
    mv "$app/Contents/Info.plist.tmp" "$app/Contents/Info.plist"
    # Écriture atomique : l'ancien script peut être en cours d'exécution.
    {
        printf '#!/bin/sh\n'
        printf 'exec /bin/sh %s "$@"\n' "$(quote "$ROOT/launchers/bootstrap.sh")"
    } >"$app/Contents/MacOS/simp2puml.tmp"
    chmod +x "$app/Contents/MacOS/simp2puml.tmp"
    mv "$app/Contents/MacOS/simp2puml.tmp" "$app/Contents/MacOS/simp2puml"
    if [ -n "${created:-}" ]; then
        say "Application « simp2puml » ajoutée dans ~/Applications (Launchpad, Spotlight)."
    fi
}

PYTHON=""
mkdir -p "$DATA"
# Python mémorisé : pas de recherche (plusieurs lancements de Python) tant qu'il existe encore.
CACHE="$DATA/python.txt"
if [ -z "${SIMP2PUML_FORCE_MANAGED:-}" ] && [ -f "$CACHE" ]; then
    cached=$(head -n 1 "$CACHE")
    [ -x "$cached" ] && PYTHON=$cached
fi
if [ -z "$PYTHON" ]; then
    find_python
    [ -n "$PYTHON" ] || managed_python
    printf '%s\n' "$PYTHON" >"$CACHE"
fi
if [ -n "${SIMP2PUML_NO_SHORTCUTS:-}" ]; then
    :
elif [ "$SYSTEM" = macos ]; then
    install_macos_app || say "(application non créée dans ~/Applications)"
else
    install_linux_entry || say "(entrée de menu non créée)"
fi

if [ -t 1 ]; then
    # Depuis un terminal : ouvrir l'interface en arrière-plan pour pouvoir fermer la fenêtre.
    nohup "$PYTHON" "$START" "$@" >"$LOG" 2>&1 &
    pid=$!
    sleep 2
    if ! kill -0 "$pid" 2>/dev/null && ! wait "$pid"; then
        cat "$LOG" >&2
        fail "L'interface s'est arrêtée au démarrage (journal : $LOG)."
    fi
    say "simp2puml est ouvert. Vous pouvez fermer cette fenêtre."
elif [ "$SYSTEM" = macos ]; then
    # Depuis l'application : remplacer ce processus pour garder le nom et l'icône dans le Dock.
    exec "$PYTHON" "$START" "$@" 2>"$LOG"
else
    "$PYTHON" "$START" "$@" 2>"$LOG" || fail "L'interface s'est arrêtée (journal : $LOG)."
fi
