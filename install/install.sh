#!/bin/sh
# Installeur de zrepo-rs pour Linux et macOS.
#
#   curl -fsSL https://github.com/jpierre-amar/zrepo-rs/releases/latest/download/install.sh | sh
#   curl -fsSL .../install.sh | sh -s -- --yes --dir ~/bin --version v0.3.3 --no-setup
#
# Options :
#   -y, --yes          aucune question (valeurs par défaut)
#   --dir <chemin>     dossier d'installation du binaire (défaut : ~/.local/bin)
#   --version <vX.Y.Z> version à installer (défaut : la dernière)
#   --no-setup         ne pas lancer « zrepo-rs setup » à la fin
#   -h, --help         cette aide
#
# Variables : ZREPO_RS_INSTALL_DIR, ZREPO_RS_VERSION.
# Aucune commande n'est lancée avec sudo. Rien n'est écrit en dehors de $HOME
# (ou du dossier choisi) et les fichiers du shell ne sont modifiés qu'avec accord.

set -eu

REPO="jpierre-amar/zrepo-rs"
GITHUB="https://github.com/$REPO"

# ---------------------------------------------------------------- affichage --

if [ -t 1 ] && [ -z "${NO_COLOR:-}" ] && [ "${TERM:-}" != "dumb" ]; then
    C_RESET=$(printf '\033[0m')
    C_BOLD=$(printf '\033[1m')
    C_DIM=$(printf '\033[2m')
    C_RED=$(printf '\033[31m')
    C_GREEN=$(printf '\033[32m')
    C_YELLOW=$(printf '\033[33m')
    C_BLUE=$(printf '\033[34m')
else
    C_RESET='' C_BOLD='' C_DIM='' C_RED='' C_GREEN='' C_YELLOW='' C_BLUE=''
fi

step() { printf '%s==>%s %s%s%s\n' "$C_BLUE" "$C_RESET" "$C_BOLD" "$*" "$C_RESET"; }
info() { printf '    %s\n' "$*"; }
ok()   { printf '%s  ✓%s %s\n' "$C_GREEN" "$C_RESET" "$*"; }
warn() { printf '%s  !%s %s\n' "$C_YELLOW" "$C_RESET" "$*" >&2; }
die()  { printf '%serreur :%s %s\n' "$C_RED" "$C_RESET" "$*" >&2; exit 1; }

usage() {
    cat <<'EOF_USAGE'
Usage : install.sh [options]

  -y, --yes          aucune question (valeurs par défaut)
  --dir <chemin>     dossier d'installation du binaire (défaut : ~/.local/bin)
  --version <vX.Y.Z> version à installer (défaut : la dernière)
  --no-setup         ne pas lancer « zrepo-rs setup » à la fin
  -h, --help         cette aide

Variables : ZREPO_RS_INSTALL_DIR, ZREPO_RS_VERSION.
EOF_USAGE
}

# ------------------------------------------------------------------- options --

ASSUME_YES=0
RUN_SETUP=1
INSTALL_DIR="${ZREPO_RS_INSTALL_DIR:-}"
VERSION="${ZREPO_RS_VERSION:-}"
DIR_FROM_USER=0
[ -n "$INSTALL_DIR" ] && DIR_FROM_USER=1

while [ $# -gt 0 ]; do
    case "$1" in
        -y|--yes) ASSUME_YES=1 ;;
        --no-setup) RUN_SETUP=0 ;;
        --dir) [ $# -ge 2 ] || die "--dir attend un chemin"; INSTALL_DIR="$2"; DIR_FROM_USER=1; shift ;;
        --dir=*) INSTALL_DIR="${1#--dir=}"; DIR_FROM_USER=1 ;;
        --version) [ $# -ge 2 ] || die "--version attend une version (ex. v0.3.3)"; VERSION="$2"; shift ;;
        --version=*) VERSION="${1#--version=}" ;;
        -h|--help) usage; exit 0 ;;
        *) die "option inconnue : $1 (voir --help)" ;;
    esac
    shift
done

[ -n "${HOME:-}" ] || die "la variable HOME n'est pas définie"

case "$VERSION" in
    ''|latest) VERSION='' ;;
    v*) ;;
    *) VERSION="v$VERSION" ;;
esac

# --------------------------------------------------------------- terminal ----

# En « curl | sh », l'entrée standard est le script : on lit /dev/tty.
HAS_TTY=0
if [ "$ASSUME_YES" -eq 0 ] && (: </dev/tty) 2>/dev/null; then
    HAS_TTY=1
fi

# ask <question> <défaut> : écrit la réponse dans REPLY
ask() {
    REPLY="$2"
    if [ "$HAS_TTY" -eq 1 ]; then
        printf '%s?%s %s %s[%s]%s ' "$C_BLUE" "$C_RESET" "$1" "$C_DIM" "$2" "$C_RESET" >/dev/tty
        IFS= read -r REPLY </dev/tty || REPLY=''
        [ -n "$REPLY" ] || REPLY="$2"
    fi
}

# confirm <question> : vrai si l'utilisateur répond oui (défaut non)
confirm() {
    [ "$HAS_TTY" -eq 1 ] || return 1
    printf '%s?%s %s %s[o/N]%s ' "$C_BLUE" "$C_RESET" "$1" "$C_DIM" "$C_RESET" >/dev/tty
    IFS= read -r _ans </dev/tty || _ans=''
    case "$_ans" in o|O|oui|Oui|OUI|y|Y|yes) return 0 ;; *) return 1 ;; esac
}

# ------------------------------------------------------------------ outils ----

need() { command -v "$1" >/dev/null 2>&1; }

if need curl; then
    DL=curl
elif need wget; then
    DL=wget
else
    DL=''
fi

# download <url> <fichier>
download() {
    case "$DL" in
        curl) curl -fsSL --proto '=https' --tlsv1.2 --retry 3 -o "$2" "$1" ;;
        wget) wget -q --https-only -O "$2" "$1" ;;
        *) die "ni curl ni wget n'est installé" ;;
    esac
}

sha256_of() {
    if need sha256sum; then
        sha256sum "$1" | awk '{print $1}'
    elif need shasum; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        die "ni sha256sum ni shasum n'est disponible : impossible de vérifier l'archive"
    fi
}

# Échappe une chaîne pour la partie remplacement de sed (délimiteur |).
sed_escape() { printf '%s' "$1" | sed -e 's/[\\|&]/\\&/g'; }

TMP=$(mktemp -d 2>/dev/null || mktemp -d -t zrepo-rs)
cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# ----------------------------------------------------------------- plateforme --

OS=$(uname -s)
ARCH=$(uname -m)
case "$ARCH" in
    x86_64|amd64) ARCH=x86_64 ;;
    aarch64|arm64) ARCH=aarch64 ;;
    *) die "architecture non prise en charge : $ARCH (x86_64 ou aarch64 attendu)" ;;
esac

case "$OS" in
    Linux)
        TARGET="$ARCH-unknown-linux-gnu"
        if ls /lib/ld-musl-* >/dev/null 2>&1 && ! ls /lib*/ld-linux* /lib/*/ld-linux* >/dev/null 2>&1; then
            warn "système musl détecté (Alpine ?) : le binaire publié vise glibc et risque de ne pas démarrer."
        fi
        ;;
    Darwin)
        # Terminal sous Rosetta : on préfère le binaire natif Apple Silicon.
        if [ "$ARCH" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
            ARCH=aarch64
        fi
        TARGET="$ARCH-apple-darwin"
        ;;
    MINGW*|MSYS*|CYGWIN*)
        die "sous Windows, utilisez PowerShell : irm $GITHUB/releases/latest/download/install.ps1 | iex" ;;
    *) die "système non pris en charge : $OS" ;;
esac

ARCHIVE="zrepo-rs-$TARGET.tar.gz"
if [ -n "$VERSION" ]; then
    BASE_URL="$GITHUB/releases/download/$VERSION"
else
    BASE_URL="$GITHUB/releases/latest/download"
fi

printf '\n%szrepo-rs%s — installation (%s, %s)\n\n' "$C_BOLD" "$C_RESET" "$TARGET" "${VERSION:-dernière version}"

# ------------------------------------------------------------ dossier cible --

DEFAULT_DIR="$HOME/.local/bin"
if [ "$DIR_FROM_USER" -eq 0 ]; then
    if [ "$HAS_TTY" -eq 1 ]; then
        ask "Dossier d'installation du binaire" "$DEFAULT_DIR"
        INSTALL_DIR="$REPLY"
    else
        INSTALL_DIR="$DEFAULT_DIR"
    fi
fi

# ~ et chemins relatifs (tilde littéral saisi par l'utilisateur)
# shellcheck disable=SC2088
case "$INSTALL_DIR" in
    "~") INSTALL_DIR="$HOME" ;;
    "~/"*) INSTALL_DIR="$HOME/${INSTALL_DIR#"~/"}" ;;
esac
case "$INSTALL_DIR" in
    /*) ;;
    *) INSTALL_DIR="$(pwd)/$INSTALL_DIR" ;;
esac
INSTALL_DIR="${INSTALL_DIR%/}"
[ -n "$INSTALL_DIR" ] || INSTALL_DIR="/"
# macOS : une mise à jour lancée depuis l'app vise le dossier du bundle, que
# l'installation recrée. Le lien du binaire va alors dans le dossier par défaut.
if [ "$OS" = Darwin ]; then
    case "$INSTALL_DIR" in
        *.app/Contents/MacOS) INSTALL_DIR="$DEFAULT_DIR" ;;
    esac
fi

mkdir -p "$INSTALL_DIR" 2>/dev/null || die "impossible de créer $INSTALL_DIR (droits ?). Choisissez un dossier à vous, sans sudo."
[ -w "$INSTALL_DIR" ] || die "$INSTALL_DIR n'est pas accessible en écriture. Choisissez un dossier à vous, sans sudo."
BIN="$INSTALL_DIR/zrepo-rs"

# ------------------------------------------------------- téléchargement ------

if [ -n "${ZREPO_RS_LOCAL_ARCHIVE:-}" ]; then
    # Mode test : archive locale, pas de réseau.
    step "Archive locale : $ZREPO_RS_LOCAL_ARCHIVE"
    [ -f "$ZREPO_RS_LOCAL_ARCHIVE" ] || die "archive introuvable : $ZREPO_RS_LOCAL_ARCHIVE"
    cp "$ZREPO_RS_LOCAL_ARCHIVE" "$TMP/$ARCHIVE"
    if [ -f "$ZREPO_RS_LOCAL_ARCHIVE.sha256" ]; then
        cp "$ZREPO_RS_LOCAL_ARCHIVE.sha256" "$TMP/$ARCHIVE.sha256"
    fi
else
    [ -n "$DL" ] || die "ni curl ni wget n'est installé"
    step "Téléchargement de $ARCHIVE"
    info "$C_DIM$BASE_URL/$ARCHIVE$C_RESET"
    download "$BASE_URL/$ARCHIVE" "$TMP/$ARCHIVE" \
        || die "téléchargement impossible. La version ${VERSION:-la plus récente} existe-t-elle pour $TARGET ?"
    download "$BASE_URL/$ARCHIVE.sha256" "$TMP/$ARCHIVE.sha256" \
        || die "somme de contrôle introuvable ($ARCHIVE.sha256)"
fi

if [ -f "$TMP/$ARCHIVE.sha256" ]; then
    step "Vérification de la somme SHA-256"
    expected=$(awk '{print $1; exit}' "$TMP/$ARCHIVE.sha256" | tr 'A-F' 'a-f')
    actual=$(sha256_of "$TMP/$ARCHIVE")
    [ -n "$expected" ] || die "fichier .sha256 vide"
    [ "$expected" = "$actual" ] || die "somme SHA-256 incorrecte (attendu $expected, obtenu $actual). Installation annulée."
    ok "somme vérifiée"
else
    warn "aucun fichier .sha256 à côté de l'archive locale : vérification sautée"
fi

mkdir "$TMP/x"
tar -xzf "$TMP/$ARCHIVE" -C "$TMP/x" || die "archive illisible"
SRC=$(find "$TMP/x" -type f -name zrepo-rs | head -n 1)
[ -n "$SRC" ] || die "binaire zrepo-rs absent de l'archive"

# ---------------------------------------------------------------- installation --

if [ "$OS" = Darwin ]; then
    # macOS : le binaire vit dans le bundle et $BIN devient un lien vers lui
    # (install_macos_app). Le trousseau ne voit ainsi qu'une seule application.
    VERSION_BIN="$SRC"
else
    step "Installation dans $INSTALL_DIR"
    # Copie puis renommage : remplace proprement un binaire en cours d'exécution.
    cp "$SRC" "$BIN.tmp.$$"
    chmod 755 "$BIN.tmp.$$"
    mv -f "$BIN.tmp.$$" "$BIN"
    ok "$BIN"
    VERSION_BIN="$BIN"
fi

INSTALLED_VERSION=$("$VERSION_BIN" --version 2>/dev/null | awk 'NR==1{print $NF}' | sed 's/^v//') || INSTALLED_VERSION=''
case "$INSTALLED_VERSION" in
    [0-9]*.[0-9]*) ;;
    *) INSTALLED_VERSION="${VERSION#v}"; [ -n "$INSTALLED_VERSION" ] || INSTALLED_VERSION="0.0.0" ;;
esac

# Fichiers d'intégration : assets de la release, vérifiés contre SHA256SUMS
# (une somme fausse arrête l'installation). Sans SHA256SUMS, ou pour un fichier
# qui n'y figure pas, rien n'est téléchargé : .desktop et Info.plist sont
# générés localement, l'icône est omise.
SUMS_STATE=""
release_sums() {
    if [ -z "$SUMS_STATE" ]; then
        if download "$BASE_URL/SHA256SUMS" "$TMP/SHA256SUMS" 2>/dev/null && [ -s "$TMP/SHA256SUMS" ]; then
            SUMS_STATE=ok
        else
            SUMS_STATE=absent
            warn "SHA256SUMS introuvable dans la release : fichiers d'intégration générés localement"
        fi
    fi
    [ "$SUMS_STATE" = ok ]
}

# fetch_pkg <nom> <destination>
fetch_pkg() {
    if [ -n "${ZREPO_RS_LOCAL_PACKAGING:-}" ]; then
        [ -f "$ZREPO_RS_LOCAL_PACKAGING/$1" ] && cp "$ZREPO_RS_LOCAL_PACKAGING/$1" "$2"
        return
    fi
    [ -n "${ZREPO_RS_LOCAL_ARCHIVE:-}" ] && return 1
    release_sums || return 1
    pkg_expected=$(awk -v f="$1" '$2 == f || $2 == "*" f { print $1; exit }' "$TMP/SHA256SUMS" | tr 'A-F' 'a-f')
    if [ -z "$pkg_expected" ]; then
        warn "$1 absent de SHA256SUMS : ignoré"
        return 1
    fi
    download "$BASE_URL/$1" "$2" 2>/dev/null || return 1
    pkg_actual=$(sha256_of "$2")
    [ "$pkg_expected" = "$pkg_actual" ] \
        || die "somme SHA-256 incorrecte pour $1 (attendu $pkg_expected, obtenu $pkg_actual) : fichier refusé, installation interrompue. Le binaire, vérifié, est déjà en place ; relancez l'installation."
}

install_linux_desktop() {
    data="${XDG_DATA_HOME:-$HOME/.local/share}"
    apps="$data/applications"
    icons="$data/icons/hicolor/256x256/apps"
    step "Intégration au bureau"

    if ! fetch_pkg zrepo-rs.desktop "$TMP/zrepo-rs.desktop"; then
        cat >"$TMP/zrepo-rs.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=zrepo-rs
GenericName=Lanceur de projets
Comment=Ouvrir un projet local, distant ou GitHub dans Zed ou Herdr
Exec=@ZREPO_RS_EXEC@
TryExec=@ZREPO_RS_BIN@
Icon=zrepo-rs
Terminal=false
Categories=Development;
StartupWMClass=zrepo-rs
EOF
    fi
    # Exec : guillemets si le chemin contient des espaces (spéc. Desktop Entry).
    case "$BIN" in
        *' '*) exec_val="\"$BIN\"" ;;
        *) exec_val="$BIN" ;;
    esac
    mkdir -p "$apps"
    sed -e "s|^Exec=.*|Exec=$(sed_escape "$exec_val")|" \
        -e "s|^TryExec=.*|TryExec=$(sed_escape "$BIN")|" \
        "$TMP/zrepo-rs.desktop" >"$apps/zrepo-rs.desktop"
    chmod 644 "$apps/zrepo-rs.desktop"
    ok "$apps/zrepo-rs.desktop"

    if fetch_pkg zrepo-rs.png "$TMP/zrepo-rs.png"; then
        mkdir -p "$icons"
        cp "$TMP/zrepo-rs.png" "$icons/zrepo-rs.png"
        ok "$icons/zrepo-rs.png"
        # Pas de gtk-update-icon-cache ici : un cache utilisateur périmé masquerait d'autres icônes.
        touch "$data/icons/hicolor" 2>/dev/null || true
    else
        warn "icône non téléchargée : le lanceur utilisera une icône générique"
    fi
    if need update-desktop-database; then
        update-desktop-database -q "$apps" >/dev/null 2>&1 || true
    fi
}

install_macos_app() {
    app="$HOME/Applications/zrepo-rs.app"
    step "Création de $app"
    rm -rf "$app"
    app_bin="$app/Contents/MacOS/zrepo-rs"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    cp "$SRC" "$app_bin"
    chmod 755 "$app_bin"

    if ! fetch_pkg Info.plist.in "$TMP/Info.plist.in"; then
        cat >"$TMP/Info.plist.in" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>zrepo-rs</string>
  <key>CFBundleIdentifier</key><string>io.github.jpierre-amar.zrepo-rs</string>
  <key>CFBundleName</key><string>zrepo-rs</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>@VERSION@</string>
  <key>CFBundleVersion</key><string>@VERSION@</string>
  <key>CFBundleIconFile</key><string>zrepo-rs</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
    fi
    sed "s|@VERSION@|$(sed_escape "$INSTALLED_VERSION")|g" "$TMP/Info.plist.in" >"$app/Contents/Info.plist"

    # Icône .icns construite localement avec les outils de macOS.
    if fetch_pkg zrepo-rs.png "$TMP/zrepo-rs.png" && need sips && need iconutil; then
        set_dir="$TMP/zrepo-rs.iconset"
        mkdir -p "$set_dir"
        for s in 16 32 128 256; do
            sips -z "$s" "$s" "$TMP/zrepo-rs.png" --out "$set_dir/icon_${s}x${s}.png" >/dev/null 2>&1 || true
        done
        for s in 16 32 128; do
            d=$((s * 2))
            sips -z "$d" "$d" "$TMP/zrepo-rs.png" --out "$set_dir/icon_${s}x${s}@2x.png" >/dev/null 2>&1 || true
        done
        iconutil -c icns "$set_dir" -o "$app/Contents/Resources/zrepo-rs.icns" >/dev/null 2>&1 \
            || warn "icône .icns non générée"
    fi

    if need xattr; then
        info "Retrait de l'attribut de quarantaine (xattr -dr com.apple.quarantine) : le binaire n'est pas notarisé."
        xattr -dr com.apple.quarantine "$app" 2>/dev/null || true
    fi
    # Signature ad hoc du bundle (exigée sur Apple Silicon, sans compte développeur).
    if need codesign; then
        codesign --force --sign - "$app" >/dev/null 2>&1 || warn "signature ad hoc du bundle impossible"
    fi
    ok "$app"

    # Après la signature : le lien et l'app lancent le même binaire signé, donc
    # le jeton enregistré par « zrepo-rs setup » reste lisible depuis le Finder
    # sans nouvelle demande du trousseau. Lien temporaire puis renommage : un
    # ancien binaire (installé par une version précédente) est remplacé d'un coup.
    step "Installation dans $INSTALL_DIR"
    ln -s "$app_bin" "$BIN.tmp.$$"
    mv -f "$BIN.tmp.$$" "$BIN"
    ok "$BIN → $app_bin"
    info "Pour l'ouvrir au clavier, voir le README (app Raccourcis)."
}

case "$OS" in
    Linux) install_linux_desktop ;;
    Darwin) install_macos_app ;;
esac

# ------------------------------------------------------------------- PATH ----

case ":${PATH:-}:" in
    *":$INSTALL_DIR:"*) IN_PATH=1 ;;
    *) IN_PATH=0 ;;
esac

if [ "$IN_PATH" -eq 0 ]; then
    shell_name=$(basename "${SHELL:-sh}")
    case "$shell_name" in
        zsh)  rc="${ZDOTDIR:-$HOME}/.zshrc"; line="export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
        bash) rc="$HOME/.bashrc"; line="export PATH=\"$INSTALL_DIR:\$PATH\""
              [ "$OS" = Darwin ] && rc="$HOME/.bash_profile" ;;
        fish) rc="$HOME/.config/fish/config.fish"; line="fish_add_path \"$INSTALL_DIR\"" ;;
        *)    rc="$HOME/.profile"; line="export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
    esac
    warn "$INSTALL_DIR n'est pas dans votre PATH."
    info "Ligne à ajouter dans $rc :"
    info "  $C_BOLD$line$C_RESET"
    if [ -f "$rc" ] && grep -F "$line" "$rc" >/dev/null 2>&1; then
        info "Elle y figure déjà : ouvrez un nouveau terminal."
    elif confirm "L'ajouter maintenant à $rc ?"; then
        mkdir -p "$(dirname "$rc")"
        printf '\n# zrepo-rs\n%s\n' "$line" >>"$rc"
        ok "ajoutée à $rc (effective dans un nouveau terminal)"
    else
        info "Fichier non modifié."
    fi
fi

# ------------------------------------------------------------------ setup ----

printf '\n%szrepo-rs est installé.%s\n' "$C_GREEN$C_BOLD" "$C_RESET"

if [ "$RUN_SETUP" -eq 1 ]; then
    if [ "$OS" = Linux ] && [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
        warn "aucun affichage graphique détecté : lancez « zrepo-rs setup » depuis votre session de bureau."
    else
        step "Lancement de l'assistant : zrepo-rs setup"
        # 0 = configuration enregistrée ; 130 = fermé avant l'enregistrement.
        setup_status=0
        if (: </dev/tty) 2>/dev/null; then
            "$BIN" setup </dev/tty || setup_status=$?
        else
            "$BIN" setup </dev/null || setup_status=$?
        fi
        case "$setup_status" in
            0) ok "configuration enregistrée" ;;
            130) info "assistant fermé sans enregistrer : relancez « zrepo-rs setup » quand vous voulez." ;;
            *) warn "l'assistant s'est terminé avec une erreur ($setup_status) ; relancez « zrepo-rs setup »." ;;
        esac
    fi
else
    info "Configurez-le avec : ${C_BOLD}zrepo-rs setup${C_RESET}"
fi
