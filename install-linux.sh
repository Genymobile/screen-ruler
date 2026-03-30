#!/usr/bin/env bash
# install-linux.sh — Install screen-ruler binary, desktop entry, and keyboard shortcut.
#
# Usage:
#   ./install-linux.sh [-d INSTALL_DIR]
#
# The script expects a built binary at dist/screen-ruler (run pyinstaller first).
# It copies the binary into INSTALL_DIR (default: current directory), installs a
# .desktop file, and registers a global Super+Shift+R shortcut for the detected
# desktop environment (GNOME, KDE Plasma, Hyprland, Sway).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
INSTALL_DIR="$PWD"
APP_NAME="Screen Ruler"
APP_ID="screen-ruler"
SHORTCUT_DISPLAY="Super+Shift+R"

usage() {
    echo "Usage: $0 [-d INSTALL_DIR]"
    echo
    echo "  -d DIR   Install the binary into DIR (default: current directory)"
    echo "  -h       Show this help"
    exit 0
}

while getopts "d:h" opt; do
    case "$opt" in
        d) INSTALL_DIR="$OPTARG" ;;
        h) usage ;;
        *) usage ;;
    esac
done

# --- Locate the built binary ------------------------------------------------

BINARY="$SCRIPT_DIR/dist/screen-ruler"
if [[ ! -f "$BINARY" ]]; then
    echo "Error: $BINARY not found."
    echo "Build it first:  pyinstaller screen_ruler.spec"
    exit 1
fi

# --- Copy binary to install dir ---------------------------------------------

mkdir -p "$INSTALL_DIR"
INSTALL_DIR="$(cd "$INSTALL_DIR" && pwd)"   # resolve to absolute path
EXEC_PATH="$INSTALL_DIR/$APP_ID"

# Portable realpath fallback for systems without coreutils realpath.
resolve_path() { cd "$(dirname "$1")" && echo "$PWD/$(basename "$1")"; }

if [[ "$EXEC_PATH" != "$(resolve_path "$BINARY")" ]]; then
    cp "$BINARY" "$EXEC_PATH"
    echo "Installed binary → $EXEC_PATH"
else
    echo "Binary already at $EXEC_PATH"
fi
chmod +x "$EXEC_PATH"

# --- Detect desktop environment ---------------------------------------------

detect_de() {
    local de="${XDG_CURRENT_DESKTOP:-}"
    de="${de,,}"  # lowercase
    case "$de" in
        *gnome*)    echo "gnome"    ;;
        *kde*)      echo "kde"      ;;
        *hyprland*) echo "hyprland" ;;
        *sway*)     echo "sway"     ;;
        *)
            # Fallback: check running processes.
            if pgrep -x gnome-shell  &>/dev/null; then echo "gnome"
            elif pgrep -x plasmashell &>/dev/null; then echo "kde"
            elif pgrep -x Hyprland   &>/dev/null; then echo "hyprland"
            elif pgrep -x sway       &>/dev/null; then echo "sway"
            else echo "unknown"
            fi
            ;;
    esac
}

DE="$(detect_de)"
echo "Detected desktop environment: $DE"

# --- Install .desktop file --------------------------------------------------

DESKTOP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
mkdir -p "$DESKTOP_DIR"
DESKTOP_FILE="$DESKTOP_DIR/$APP_ID.desktop"

cat > "$DESKTOP_FILE" <<EOF
[Desktop Entry]
Type=Application
Name=$APP_NAME
Comment=Measure on-screen UI elements using edge detection
Exec="$EXEC_PATH"
Icon=utilities-terminal
Terminal=false
Categories=Utility;
Keywords=ruler;measure;pixel;screen;
EOF

echo "Installed desktop entry → $DESKTOP_FILE"

# Refresh the application database so the entry appears in launchers.
if command -v update-desktop-database &>/dev/null; then
    update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
fi

# --- Shortcut helpers per DE ------------------------------------------------

install_gnome_shortcut() {
    local binding="<Super><Shift>r"
    local schema="org.gnome.settings-daemon.plugins.media-keys"
    local path_prefix="/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings"

    if ! command -v gsettings &>/dev/null \
        || ! gsettings list-schemas 2>/dev/null | grep -q "^${schema}$"; then
        echo "gsettings or GNOME media-keys schema not available; skipping shortcut."
        return 1
    fi

    # Read the current list of custom-keybinding slots.
    local current
    current="$(gsettings get "$schema" custom-keybindings)"

    # Check if screen-ruler is already registered in any slot.
    local slots
    slots="$(echo "$current" | tr -d "[]' " | tr ',' '\n')"
    for slot in $slots; do
        if [[ -z "$slot" ]]; then continue; fi
        local cmd
        cmd="$(gsettings get "$schema.custom-keybinding:$slot" command 2>/dev/null || true)"
        if [[ "$cmd" == *"$APP_ID"* ]]; then
            gsettings set "$schema.custom-keybinding:$slot" command "'$EXEC_PATH'"
            gsettings set "$schema.custom-keybinding:$slot" binding "'$binding'"
            echo "Updated existing GNOME shortcut (${slot}) → $SHORTCUT_DISPLAY"
            return
        fi
    done

    # Find the next unused slot index.
    local idx=0
    while echo "$current" | grep -q "custom${idx}/"; do
        idx=$((idx + 1))
    done
    local new_slot="$path_prefix/custom${idx}/"

    if [[ "$current" == "@as []" ]]; then
        gsettings set "$schema" custom-keybindings "['$new_slot']"
    else
        local updated
        updated="$(echo "$current" | sed "s|]$|, '$new_slot']|")"
        gsettings set "$schema" custom-keybindings "$updated"
    fi

    gsettings set "$schema.custom-keybinding:$new_slot" name "'$APP_NAME'"
    gsettings set "$schema.custom-keybinding:$new_slot" command "'$EXEC_PATH'"
    gsettings set "$schema.custom-keybinding:$new_slot" binding "'$binding'"

    echo "Registered GNOME shortcut ($new_slot) → $SHORTCUT_DISPLAY"
}

install_kde_shortcut() {
    local kwrite=""
    if   command -v kwriteconfig6 &>/dev/null; then kwrite="kwriteconfig6"
    elif command -v kwriteconfig5 &>/dev/null; then kwrite="kwriteconfig5"
    fi

    if [[ -z "$kwrite" ]]; then
        echo "kwriteconfig not found; cannot register KDE shortcut automatically."
        return 1
    fi

    local config_file="kglobalshortcutsrc"
    local group="$APP_ID.desktop"
    local binding="Meta+Shift+R"

    # Write the shortcut entry.
    "$kwrite" --file "$config_file" --group "$group" \
        --key "_k_friendly_name" "$APP_NAME"
    "$kwrite" --file "$config_file" --group "$group" \
        --key "_launch" "${binding},none,Launch ${APP_NAME}"

    # Also add an Actions line to the .desktop file so KDE can map it.
    if ! grep -q "^Actions=" "$DESKTOP_FILE"; then
        printf '\nActions=_launch;\n\n[Desktop Action _launch]\nName=Launch %s\nExec="%s"\n' \
            "$APP_NAME" "$EXEC_PATH" >> "$DESKTOP_FILE"
    fi

    # Rebuild KDE's system config cache.
    if   command -v kbuildsycoca6 &>/dev/null; then kbuildsycoca6 2>/dev/null
    elif command -v kbuildsycoca5 &>/dev/null; then kbuildsycoca5 2>/dev/null
    fi

    echo "Registered KDE Plasma shortcut → $SHORTCUT_DISPLAY"
    echo "You may need to log out and back in (or run kquitapp5 kglobalaccel && kstart5 kglobalaccel) for the shortcut to activate."
}

install_hyprland_shortcut() {
    local config="${XDG_CONFIG_HOME:-$HOME/.config}/hypr/hyprland.conf"
    local begin_marker="# BEGIN Screen Ruler"
    local end_marker="# END Screen Ruler"

    if [[ ! -f "$config" ]]; then
        echo "Hyprland config not found at $config"
        return 1
    fi

    # Already has a marked block — update it; else check for legacy unmarked line.
    if grep -qF "$begin_marker" "$config"; then
        : # will be replaced below
    elif grep -q "screen-ruler" "$config"; then
        echo "Hyprland: screen-ruler binding already present in $config"
        return
    fi

    local backup="${config}.bak.$(date +%Y%m%d%H%M%S)"
    cp "$config" "$backup"

    # Remove any existing marked block, then append a fresh one.
    local tmp
    tmp="$(mktemp)"
    awk -v begin="$begin_marker" -v end="$end_marker" '
        $0 == begin { in_block=1; next }
        $0 == end   { in_block=0; next }
        !in_block   { print }
    ' "$config" > "$tmp"

    printf '\n%s\nbind = $mainMod SHIFT, R, exec, "%s"\n%s\n' \
        "$begin_marker" "$EXEC_PATH" "$end_marker" >> "$tmp"
    mv "$tmp" "$config"

    echo "Added Hyprland binding to $config → $SHORTCUT_DISPLAY"
    echo "Backup saved at $backup"
    echo "Reload your config (hyprctl reload) to activate."
}

install_sway_shortcut() {
    local config="${XDG_CONFIG_HOME:-$HOME/.config}/sway/config"
    local begin_marker="# BEGIN Screen Ruler"
    local end_marker="# END Screen Ruler"

    if [[ ! -f "$config" ]]; then
        echo "Sway config not found at $config"
        return 1
    fi

    if grep -qF "$begin_marker" "$config"; then
        : # will be replaced below
    elif grep -q "screen-ruler" "$config"; then
        echo "Sway: screen-ruler binding already present in $config"
        return
    fi

    local backup="${config}.bak.$(date +%Y%m%d%H%M%S)"
    cp "$config" "$backup"

    local tmp
    tmp="$(mktemp)"
    awk -v begin="$begin_marker" -v end="$end_marker" '
        $0 == begin { in_block=1; next }
        $0 == end   { in_block=0; next }
        !in_block   { print }
    ' "$config" > "$tmp"

    # shellcheck disable=SC2016
    printf '\n%s\nbindsym $mod+Shift+r exec "%s"\n%s\n' \
        "$begin_marker" "$EXEC_PATH" "$end_marker" >> "$tmp"
    mv "$tmp" "$config"

    echo "Added Sway binding to $config → $SHORTCUT_DISPLAY"
    echo "Backup saved at $backup"
    echo "Run 'swaymsg reload' to activate."
}

# --- Register shortcut ------------------------------------------------------

shortcut_ok=true
case "$DE" in
    gnome)    install_gnome_shortcut    || shortcut_ok=false ;;
    kde)      install_kde_shortcut      || shortcut_ok=false ;;
    hyprland) install_hyprland_shortcut || shortcut_ok=false ;;
    sway)     install_sway_shortcut     || shortcut_ok=false ;;
    *)        shortcut_ok=false ;;
esac

if [[ "$shortcut_ok" == false ]]; then
    echo
    echo "Could not register shortcut automatically for this desktop environment."
    echo "Bind '$EXEC_PATH' to your preferred shortcut manually."
    echo "See docs/manual-shortcut-setup.md for detailed instructions."
fi

echo
echo "Done. Press $SHORTCUT_DISPLAY to launch $APP_NAME."
