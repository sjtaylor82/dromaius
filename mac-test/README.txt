DROMAIUS MAC TESTING

This folder builds and runs the current shared source directly on a Mac. You
do not need to download a new GitHub Actions build after every code change.

One-time Mac requirements:

- macOS 12 or later, on Apple Silicon or Intel.
- Apple's command-line tools. Install with: xcode-select --install
- An internet connection. If Rust is missing, the launcher downloads the
  official minimal Rust toolchain automatically from https://rustup.rs.
- dromaius-bridge.apk in this folder. The Windows workspace is prepared with
  the current signed bridge already copied here.

To test, open Terminal in the repository and run:

  bash mac-test/run.sh

The script builds only for that Mac's architecture, then runs Dromaius in the
foreground. Quit Dromaius normally when finished.

Compiled dependencies are cached locally on the Mac under
~/Library/Caches/Dromaius/source-build. They are not compiled across the SMB
share, and later builds reuse them.

Every run writes to a separate directory under mac-test/logs. The file
mac-test/logs/latest.txt contains the newest directory path. That directory
includes build.log, console.log, startup.log, emulator.log, system.txt, the
latest accessibility snapshot, and recent crash-report tails.

The accessibility snapshot can contain text currently displayed by Android,
including notifications. Password-field values are excluded. Treat the logs as
private and inspect them before sharing them in a public issue.

Android itself remains in ~/Library/Application Support/Dromaius, so its
multi-gigabyte download and quick-start state are reused between builds.
