# Dromaius 0.1.1

This release improves app compatibility, keyboard access and text entry on
Windows and macOS.

## Highlights

- **Messenger on Windows:** Messenger is available from **Your apps** as a web
  app. Messenger searches, Play links and attempts to launch the incompatible
  ARM-only Android build are redirected to Messenger's supported website.
- **Reliable passcode entry:** rapid text updates are delivered to Android in
  order, so fixed-length passcode fields no longer require an extra digit.
- **Autocomplete:** Down Arrow in a single-line edit field is forwarded to
  Android so apps can open and navigate their suggestion lists.
- **Talk controls:** F7 presses an on-screen talk or record button once;
  Shift+F7 holds it for push-to-talk. The Mac equivalents are Fn+F7 and
  Fn+Shift+F7 on keyboards that use Fn for function keys.
- **Browser shortcuts:** Ctrl+L and Ctrl+T on Windows, or Cmd+L and Cmd+T on
  macOS, are forwarded to Android browsers. Installing from Google Play moves
  to Alt+S on Windows and Option+S on macOS.
- **Web content:** visible controls inside invisible web wrappers are retained,
  improving access to overlays and search suggestions.
- **Documentation:** the release README now has separate Windows and macOS
  requirements, installation, keyboard, troubleshooting and removal sections.

## Requirements

- Windows 10 or 11 x64 with Windows Hypervisor Platform, or macOS 12 or later
  on Apple Silicon or Intel.
- 8 GB RAM minimum, 16 GB recommended, and about 12 GB free disk space.
