#!/bin/bash
# Build the current shared source on this Mac, run it, and keep every useful
# diagnostic in mac-test/logs. Run with: bash mac-test/run.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
STAMP="$(date '+%Y-%m-%d_%H-%M-%S')"
LOG_DIR="$SCRIPT_DIR/logs/$STAMP"
# Compilation creates thousands of small files. Keep those on the Mac rather
# than the SMB share; network filesystem latency can turn the first build into
# a very long one. Source and diagnostic logs remain in the shared workspace.
TARGET_DIR="${DROMAIUS_MAC_TARGET_DIR:-$HOME/Library/Caches/Dromaius/source-build}"
BRIDGE="$SCRIPT_DIR/dromaius-bridge.apk"

mkdir -p "$LOG_DIR"
mkdir -p "$TARGET_DIR"
chmod 700 "$SCRIPT_DIR/logs" "$LOG_DIR" 2>/dev/null || true
printf '%s\n' "$LOG_DIR" > "$SCRIPT_DIR/logs/latest.txt"
chmod 600 "$SCRIPT_DIR/logs/latest.txt" 2>/dev/null || true

if [ "$(uname -s)" != "Darwin" ]; then
    echo "This launcher must be run on macOS."
    exit 1
fi

MACOS_MAJOR="$(sw_vers -productVersion | cut -d. -f1)"
if [ "$MACOS_MAJOR" -lt 12 ]; then
    echo "Dromaius requires macOS 12 or later (this Mac has $(sw_vers -productVersion))."
    exit 1
fi

MAC_ARCH="$(uname -m)"
case "$MAC_ARCH" in
    arm64|x86_64) ;;
    *)
        echo "Unsupported Mac architecture: $MAC_ARCH (expected arm64 or x86_64)."
        exit 1
        ;;
esac

if [ "$(sysctl -n kern.hv_support 2>/dev/null || echo 0)" != "1" ]; then
    echo "Hardware virtualization is unavailable, so the Android emulator cannot start."
    exit 1
fi

if ! xcode-select -p >/dev/null 2>&1; then
    echo "Apple's command-line tools are required. Run: xcode-select --install"
    exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "Rust is not installed. Downloading the official minimal Rust toolchain..."
    RUSTUP_INSTALLER="$LOG_DIR/rustup-init.sh"
    curl --proto '=https' --tlsv1.2 --fail --silent --show-error \
        https://sh.rustup.rs --output "$RUSTUP_INSTALLER"
    sh "$RUSTUP_INSTALLER" -y --profile minimal
    rm -f "$RUSTUP_INSTALLER"
    # rustup updates future shells; make Cargo available to this run as well.
    if [ -f "$HOME/.cargo/env" ]; then
        . "$HOME/.cargo/env"
    else
        export PATH="$HOME/.cargo/bin:$PATH"
    fi
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "Rust installation completed, but Cargo could not be found. Open a new Terminal and run this script again."
    exit 1
fi

if [ ! -f "$BRIDGE" ]; then
    echo "The Android bridge is missing: $BRIDGE"
    echo "Copy dromaius-bridge.apk into mac-test, then run this script again."
    exit 1
fi

{
    echo "Dromaius macOS test run"
    echo "Started: $(date)"
    echo "Repository: $ROOT"
    echo "Local build cache: $TARGET_DIR"
    echo "Commit: $(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo uncommitted-copy)"
    sw_vers
    echo "Architecture: $MAC_ARCH"
    echo "Hypervisor support: $(sysctl -n kern.hv_support)"
    echo "Memory: $(( $(sysctl -n hw.memsize) / 1073741824 )) GB"
    cargo --version
    rustc --version
} > "$LOG_DIR/system.txt" 2>&1

export CARGO_TARGET_DIR="$TARGET_DIR"
export DROMAIUS_EMBED_APK="$BRIDGE"
export DROMAIUS_BRIDGE_APK="$BRIDGE"
export DROMAIUS_LOG_DIR="$LOG_DIR"
export DROMAIUS_DEBUG=1

collect_crashes() {
    {
        echo "Recent Dromaius and emulator crash-report tails"
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
        [ "$found" -eq 0 ] && echo "No matching crash reports found."
    } > "$LOG_DIR/crash-reports.txt" 2>&1

    # The Play Store reports installation failures to Android's logs but often
    # shows users only a generic "see common ways to fix the problem" message.
    # Capture the useful device-side evidence while the paused emulator is
    # still reachable, without including the full (and potentially private)
    # system log.
    SDK_ROOT="$HOME/Library/Application Support/Dromaius/sdk"
    ADB="$SDK_ROOT/platform-tools/adb"
    SERIAL="emulator-5580"
    if [ -x "$ADB" ] && "$ADB" -s "$SERIAL" get-state >/dev/null 2>&1; then
        {
            echo "Mac time:     $(date)"
            echo "Android time: $("$ADB" -s "$SERIAL" shell date 2>&1)"
            echo
            echo "Android storage"
            "$ADB" -s "$SERIAL" shell df -h /data /cache 2>&1 || true
            echo
            echo "Android memory"
            "$ADB" -s "$SERIAL" shell head -3 /proc/meminfo 2>&1 || true
            echo
            echo "Download and package installation diagnostics"
            "$ADB" -s "$SERIAL" logcat -d -v threadtime \
                PackageInstaller:I PackageManager:I DownloadManager:I \
                Finsky:I Phonesky:I vending:I GmsCheckin:I Checkin:I '*:W' 2>&1 | tail -n 6000
        } > "$LOG_DIR/android-install.log" 2>&1
    else
        echo "The Android emulator was not reachable when diagnostics were collected." \
            > "$LOG_DIR/android-install.log"
    fi
    chmod 600 "$LOG_DIR"/* 2>/dev/null || true
    echo "Logs for this run: $LOG_DIR"
}
trap collect_crashes EXIT
# Closing the Terminal window (HUP) or Ctrl+C would otherwise skip the EXIT
# handler on some shells; turn those into a normal exit so logs are kept.
trap 'exit 130' INT
trap 'exit 129' HUP
trap 'exit 143' TERM

echo "Building the current Dromaius source for this Mac..."
(
    cd "$ROOT/desktop"
    cargo build --release
) 2>&1 | tee "$LOG_DIR/build.log"

APP="$TARGET_DIR/release/dromaius"
if [ ! -x "$APP" ]; then
    echo "Build completed but the executable was not found: $APP"
    exit 1
fi

# Google Play's background installer can keep "no account" from before the
# first Google sign-in, failing every install with status 1408. Restarting
# Play Store clears it.
PLAY_ADB="$HOME/Library/Application Support/Dromaius/sdk/platform-tools/adb"
if [ -x "$PLAY_ADB" ] && "$PLAY_ADB" -s emulator-5580 get-state >/dev/null 2>&1; then
    "$PLAY_ADB" -s emulator-5580 emu avd start >/dev/null 2>&1 || true
    "$PLAY_ADB" -s emulator-5580 shell am force-stop com.android.vending >/dev/null 2>&1 || true
    echo "Restarted Google Play Store."
fi

echo "Starting Dromaius. Quit it normally when testing is finished."
"$APP" 2>&1 | tee "$LOG_DIR/console.log"
