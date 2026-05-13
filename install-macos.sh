#!/usr/bin/env bash
# install-macos.sh — Install screen-ruler and create an Automator shortcut on macOS.
#
# Usage:
#   ./install-macos.sh [-d INSTALL_DIR]
#
# The script expects a built binary at dist/screen-ruler (run pyinstaller first).
# It copies the binary to INSTALL_DIR (default: current directory), creates an
# Automator Quick Action that launches it, and prints instructions for binding
# a keyboard shortcut in System Settings.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
INSTALL_DIR="$PWD"
APP_ID="screen-ruler"
SHORTCUT_DISPLAY="⌘⇧R (Cmd+Shift+R)"
SERVICE_NAME="Launch Screen Ruler"

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
INSTALL_DIR="$(cd "$INSTALL_DIR" && pwd)"
EXEC_PATH="$INSTALL_DIR/$APP_ID"

# Portable realpath: stock macOS may not ship coreutils realpath.
resolve_path() { cd "$(dirname "$1")" && echo "$PWD/$(basename "$1")"; }

if [[ "$EXEC_PATH" != "$(resolve_path "$BINARY")" ]]; then
    cp "$BINARY" "$EXEC_PATH"
    echo "Installed binary → $EXEC_PATH"
else
    echo "Binary already at $EXEC_PATH"
fi
chmod +x "$EXEC_PATH"

# --- Create Automator Quick Action (Service) --------------------------------

SERVICES_DIR="$HOME/Library/Services"
WORKFLOW_DIR="$SERVICES_DIR/${SERVICE_NAME}.workflow"
CONTENTS_DIR="$WORKFLOW_DIR/Contents"

mkdir -p "$CONTENTS_DIR"

# Info.plist — declares this as a Service (Quick Action) receiving no input.
cat > "$CONTENTS_DIR/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>NSServices</key>
    <array>
        <dict>
            <key>NSMenuItem</key>
            <dict>
                <key>default</key>
                <string>${SERVICE_NAME}</string>
            </dict>
            <key>NSMessage</key>
            <string>runWorkflowAsService</string>
        </dict>
    </array>
</dict>
</plist>
PLIST

# document.wflow — the Automator workflow definition: a single "Run Shell Script" action.
# Pre-compute the XML-escaped, shell-quoted command string.
COMMAND_XML="$(printf '"%s"' "$EXEC_PATH" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g')"

cat > "$CONTENTS_DIR/document.wflow" <<WFLOW
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>AMApplicationBuild</key>
    <string>523</string>
    <key>AMApplicationVersion</key>
    <string>2.10</string>
    <key>AMDocumentVersion</key>
    <string>2</string>
    <key>actions</key>
    <array>
        <dict>
            <key>action</key>
            <dict>
                <key>AMAccepts</key>
                <dict>
                    <key>Container</key>
                    <string>List</string>
                    <key>Optional</key>
                    <true/>
                    <key>Types</key>
                    <array>
                        <string>com.apple.cocoa.string</string>
                    </array>
                </dict>
                <key>AMActionVersion</key>
                <string>2.0.3</string>
                <key>AMApplication</key>
                <array>
                    <string>Automator</string>
                </array>
                <key>AMCategory</key>
                <string>AMCategoryUtilities</string>
                <key>AMIconName</key>
                <string>Automator</string>
                <key>AMKeywords</key>
                <array>
                    <string>Shell</string>
                    <string>Script</string>
                </array>
                <key>AMName</key>
                <string>Run Shell Script</string>
                <key>AMProvides</key>
                <dict>
                    <key>Container</key>
                    <string>List</string>
                    <key>Types</key>
                    <array>
                        <string>com.apple.cocoa.string</string>
                    </array>
                </dict>
                <key>ActionBundlePath</key>
                <string>/System/Library/Automator/Run Shell Script.action</string>
                <key>ActionName</key>
                <string>Run Shell Script</string>
                <key>ActionParameters</key>
                <dict>
                    <key>COMMAND_STRING</key>
                    <string>${COMMAND_XML}</string>
                    <key>CheckedForUserDefaultShell</key>
                    <true/>
                    <key>inputMethod</key>
                    <integer>1</integer>
                    <key>shell</key>
                    <string>/bin/bash</string>
                    <key>source</key>
                    <string></string>
                </dict>
                <key>BundleIdentifier</key>
                <string>com.apple.RunShellScript</string>
                <key>CFBundleVersion</key>
                <string>2.0.3</string>
                <key>CanShowSelectedItemsWhenRun</key>
                <false/>
                <key>CanShowWhenRun</key>
                <true/>
                <key>Category</key>
                <array>
                    <string>AMCategoryUtilities</string>
                </array>
                <key>Class Name</key>
                <string>RunShellScriptAction</string>
                <key>InputUUID</key>
                <string>00000000-0000-0000-0000-000000000000</string>
                <key>Keywords</key>
                <array>
                    <string>Shell</string>
                    <string>Script</string>
                </array>
                <key>OutputUUID</key>
                <string>00000000-0000-0000-0000-000000000001</string>
                <key>UUID</key>
                <string>00000000-0000-0000-0000-000000000002</string>
                <key>UnlocalizedApplications</key>
                <array>
                    <string>Automator</string>
                </array>
            </dict>
        </dict>
    </array>
    <key>connectors</key>
    <dict/>
    <key>workflowMetaData</key>
    <dict>
        <key>workflowTypeIdentifier</key>
        <string>com.apple.Automator.servicesMenu</string>
    </dict>
</dict>
</plist>
WFLOW

echo "Installed Automator Quick Action → $WORKFLOW_DIR"

# --- Print shortcut binding instructions ------------------------------------

echo
echo "To bind a keyboard shortcut:"
echo "  1. Open System Settings → Keyboard → Keyboard Shortcuts → Services"
echo "     (on older macOS: System Preferences → Keyboard → Shortcuts → Services)"
echo "  2. Find \"$SERVICE_NAME\" under General"
echo "  3. Click \"Add Shortcut\" and press your preferred keys (e.g. $SHORTCUT_DISPLAY)"
echo
echo "Done."
