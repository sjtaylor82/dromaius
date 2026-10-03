DROMAIUS 0.1.0
Use Android apps on your Windows PC with your screen reader and keyboard.

Dromaius runs a real Android phone, Google's official Android emulator, out of
sight in the background, and shows each Android screen as an accessible web
page. NVDA, JAWS and Narrator can use browse mode, quick navigation keys, the
elements list and say all, and typing goes into normal edit fields.

This is an early hobby release. Please report problems at
https://github.com/sjtaylor82/dromaius/issues
If starting is slow, please include %LOCALAPPDATA%\Dromaius\startup.log,
which records how long each step took.


WHAT YOU NEED

- Windows 10 or 11, 64-bit, with 8 GB of RAM (16 GB is better).
- About 12 GB of free disk space. Dromaius downloads 2.8 GB:
    Android tools          8 MB
    Android emulator     455 MB
    Android system     2.3 GB (Google Play edition)
  Once installed, Android uses about 4 GB, plus about 4 GB for the
  "quick-start" snapshot that lets it start in seconds, plus your apps.
  Keyboard help (F1) shows the exact sizes and versions in use.
- An internet connection for the first start.
- Hardware virtualization. If Dromaius says it isn't available: press
  Windows+R, type optionalfeatures, press Enter, check "Windows Hypervisor
  Platform", press OK and restart your PC. If that doesn't help,
  virtualization may need turning on in your PC's BIOS or UEFI settings.
- A Google account, to use Google Play.


INSTALLING

1. Unzip this folder anywhere, for example into Documents. Keep the three
   files together: dromaius.exe, WebView2Loader.dll and dromaius-bridge.apk.
2. Run dromaius.exe.
3. The first time, Dromaius offers to download Android from Google. It tells
   you exactly which Android version and how much it will download. Read
   Google's licence, then choose "Accept and download".
   Progress is announced every 10 percent. A typical download takes a few
   minutes.
4. Android then starts for the first time, which takes about a minute.
   You'll arrive at "Your apps".
5. Sign in to Google once: open Play Store from Your apps and choose
   Sign in.

Later starts take a few seconds: closing Dromaius pauses Android rather than
shutting it down.


KEYS

Alt+Home                  Your apps
Alt+Left                  Back
Ctrl+L                    Find an app on Google Play (type its name or paste
                          a Google Play link)
Shift+F10 or Applications More actions for the current item, such as long press
F5                        Refresh the screen
Alt+N                     Android notifications (new ones are also announced
                          as they arrive)
F1                        Keyboard help

In edit fields, press Enter on the field (or NVDA+Space) to type, as on any
web page. Enter in a search field runs the search.

Long lists show what fits on the Android screen. Use "Show more items" at the
end of a list to load more.


GOOD TO KNOW

- Android's location is set from your PC: Windows location services if
  they're turned on (Settings, Privacy & security, Location), otherwise the
  town of your internet connection.
- Some apps refuse to run on an emulator, such as some banking and streaming
  apps.
- Games and other apps that draw their own screens can't be read.


REMOVING DROMAIUS

1. Close Dromaius, and delete the folder you unzipped.
2. Delete these folders to remove Android and your Android data, including
   your Google sign-in:
     %LOCALAPPDATA%\Dromaius
     %LOCALAPPDATA%\com.dromaius.desktop
     %USERPROFILE%\.android\avd\Dromaius.avd  (and Dromaius.ini next to it)
   You can paste each of these into the Windows+R box to open it.
