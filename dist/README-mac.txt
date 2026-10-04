DROMAIUS FOR MAC (preview)
Use Android apps on your Mac with VoiceOver and the keyboard.

Dromaius is an independent accessibility project and is not affiliated with or
endorsed by Google. Android and Google Play are trademarks of Google LLC.

Dromaius runs a real Android phone, Google's official Android emulator, out of
sight in the background, and shows each Android screen as an accessible web
page that VoiceOver can read with its usual web navigation (rotor, headings,
links, form controls, and VO+A to read all). Typing goes into normal edit
fields.

The Mac version is new and has had little testing. Please report problems at
https://github.com/sjtaylor82/dromaius/issues. To include diagnostics, run
this in Terminal from the Dromaius folder and attach the file it creates:
  bash collect-macos-logs.sh


WHAT YOU NEED

- macOS 12 (Monterey) or later, on Apple Silicon (M1 or later) or Intel.
- 8 GB of RAM (16 GB is better).
- About 12 GB of free disk space. Dromaius downloads about 2.8 GB of Android
  from Google; once installed, Android uses about 4 GB, plus about 4 GB for
  the "quick-start" snapshot that lets it start in seconds, plus your apps.
- An internet connection for the first start. A Google account is needed only
  if you want to use Google Play.


INSTALLING

1. Unzip the download. You get a Dromaius folder containing Dromaius.app,
   install-macos.sh, collect-macos-logs.sh and this README.
2. Install with the included script (easiest). In Finder, select
   install-macos.sh and copy it with Cmd+C. Open Terminal, paste with Cmd+V,
   then press Return.
   The script removes the "downloaded from the internet" flag that would
   otherwise stop macOS opening Dromaius, copies Dromaius.app to your
   Applications folder (replacing an older copy) and opens it.

   Without the script: move Dromaius.app to Applications yourself. The
   first time, macOS refuses to open it because Dromaius isn't registered
   with Apple. In Finder, select Dromaius.app, press VO+Shift+M (or
   Control-click), choose Open, then Open again in the warning. Or use
   System Settings, Privacy & Security, "Dromaius was blocked", Open Anyway.
3. Dromaius offers to download Android from Google. It tells you which
   Android version and how much it downloads. Read Google's licence, then
   choose "Accept and download". Progress is announced every 10 percent.
4. Android starts for the first time (about a minute), and you arrive at
   "Your apps".
5. Sign in to Google once: open Play Store from Your apps and choose Sign in.

Additional runs take a few seconds: closing Dromaius pauses Android rather than
shutting it down.


KEYS

Option+H            Your apps
Cmd+[               Back
Option+S            Find an app on Google Play (type its name or paste a link)
Cmd+L               In a browser such as Chrome, go to the address bar
Cmd+T               In a browser such as Chrome, open a new tab
VO+Shift+M          More actions for the current item, such as long press
Cmd+R               Refresh the screen
Option+N            Android notifications (new ones are also announced)
Option+Page Down    Next screen of items in a long Android list
Option+Page Up      Previous screen of items in a long Android list
Cmd+?               Keyboard help
Fn+F7               Talk: presses an on-screen talk or record button once;
                    without one, toggles the hardware PTT key.
Fn+Shift+F7         Push to talk: holds the on-screen talk or record button
                    until pressed again.
Fn+F8               Tap and hold (long press) the current item

In edit fields, interact with the field and type. Return in a search field
runs the search.

Long lists show only what currently fits on Android's screen. Press
Option+Page Down or Option+Page Up from anywhere on the screen to move through
them. While interacting with an Android item, Up and Down Arrow move through
the items and scroll Android at either end.


GOOD TO KNOW

- Android's location is set from your internet connection's location.
- Some apps refuse to run on an emulator, such as some banking and streaming
  apps. Games and apps that draw their own screens can't be read.


REMOVING DROMAIUS

1. Quit Dromaius and move Dromaius.app to the Bin.
2. To remove Android and your Android data, including your Google sign-in,
   delete these folders (in Finder, Go menu, Go to Folder, Cmd+Shift+G):
     ~/Library/Application Support/Dromaius
     ~/Library/Application Support/com.dromaius.desktop
     ~/Library/WebKit/com.dromaius.desktop
     ~/.android/avd/Dromaius.avd  (and Dromaius.ini next to it)
