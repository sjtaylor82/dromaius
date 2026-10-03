#!/bin/bash
# Dromaius: collect macOS diagnostics into one text file to send with a
# problem report. It does not collect your Google account, Android apps'
# data, or anything from your Android device's storage.
#
# Usage, in Terminal:
#   cd ~/Downloads/Dromaius
#   bash collect-macos-logs.sh

set -u

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
OUTPUT="$SCRIPT_DIR/dromaius-mac-diagnostics.txt"
DATA="$HOME/Library/Application Support/Dromaius"
ADB="$DATA/sdk/platform-tools/adb"

{
    echo "Dromaius macOS diagnostics"
    echo "Collected: $(date)"
    echo
    echo "=== macOS and hardware ==="
    sw_vers 2>&1
    echo "CPU: $(uname -m)"
    echo "Memory: $(( $(sysctl -n hw.memsize) / 1073741824 )) GB"
    echo "Hypervisor support (1 = available): $(sysctl -n kern.hv_support 2>&1)"
    echo
    echo "=== Dromaius ==="
    for app in /Applications/Dromaius.app "$SCRIPT_DIR/Dromaius.app"; do
        if [ -d "$app" ]; then
            echo "$app: version $(defaults read "$app/Contents/Info" CFBundleShortVersionString 2>&1)"
            echo "Quarantine flag: $(xattr -p com.apple.quarantine "$app" 2>/dev/null || echo none)"
            codesign --verify --deep --strict "$app" 2>&1 && echo "Signature: valid (ad-hoc)"
        fi
    done
    echo
    echo "=== Disk space ==="
    df -h "$HOME" 2>&1
    echo
    echo "=== Android files ==="
    if [ -d "$DATA/sdk" ]; then
        du -sh "$DATA/sdk"/* 2>&1
        ls "$DATA/sdk/system-images" 2>&1
    else
        echo "No Android files yet (first-run setup not finished)."
    fi
    du -sh "$HOME/.android/avd/"*.avd 2>/dev/null
    echo
    echo "=== Emulator ==="
    if [ -x "$ADB" ]; then
        "$ADB" devices 2>&1
        "$ADB" -s emulator-5580 emu avd status 2>&1
        "$ADB" -s emulator-5580 shell getprop ro.build.version.release 2>&1
    else
        echo "adb not found."
    fi
    echo
    echo "=== Start-up log ==="
    if [ -f "$DATA/startup.log" ]; then
        cat "$DATA/startup.log"
    else
        echo "No log found at: $DATA/startup.log"
    fi
    echo
    echo "=== Emulator log ==="
    if [ -f "$DATA/emulator.log" ]; then
        tail -n 300 "$DATA/emulator.log"
    else
        echo "No emulator log found."
    fi
    echo
    echo "=== Recent Dromaius crash reports ==="
    found=0
    for report in "$HOME/Library/Logs/DiagnosticReports"/dromaius*.ips \
                  "$HOME/Library/Logs/DiagnosticReports"/Dromaius*.ips \
                  "$HOME/Library/Logs/DiagnosticReports"/qemu-system*.ips; do
        if [ -f "$report" ]; then
            found=1
            echo "--- $report ---"
            tail -n 200 "$report"
        fi
    done
    [ "$found" -eq 0 ] && echo "No crash reports found."
} > "$OUTPUT" 2>&1

echo "Diagnostics saved to:"
echo "$OUTPUT"
echo "Please attach this file to your problem report."
