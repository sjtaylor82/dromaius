# Dromaius

Run Android apps on a PC with a screen reader and keyboard.

Dromaius runs the official Android Emulator (with Google Play) in the
background and shows each Android screen as an accessible web page. NVDA, JAWS
and Narrator can use browse mode, quick navigation, the elements list and say
all. Typing goes into real edit fields and is passed on to Android.

## How it works

```
Windows / macOS                                    Android Emulator (hidden)
┌──────────────────────────────┐                  ┌───────────────────────────────┐
│ Dromaius (Rust, Tauri) │  JSON over adb   │ Bridge (Kotlin accessibility  │
│  web view: HTML + ARIA  ◄────┼──────────────────┼─ service): streams the screen,│
│  ↑ screen reader             │  forward         │  performs clicks, text, scroll│
└──────────────────────────────┘                  └───────────────────────────────┘
```

- `android-bridge/` – an Android accessibility service. It reads the screen
  through Android's accessibility API and carries out actions (click, set text,
  scroll, launch app). It only accepts connections from adb.
- `desktop/` – the desktop app. Rust starts and pauses the emulator, installs and
  enables the bridge, and turns Android's node tree into a simplified model
  (`src/mirror.rs`) that the web front end (`ui/`) renders as HTML.
- Android's home screen is replaced by the app's own **Your apps** list.

## Using it

| Windows | macOS | Action |
|---|---|---|
| Alt+H | Option+H | Your apps |
| Alt+Left | Cmd+[ | Android Back |
| Ctrl+L | Cmd+L | Install from a Google Play link or package name |
| Shift+F10 / Applications key | VoiceOver+Shift+M | More actions: long press, app-specific actions, expand |
| F5 | Cmd+R | Refresh |
| Alt+N | Option+N | Android notifications |
| [ | [ | Push to talk: start, then stop (holds the current item, or presses F12 as a hardware PTT key) |
| ] | ] | Tap and hold the current item |
| Alt+Page Down | Option+Page Down | Next screen of items in a long Android list |
| Alt+Page Up | Option+Page Up | Previous screen of items in a long Android list |
| F1 | Cmd+? | Keyboard help |

Long lists only contain what fits on the Android screen. Press **Alt+Page Down**
or **Alt+Page Up** on Windows, or **Option+Page Down** or **Option+Page Up** on
macOS, from anywhere on the screen. You can also use the buttons at the ends of
the list. In screen-reader focus mode, Up and Down Arrow move through Android
items and scroll at the ends. The Arrow keys are handled only while keyboard
focus is inside the Android screen.

The first start creates the virtual device and cold-boots Android (about a
minute). Closing the window pauses the emulator, so the next start takes a few
seconds. Run with `--shutdown-on-exit` to shut it down instead.

Android gets the PC's location at every start: Windows location services when
they're on (Settings > Privacy & security > Location, including "Let desktop
apps access your location"), which is usually accurate to a few hundred metres,
otherwise the city of your internet connection (looked up at ip-api.com).

Other options: `--install <play link>`, `--show-emulator` (show the emulator's
own window for sighted helpers), `--no-location` (leave the emulator's default
location), `--mock <snapshot.json>` (render a saved screen without Android).

## Requirements for users

- Windows 10/11 64-bit with hardware virtualization and the **Windows Hypervisor
  Platform** feature enabled, or macOS 12 or later (preview; see
  `dist/README-mac.txt` for the Mac keys and first-open step)
- 8 GB RAM (16 GB recommended), about 12 GB free disk space: Android itself
  is about 4 GB, its quick-start snapshot about 4 GB, plus your apps
- An internet connection for the first start. Dromaius then downloads Android
  from Google (about 2.8 GB; 4 GB on disk) into `%LOCALAPPDATA%\Dromaius\sdk`,
  after you accept Google's licence. No Java or developer tools are needed.
  If an Android SDK with an emulator and Google Play image is already
  installed, Dromaius uses that instead.

## Developer setup (Windows)

Tools used (all per-user, no admin needed):

| Tool | Where |
|---|---|
| Rust, `stable-x86_64-pc-windows-gnullvm` toolchain | `rustup` |
| llvm-mingw (linker / `dlltool` for that toolchain) | `%LOCALAPPDATA%\Programs\llvm-mingw-*` on `PATH` |
| Microsoft OpenJDK 21 (bridge build only) | `%LOCALAPPDATA%\Programs\jdk-21` |
| Android SDK: platform-tools, emulator, `system-images;android-37.0;google_apis_playstore;x86_64` | `%LOCALAPPDATA%\Android\Sdk` |

Build the bridge:

```bash
cd android-bridge
JAVA_HOME="$LOCALAPPDATA/Programs/jdk-21" ANDROID_HOME="$LOCALAPPDATA/Android/Sdk" ./gradlew.bat assembleDebug
```

`assembleRelease` signs the bridge with the release key in `signing/`
(`dromaius-release.jks` and `keystore.properties`). That folder is never
committed; without it, release builds fall back to the debug key. Always sign
releases with the same key, or new bridges can't update installed ones.

Build and run the desktop app. The signed bridge
(`android-bridge/app/build/outputs/apk/release/app-release.apk`, or the file
named by `DROMAIUS_EMBED_APK`) is built into the executable, which unpacks and
installs it into Android when needed; build the bridge first. Without it the
build warns, and Dromaius looks for `dromaius-bridge.apk` next to the
executable instead.

```bash
cd desktop
cargo run
```

The Rust build links the C runtime statically (`desktop/.cargo/config.toml`).
With this toolchain, `WebView2Loader.dll` (Microsoft's redistributable loader)
must ship next to the `.exe`; it is copied to `target/` by the build. The
Windows download is therefore `dromaius.exe`, `WebView2Loader.dll` and
`dist/README.txt`.

### macOS

The Mac app is built by GitHub Actions (`.github/workflows/build.yml`): run the
workflow from the Actions tab for a test build, or push a `v*` tag to attach it
to that release. It is a universal app (Apple Silicon and Intel), ad-hoc signed
but not notarized, so users open it once via Open in Finder's menu
(see `dist/README-mac.txt`). The workflow embeds the signed bridge from the
latest GitHub release, so the signing key stays off GitHub; publish the
release's `dromaius-bridge.apk` asset whenever the bridge changes.

For repeated testing from a folder shared with a Mac, use
`bash mac-test/run.sh`. It builds the current source locally instead of
downloading another Actions artifact. The prepared `mac-test` folder includes
the signed bridge and keeps each run's build, console, startup, emulator,
snapshot and crash diagnostics under `mac-test/logs`; see
`mac-test/README.txt`.

### Tests

```bash
cd desktop && cargo test
cd desktop/ui-tests && npm install && npm test
```

The Rust tests cover the Android tree simplification and setup logic; the
`ui-tests` load the real page in jsdom and check the HTML a screen reader gets
(roles, ARIA, focus, editing).

### Debugging

- Every start writes `%LOCALAPPDATA%\Dromaius\startup.log` with the time of
  each step (emulator resume or boot, bridge, first screen, location, update
  check), so slow starts can be attributed.

- `DROMAIUS_SDK_DIR` and `ANDROID_AVD_HOME` point Dromaius at other folders,
  e.g. empty ones to test first-run setup without touching your real setup.
- `DROMAIUS_DEBUG=1` saves every Android snapshot to
  `%TEMP%\dromaius-snapshot.json` (usable with `--mock`).
- `DROMAIUS_LOG_DIR` puts `startup.log`, `emulator.log`, and debug snapshots in
  a chosen directory. The Mac test launcher sets this to its per-run log folder.
- `tools/uia-dump.ps1` prints what a screen reader sees through UI Automation,
  and can send keys: `powershell -File tools\uia-dump.ps1 -Depth 30 -Keys "{TAB}"`.
- Don't run `uiautomator dump` while testing: it temporarily unbinds
  accessibility services (the bridge reconnects, but the screen goes blank
  for a moment).

## Bridge protocol

Newline-delimited JSON over `adb forward tcp:38300 localabstract:dromaius_bridge`.

From the bridge: `hello` (device, API level, home-screen package), `tree`
(windows and node trees), `apps` (launchable apps), `announce` (toasts and
announcements), `windowChanged`, `result` (reply to a request).

To the bridge: `refresh`, `apps`, `launch`, `global` (back, home, recents,
notifications, quickSettings), and `action` on a node (`click`, `longClick`,
`focus`, `setText`, `setSelection`, `setProgress`, `scrollForward`,
`scrollBackward`, `imeEnter`, `expand`, `collapse`, `dismiss`, `showOnScreen`,
`custom`).

## Known limitations

- Apps that check for a real device (some banking and streaming apps) may not
  install or run on the emulator.
- Content drawn without accessibility information (most games) can't be mirrored.
- Google sign-in has to be done once, through the mirrored Android screens.

## Licensing and trademarks

Dromaius is an independent accessibility project and is not affiliated with or
endorsed by Google. Google software is not included in Dromaius distributions;
required Android SDK packages are downloaded directly from Google only after
the user is shown and accepts every applicable package licence.

Android is a trademark of Google LLC. Google Play is a trademark of Google LLC.
