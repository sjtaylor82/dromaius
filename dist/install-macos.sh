#!/bin/bash
# Dromaius: macOS install helper
#
# Run this once after unzipping the download. It removes the quarantine flag
# macOS puts on downloaded apps (so you don't get the "can't be opened"
# warning), copies Dromaius.app to your Applications folder, and opens it.
#
# Usage, in Terminal:
#   cd ~/Downloads/Dromaius
#   bash install-macos.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
APP="$SCRIPT_DIR/Dromaius.app"
TARGET="/Applications/Dromaius.app"

if [ ! -d "$APP" ]; then
    echo "Error: Dromaius.app was not found next to this script."
    echo "Keep install-macos.sh and Dromaius.app in the same folder."
    exit 1
fi

if pgrep -xq dromaius; then
    echo "Dromaius is running. Please quit it (Cmd+Q) and run this script again."
    exit 1
fi

echo "Removing the macOS quarantine flag from Dromaius.app..."
xattr -rd com.apple.quarantine "$APP" 2>/dev/null || true

if [ -d "$TARGET" ]; then
    echo "Replacing the Dromaius already in Applications..."
    rm -rf "$TARGET"
fi
echo "Copying Dromaius.app to Applications..."
cp -R "$APP" "$TARGET"

echo "Done. Opening Dromaius."
echo "The first start downloads Android (about 2.8 GB) after you accept Google's licence."
open "$TARGET"
