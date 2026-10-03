DROMAIUS FOR MAC (preview)
Use Android apps on your Mac with VoiceOver and the keyboard.

Dromaius runs a real Android phone, Google's official Android emulator, out of
sight in the background, and shows each Android screen as an accessible web
page that VoiceOver can read with its usual web navigation (rotor, headings,
links, form controls, and VO+A to read all). Typing goes into normal edit
fields.

The Mac version is new and has had little testing. Please report problems at
https://github.com/sjtaylor82/dromaius/issues and include
~/Library/Application Support/Dromaius/startup.log if starting is slow.


WHAT YOU NEED

- macOS 11 (Big Sur) or later, on Apple Silicon (M1 or later) or Intel.
- 8 GB of RAM (16 GB is better).
- About 12 GB of free disk space. Dromaius downloads about 2.8 GB of Android
  from Google; once installed, Android uses about 4 GB, plus about 4 GB for
  the "quick-start" snapshot that lets it start in seconds, plus your apps.
- An internet connection for the first start, and a Google account.


INSTALLING

1. Unzip the download and move Dromaius.app to your Applications folder.
2. The first time, macOS will refuse to open it, because Dromaius isn't
   registered with Apple. To open it anyway:
   - In Finder, select Dromaius.app, press VO+Shift+M (or Control-click) to
     open its menu, choose Open, then Open again in the warning.
   - Or: System Settings, Privacy & Security, scroll to "Dromaius was blocked",
     and choose Open Anyway.
   You only need to do this once.
3. Dromaius offers to download Android from Google. It tells you which
   Android version and how much it downloads. Read Google's licence, then
   choose "Accept and download". Progress is announced every 10 percent.
4. Android starts for the first time (about a minute), and you arrive at
   "Your apps".
5. Sign in to Google once: open Play Store from Your apps and choose Sign in.

Later starts take a few seconds: closing Dromaius pauses Android rather than
shutting it down.


KEYS

Cmd+Shift+H         Your apps
Cmd+[               Back
Cmd+L               Find an app on Google Play (type its name or paste a link)
VO+Shift+M          More actions for the current item, such as long press
Cmd+R               Refresh the screen
Cmd+Shift+N         Android notifications (new ones are also announced)
Cmd+?               Keyboard help

In edit fields, interact with the field and type. Return in a search field
runs the search.


GOOD TO KNOW

- Android's location is set from your internet connection's town.
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
