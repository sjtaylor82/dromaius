First release of Dromaius: use Android apps on your Windows PC with your screen reader and keyboard.

Dromaius runs Google's official Android emulator (Android 17 with Google Play) out of sight, and shows each Android screen as an accessible web page. NVDA, JAWS and Narrator can use browse mode, quick navigation, the elements list and say all; typing goes into normal edit fields.

**Download:** `Dromaius-0.1.0-windows-x64.zip`. Unzip it and run `dromaius.exe`. The included README.txt covers requirements, keys and how to remove Dromaius.

### Highlights
- **Your apps**: a plain list of installed apps replaces Android's home screen.
- **Find apps on Google Play** with Ctrl+L: type a name or paste a link; focus jumps to Install.
- **First-run setup** downloads Android from Google (2.8 GB) after you accept Google's licence. No Java or developer tools needed.
- **Fast starts**: closing Dromaius pauses Android, so it's back in seconds.
- **Notifications** are announced as they arrive (Alt+N opens them).
- **Location** follows your PC (Windows location services, or your internet connection's town).
- **Updates**: newer emulators and Android releases are offered on the Your apps page. Android upgrades start fresh, optionally keeping your current Android as a backup you can switch back to.

### Requirements
Windows 10 or 11 (64-bit), 8 GB RAM (16 GB recommended), about 12 GB free disk space, hardware virtualization with Windows Hypervisor Platform turned on, and a Google account.

### Known limitations
- Some apps refuse to run on an emulator (some banking and streaming apps).
- Games and apps that draw their own screens can't be read.
- Signing in to Google, and to apps, is done once through the mirrored Android screens.

This is an early hobby project; please report problems in the Issues tab.
