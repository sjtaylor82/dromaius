DROMAIUS 0.1.2

Use Android apps on Windows or macOS with your screen reader and keyboard.

Dromaius is an independent accessibility project and is not affiliated with or
endorsed by Google. Android and Google Play are trademarks of Google LLC.

Dromaius runs Google's official Android Emulator in the background and turns
Android screens into accessible web pages. On Windows, NVDA, JAWS and Narrator
can use browse mode, quick-navigation keys, the elements list and Say All. On
macOS, VoiceOver can use its usual web navigation. You can also type into
Android apps with your computer keyboard.

Dromaius is an early hobby release. Please report problems at:
https://github.com/sjtaylor82/dromaius/issues


COMMON REQUIREMENTS

- 8 GB of RAM; 16 GB recommended
- About 12 GB of free disk space
- An internet connection for the first setup
- Hardware virtualization
- A Google account only if you want to use Google Play

You do not need Java, Android Studio or the Android SDK. Dromaius downloads
and manages Android for you. The first setup downloads about 2.8 GB.

Android's first start may take about a minute. Later starts should take only a
few seconds because closing Dromaius pauses Android instead of shutting it down.


WINDOWS REQUIREMENTS

- 64-bit Windows 10 or 11
- Windows Hypervisor Platform enabled
- NVDA, JAWS or Narrator


INSTALLING ON WINDOWS

1. Extract the Zip file.
2. Open the extracted folder and run dromaius.exe.
3. Accept Google's Android licence when prompted and wait for the download.


WINDOWS KEYBOARD COMMANDS

Alt+H                     Your apps
Alt+Left                  Back
Alt+S                     Find an app on Google Play
Ctrl+L                    In a browser such as Chrome, address bar
Ctrl+T                    In a browser such as Chrome, new tab
Shift+F10 or Applications More actions, including long press
F5                        Refresh
Alt+N                     Android notifications
Alt+Page Down             Next screen of items in a long Android list
Alt+Page Up               Previous screen of items in a long Android list
F1                        Keyboard help
F7                        Talk: presses an on-screen talk or record button
                          once; without one, toggles the hardware PTT key.
Shift+F7                  Push to talk: holds the on-screen talk or record
                          button until pressed again.
F8                        Tap and hold (long press) the current item

Long lists show only what currently fits on Android's screen. Press Alt+Page
Down or Alt+Page Up from anywhere on the screen to move through them. You can
also turn off browse mode while focused on an Android item. Up and Down Arrow
then move through the items and scroll Android at either end.


WINDOWS NOTES AND TROUBLESHOOTING

- Messenger (opens in browser) is always in Your apps. Messenger searches and
  Play links open its website instead of installing the ARM-only Android build,
  which crashes in the emulator's translation layer.
- Dromaius uses Windows location services when available, or the approximate
  location of your internet connection.
- If virtualization is unavailable, press Windows+R, type optionalfeatures,
  press Enter, turn on Windows Hypervisor Platform, and restart your PC.
- If startup is unusually slow, include this file with your bug report:
  %LOCALAPPDATA%\Dromaius\startup.log


REMOVING DROMAIUS FROM WINDOWS

Delete the extracted Dromaius folder. To also remove Android, its installed
apps and your Google sign-in, delete:

%LOCALAPPDATA%\Dromaius
%LOCALAPPDATA%\com.dromaius.desktop
%USERPROFILE%\.android\avd\Dromaius.avd
%USERPROFILE%\.android\avd\Dromaius.ini


MACOS REQUIREMENTS

- macOS 12 (Monterey) or later
- Apple Silicon (M1 or later) or Intel
- VoiceOver

The Mac build is a preview and has had less testing than the Windows build.


INSTALLING ON MACOS

Use the macOS download and follow its README-mac.txt. The included installer
handles macOS security, copies Dromaius to Applications and opens it.


MACOS KEYBOARD COMMANDS

Option+H                  Your apps
Cmd+[                     Back
Option+S                  Find an app on Google Play
Cmd+L                     In a browser such as Chrome, address bar
Cmd+T                     In a browser such as Chrome, new tab
VoiceOver+Shift+M         More actions, including long press
Cmd+R                     Refresh
Option+N                  Android notifications
Option+Page Down          Next screen of items in a long Android list
Option+Page Up            Previous screen of items in a long Android list
Cmd+?                     Keyboard help
Fn+F7                     Talk: presses an on-screen talk or record button
                          once; without one, toggles the hardware PTT key.
Fn+Shift+F7               Push to talk: holds the on-screen talk or record
                          button until pressed again.
Fn+F8                     Tap and hold (long press) the current item

In edit fields, interact with the field and type. Return in a search field
runs the search. While interacting with an Android item, Up and Down Arrow
move through items and scroll Android collections at either end.


MACOS NOTES AND TROUBLESHOOTING

- Android's location is set from the approximate location of your internet
  connection.
- Run collect-macos-logs.sh from the Dromaius folder to create diagnostics for
  a bug report.
- README-mac.txt contains detailed first-open and macOS security instructions.


REMOVING DROMAIUS FROM MACOS

Quit Dromaius and move Dromaius.app to the Bin. README-mac.txt lists the data
folders to remove if you also want to delete Android, its apps and sign-ins.


LIMITATIONS ON BOTH PLATFORMS

- Some banking and streaming apps refuse to run on emulators.
- Apps and games that do not provide accessibility information cannot be read.
