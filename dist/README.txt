DROMAIUS 0.1.0

Use Android apps on Windows with your screen reader and keyboard.

Dromaius is an independent accessibility project and is not affiliated with or
endorsed by Google. Android and Google Play are trademarks of Google LLC.

Dromaius runs Google's official Android Emulator in the background and turns
Android screens into accessible web pages. NVDA, JAWS and Narrator can use
browse mode, quick-navigation keys, the elements list and Say All. You can
also type into Android apps with your PC keyboard.

Dromaius is an early hobby release. Please report problems at:
https://github.com/sjtaylor82/dromaius/issues


WHAT YOU NEED

- 64-bit Windows 10 or 11
- 8 GB of RAM; 16 GB recommended
- About 12 GB of free disk space
- An internet connection for the first setup
- Hardware virtualization and Windows Hypervisor Platform
- A Google account only if you want to use Google Play

You do not need Java, Android Studio or the Android SDK. Dromaius downloads
and manages Android for you. The first setup downloads about 2.8 GB.


1. Extract the Zip file.
2. Open the extracted folder and run dromaius.exe.
3. Accept Google's Android licence when prompted and wait for the download.


Android's first start may take about a minute. Later starts should take only a
few seconds because closing Dromaius pauses Android instead of shutting it down.


KEYBOARD COMMANDS

Alt+H                     Your apps
Alt+Left                  Back
Ctrl+L                    Find an app on Google Play
Shift+F10 or Applications More actions, including long press
F5                        Refresh
Alt+N                     Android notifications
Alt+Page Down             Next screen of items in a long Android list
Alt+Page Up               Previous screen of items in a long Android list
F1                        Keyboard help
[                         Push to talk: press to start, press again to stop
]                         Tap and hold (long press) the current item

Long lists show only what currently fits on Android's screen. Press Alt+Page
Down or Alt+Page Up from anywhere on the screen to move through them. You can
also turn off browse mode while focused on an Android item. Up and Down Arrow
then move through the items and scroll Android at either end.


GOOD TO KNOW

- Some banking and streaming apps refuse to run on emulators.
- Apps and games that do not provide accessibility information cannot be read.
- Dromaius sets Android's location from Windows location services when
  available, or from the approximate location of your internet connection.

If virtualization is unavailable, press Windows+R, type optionalfeatures,
press Enter, turn on Windows Hypervisor Platform, and restart your PC.

If startup is unusually slow, include this file with your bug report:
%LOCALAPPDATA%\Dromaius\startup.log


REMOVING DROMAIUS

Delete the extracted Dromaius folder. To also remove Android, its installed
apps and your Google sign-in, delete:

%LOCALAPPDATA%\Dromaius
%LOCALAPPDATA%\com.dromaius.desktop
%USERPROFILE%\.android\avd\Dromaius.avd
%USERPROFILE%\.android\avd\Dromaius.ini
