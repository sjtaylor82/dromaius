//! Finds the Android SDK, creates the virtual device, starts / resumes the
//! emulator, installs and enables the bridge, and forwards its socket.
//!
//! Everything here shells out to `adb` and `emulator`; nothing needs Java,
//! so end users never need a JDK.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};

use crate::protocol::{BRIDGE_PORT, BRIDGE_SOCKET};

pub const AVD_NAME: &str = "Dromaius";
/// Virtual device created by builds from before the rename; reused so its
/// Google account and installed apps are kept.
const LEGACY_AVD_NAME: &str = "AndroidAccess";
const LEGACY_BRIDGE_PACKAGE: &str = "com.androidaccess.bridge";
pub const EMULATOR_PORT: u16 = 5580;
pub const BRIDGE_PACKAGE: &str = "com.dromaius.bridge";
pub const BRIDGE_SERVICE: &str = "com.dromaius.bridge/.BridgeService";

#[derive(Clone)]
pub struct Device {
    pub sdk: PathBuf,
    pub serial: String,
}

pub struct StartOptions {
    pub show_emulator: bool,
    /// Erase the virtual device's apps and data ("start fresh").
    pub wipe_data: bool,
    /// How long Android may take to boot.
    pub boot_timeout: Duration,
}

impl StartOptions {
    pub fn new(show_emulator: bool) -> Self {
        Self {
            show_emulator,
            wipe_data: false,
            boot_timeout: Duration::from_secs(240),
        }
    }
}

#[cfg(windows)]
const EXE: &str = ".exe";
#[cfg(not(windows))]
const EXE: &str = "";

pub(crate) fn quiet(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// A complete Android SDK (tools, emulator and a Google Play image): a
/// developer's existing SDK if there is one, otherwise Dromaius's own copy.
/// None means first-run setup is needed.
pub fn find_sdk() -> Option<PathBuf> {
    if std::env::var_os("DROMAIUS_SDK_DIR").is_some() {
        let own = crate::setup::own_sdk_dir();
        return crate::setup::is_complete(&own).then_some(own);
    }
    let mut candidates: Vec<PathBuf> = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .iter()
        .filter_map(|v| std::env::var_os(v).map(PathBuf::from))
        .collect();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(Path::new(&local).join("Android").join("Sdk"));
    }
    if let Some(home) = home_dir() {
        candidates.push(home.join("Library/Android/sdk"));
        candidates.push(home.join("Android/Sdk"));
    }
    candidates.push(crate::setup::own_sdk_dir());
    candidates
        .into_iter()
        .find(|p| crate::setup::is_complete(p))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn avd_home() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("ANDROID_AVD_HOME") {
        return Ok(PathBuf::from(p));
    }
    if let Some(p) = std::env::var_os("ANDROID_USER_HOME") {
        return Ok(PathBuf::from(p).join("avd"));
    }
    Ok(home_dir()
        .context("no home directory")?
        .join(".android")
        .join("avd"))
}

fn adb_path(sdk: &Path) -> PathBuf {
    sdk.join("platform-tools").join(format!("adb{EXE}"))
}

pub fn adb(sdk: &Path, serial: Option<&str>, args: &[&str]) -> Result<String> {
    let mut cmd = Command::new(adb_path(sdk));
    if let Some(s) = serial {
        cmd.args(["-s", s]);
    }
    let out = quiet(&mut cmd)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("running adb {}", args.join(" ")))?;
    if !out.status.success() {
        bail!(
            "adb {} failed: {}{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout).trim(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Newest installed Google Play system image for this machine's CPU.
fn newest_play_image(sdk: &Path) -> Result<(String, String)> {
    let platform = crate::setup::installed_image(sdk)
        .ok_or_else(|| anyhow!("No Google Play system image installed"))?;
    Ok((platform, crate::setup::image_abi().to_string()))
}

/// Writes the AVD definition directly (what avdmanager would do), so no JDK is needed.
/// The virtual device's folder (its disk images and data).
pub fn avd_dir() -> Option<PathBuf> {
    Some(avd_home().ok()?.join(format!("{}.avd", avd_name())))
}

/// The Android release the virtual device runs, e.g. "android-37.0".
pub fn avd_platform() -> Option<String> {
    avd_dir().and_then(|d| platform_in(&d))
}

/// The Android release recorded in a device folder's config.ini.
fn platform_in(dir: &Path) -> Option<String> {
    let config = std::fs::read_to_string(dir.join("config.ini")).ok()?;
    let sysdir = config
        .lines()
        .find_map(|l| l.strip_prefix("image.sysdir.1="))?;
    sysdir
        .split(['/', '\\'])
        .find(|part| part.starts_with("android-"))
        .map(str::to_string)
}

/// Where the previous Android is kept after an upgrade (if the user chose to).
pub fn backup_dir() -> Option<PathBuf> {
    avd_dir().map(|d| d.with_extension("avd-backup"))
}

/// Where the current device waits while an upgrade is in progress.
fn upgrading_dir() -> Option<PathBuf> {
    avd_dir().map(|d| d.with_extension("avd-upgrading"))
}

/// The Android release of the kept backup, if there is one.
pub fn backup_platform() -> Option<String> {
    backup_dir()
        .filter(|d| d.exists())
        .and_then(|d| platform_in(&d))
}

/// Folder renames can fail briefly while Windows releases file handles.
fn rename_dir(from: &Path, to: &Path) -> Result<()> {
    let mut attempt = 0;
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if attempt < 10 => {
                attempt += 1;
                eprintln!("rename {} failed ({e}), retrying", from.display());
                std::thread::sleep(Duration::from_secs(1));
            }
            Err(e) => return Err(e).with_context(|| format!("moving {}", from.display())),
        }
    }
}

/// Moves the current device aside so an upgrade can start a fresh one. Its
/// Quick Boot snapshot is dropped (it's large and can be rebuilt).
pub fn set_aside_for_upgrade() -> Result<()> {
    let (dir, aside) = (
        avd_dir().context("no virtual device")?,
        upgrading_dir().context("no virtual device")?,
    );
    if aside.exists() {
        std::fs::remove_dir_all(&aside)?;
    }
    let _ = std::fs::remove_dir_all(dir.join("snapshots"));
    rename_dir(&dir, &aside)
}

/// After a successful upgrade: keep the previous device as the backup
/// (replacing any older backup), or delete it.
pub fn finish_upgrade(keep: bool) -> Result<()> {
    let aside = upgrading_dir().context("no virtual device")?;
    if keep {
        let backup = backup_dir().context("no virtual device")?;
        if backup.exists() {
            std::fs::remove_dir_all(&backup)?;
        }
        rename_dir(&aside, &backup)
    } else {
        std::fs::remove_dir_all(&aside).context("deleting the previous Android")
    }
}

/// After a failed upgrade: discard the new device and bring back the previous one.
pub fn undo_upgrade() -> Result<()> {
    let (dir, aside) = (
        avd_dir().context("no virtual device")?,
        upgrading_dir().context("no virtual device")?,
    );
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    rename_dir(&aside, &dir)
}

/// Swaps the current device and the backup. The emulator must be stopped.
pub fn swap_with_backup() -> Result<()> {
    let (dir, backup, tmp) = (
        avd_dir().context("no virtual device")?,
        backup_dir().context("no virtual device")?,
        upgrading_dir().context("no virtual device")?,
    );
    if !backup.exists() {
        bail!("there is no backup to switch to");
    }
    let _ = std::fs::remove_dir_all(dir.join("snapshots"));
    rename_dir(&dir, &tmp)?;
    rename_dir(&backup, &dir)?;
    rename_dir(&tmp, &backup)
}

pub fn delete_backup() -> Result<()> {
    match backup_dir() {
        Some(b) if b.exists() => std::fs::remove_dir_all(&b).context("deleting the backup"),
        _ => Ok(()),
    }
}

/// Disk space used by a folder tree.
pub fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_size(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// The virtual device to use: an existing legacy one, otherwise ours.
fn avd_name() -> &'static str {
    let legacy = avd_home().map(|h| h.join(format!("{LEGACY_AVD_NAME}.ini")).exists());
    if legacy.unwrap_or(false) {
        LEGACY_AVD_NAME
    } else {
        AVD_NAME
    }
}

pub fn ensure_avd(sdk: &Path) -> Result<()> {
    let name = avd_name();
    let home = avd_home()?;
    let ini = home.join(format!("{name}.ini"));
    let dir = home.join(format!("{name}.avd"));
    if ini.exists() && dir.join("config.ini").exists() {
        // Devices created before push-to-talk had the microphone off.
        let config_path = dir.join("config.ini");
        if let Ok(config) = std::fs::read_to_string(&config_path)
            && config.contains("hw.audioInput=no")
        {
            let _ = std::fs::write(
                &config_path,
                config.replace("hw.audioInput=no", "hw.audioInput=yes"),
            );
        }
        return Ok(());
    }
    let (platform, abi) = newest_play_image(sdk)?;
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        &ini,
        format!(
            "avd.ini.encoding=UTF-8\npath={}\npath.rel=avd/{name}.avd\ntarget={platform}\n",
            dir.display()
        ),
    )?;
    let sep = std::path::MAIN_SEPARATOR;
    let config = format!(
        "AvdId={name}
avd.ini.displayname=Dromaius
abi.type={abi}
hw.cpu.arch={arch}
hw.cpu.ncore=4
image.sysdir.1=system-images{sep}{platform}{sep}google_apis_playstore{sep}{abi}{sep}
tag.id=google_apis_playstore
tag.display=Google Play
PlayStore.enabled=true
hw.ramSize=2048
vm.heapSize=256
hw.keyboard=yes
hw.lcd.width=1080
hw.lcd.height=2400
hw.lcd.density=420
hw.gpu.enabled=yes
hw.gpu.mode=auto
hw.audioInput=yes
hw.camera.back=none
hw.camera.front=none
disk.dataPartition.size=6G
fastboot.forceColdBoot=no
fastboot.forceFastBoot=yes
showDeviceFrame=no
",
        arch = if abi == "x86_64" { "x86_64" } else { "arm64" },
    );
    std::fs::write(dir.join("config.ini"), config)?;
    Ok(())
}

fn device_state(sdk: &Path, serial: &str) -> Option<String> {
    adb(sdk, Some(serial), &["get-state"]).ok()
}

/// Starts the emulator (Quick Boot) or resumes it if we paused it earlier.
pub fn start(sdk: PathBuf, opts: &StartOptions, status: &dyn Fn(&str)) -> Result<Device> {
    let serial = format!("emulator-{EMULATOR_PORT}");
    adb(&sdk, None, &["start-server"])?;

    let mut emulator = None;
    let timeout = opts.boot_timeout;
    if device_state(&sdk, &serial).as_deref() == Some("device") {
        status("Resuming Android");
        crate::timing::mark("emulator was paused or running: resuming");
        // Harmless if it is already running.
        let _ = adb(&sdk, Some(&serial), &["emu", "avd", "start"]);
    } else {
        ensure_avd(&sdk)?;
        status("Starting Android");
        crate::timing::mark(
            "emulator not running: starting it (Quick Boot snapshot, or cold boot)",
        );
        let mut cmd = Command::new(sdk.join("emulator").join(format!("emulator{EXE}")));
        cmd.args([
            "-avd",
            avd_name(),
            "-port",
            &EMULATOR_PORT.to_string(),
            "-no-boot-anim",
            "-no-metrics",
        ]);
        if !opts.show_emulator {
            cmd.arg("-no-window");
        }
        if opts.wipe_data {
            cmd.arg("-wipe-data");
        }
        // Without this the emulator sends silence instead of the microphone
        // (push-to-talk, voice messages). Android apps still need their own
        // microphone permission, and the PC asks once.
        cmd.arg("-allow-host-audio");
        // Keep the emulator's own messages: they explain boot failures.
        let log = std::fs::File::create(emulator_log_path()).context("creating emulator.log")?;
        let child = quiet(&mut cmd)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()
            .context("starting the emulator")?;
        emulator = Some(child);
    }

    let device = Device { sdk, serial };
    wait_for_boot(&device, timeout, status, emulator.as_mut())?;
    crate::timing::mark("Android boot completed");
    Ok(device)
}

fn wait_for_boot(
    d: &Device,
    timeout: Duration,
    status: &dyn Fn(&str),
    mut emulator: Option<&mut std::process::Child>,
) -> Result<()> {
    let started = Instant::now();
    let mut announced = false;
    loop {
        if adb(
            &d.sdk,
            Some(&d.serial),
            &["shell", "getprop", "sys.boot_completed"],
        )
        .is_ok_and(|s| s == "1")
        {
            return Ok(());
        }
        if started.elapsed() > Duration::from_secs(10) && !announced {
            status("Android is still starting, please wait");
            announced = true;
        }
        // Fail at once, with its explanation, if the emulator gave up.
        if let Some(child) = emulator.as_deref_mut()
            && let Ok(Some(exit)) = child.try_wait()
        {
            bail!(
                "the Android emulator stopped while starting ({exit}).{}",
                emulator_log_tail()
            );
        }
        if started.elapsed() > timeout {
            bail!(
                "Android did not finish starting within {} minutes.{}",
                timeout.as_secs() / 60,
                emulator_log_tail()
            );
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// The signed bridge, built into the executable (see build.rs).
#[cfg(embedded_bridge)]
const EMBEDDED_BRIDGE: Option<&[u8]> =
    Some(include_bytes!(concat!(env!("OUT_DIR"), "/bridge.apk")));
#[cfg(not(embedded_bridge))]
const EMBEDDED_BRIDGE: Option<&[u8]> = None;

/// The emulator's output from the last time Dromaius started it.
pub fn emulator_log_path() -> PathBuf {
    crate::timing::log_path().with_file_name("emulator.log")
}

/// The last lines of emulator.log, for error messages.
fn emulator_log_tail() -> String {
    let text = std::fs::read_to_string(emulator_log_path()).unwrap_or_default();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return String::new();
    }
    let tail = lines[lines.len().saturating_sub(6)..].join("\n");
    format!(
        " The emulator said:\n{tail}\n(Full log: {})",
        emulator_log_path().display()
    )
}

/// Finds the bridge APK: a path from DROMAIUS_BRIDGE_APK, the copy built into
/// the executable, or (for development builds without one) a file next to the
/// executable or in the source tree.
pub fn find_bridge_apk() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("DROMAIUS_BRIDGE_APK") {
        return Some(PathBuf::from(p));
    }
    if let Some(bytes) = EMBEDDED_BRIDGE {
        // adb installs from a file: unpack it, but only when it changed.
        let sdk = crate::setup::own_sdk_dir();
        let path = sdk.parent().unwrap_or(&sdk).join("dromaius-bridge.apk");
        if std::fs::read(&path).ok().as_deref() != Some(bytes) {
            let _ = std::fs::create_dir_all(path.parent()?);
            std::fs::write(&path, bytes).ok()?;
        }
        return Some(path);
    }
    let exe = std::env::current_exe().ok()?;
    for dir in exe.ancestors().skip(1) {
        for candidate in [
            dir.join("dromaius-bridge.apk"),
            // Inside a macOS app bundle: Dromaius.app/Contents/Resources.
            dir.join("Resources").join("dromaius-bridge.apk"),
            dir.join("android-bridge/app/build/outputs/apk/release/app-release.apk"),
            dir.join("android-bridge/app/build/outputs/apk/debug/app-debug.apk"),
        ] {
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

fn marker_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".config")))?;
    Some(base.join("Dromaius").join("bridge-installed.txt"))
}

/// Installs the bridge when it's missing or the APK changed, then enables it.
/// Android sometimes restarts once more right after booting (e.g. after an
/// upgrade), so connection errors wait for it to come back and retry.
pub fn ensure_bridge(d: &Device, status: &dyn Fn(&str)) -> Result<()> {
    let mut attempt = 0;
    loop {
        match ensure_bridge_once(d, status) {
            Ok(()) => return Ok(()),
            Err(e) if attempt < 3 && is_connection_error(&e) => {
                attempt += 1;
                status("Android is restarting, waiting for it");
                std::thread::sleep(Duration::from_secs(3));
                wait_for_boot(d, Duration::from_secs(300), status, None)?;
            }
            Err(e) => return Err(e),
        }
    }
}

/// adb lost the emulator (it is rebooting or briefly unreachable).
fn is_connection_error(e: &anyhow::Error) -> bool {
    let text = format!("{e:#}");
    [
        "not found",
        "offline",
        "no devices",
        "device still",
        "closed",
    ]
    .iter()
    .any(|k| text.contains(k))
}

fn ensure_bridge_once(d: &Device, status: &dyn Fn(&str)) -> Result<()> {
    let apk = find_bridge_apk().ok_or_else(|| anyhow!("Bridge APK not found"))?;
    // Identify the bridge by its contents, so unpacking it again (or a new
    // file date) doesn't cause needless reinstalls.
    let digest = sha1_smol::Sha1::from(std::fs::read(&apk)?)
        .digest()
        .to_string();
    let stamp = format!("{}:{digest}", d.serial);
    // Remove the bridge from before the rename so two copies don't run.
    if adb(
        &d.sdk,
        Some(&d.serial),
        &["shell", "pm", "path", LEGACY_BRIDGE_PACKAGE],
    )
    .is_ok_and(|s| !s.is_empty())
    {
        let _ = adb(
            &d.sdk,
            Some(&d.serial),
            &["uninstall", LEGACY_BRIDGE_PACKAGE],
        );
    }
    let installed = adb(
        &d.sdk,
        Some(&d.serial),
        &["shell", "pm", "path", BRIDGE_PACKAGE],
    )
    .is_ok_and(|s| !s.is_empty());
    let marker = marker_path();
    let up_to_date = installed
        && marker
            .as_ref()
            .and_then(|m| std::fs::read_to_string(m).ok())
            .as_deref()
            == Some(&stamp);
    if !up_to_date {
        status("Installing the accessibility bridge");
        let apk = apk.to_string_lossy();
        let install = || adb(&d.sdk, Some(&d.serial), &["install", "-r", "-g", &apk]);
        if let Err(e) = install() {
            // A bridge signed with a different key (e.g. an old debug build)
            // can't be updated in place. It keeps no data, so replace it.
            if !format!("{e:#}").contains("INSTALL_FAILED_UPDATE_INCOMPATIBLE") {
                return Err(e);
            }
            adb(&d.sdk, Some(&d.serial), &["uninstall", BRIDGE_PACKAGE])?;
            install()?;
        }
        if let Some(m) = marker {
            let _ = std::fs::create_dir_all(m.parent().unwrap());
            let _ = std::fs::write(m, &stamp);
        }
    }

    // On a brand-new Android, first-boot setup can reset accessibility
    // settings right after we set them, so check the bridge actually runs:
    // poll often, re-enable it at most every few seconds, stop once it runs.
    let started = Instant::now();
    let mut last_enable: Option<Instant> = None;
    let mut announced = false;
    while !bridge_bound(d) {
        if last_enable.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            enable_bridge_service(d)?;
            last_enable = Some(Instant::now());
        }
        if started.elapsed() > Duration::from_secs(30) {
            crate::timing::mark("bridge still not running after 30 s; continuing anyway");
            break;
        }
        if !announced && started.elapsed() > Duration::from_secs(6) {
            status("Waiting for Android to finish setting up");
            announced = true;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    crate::timing::mark(&format!(
        "bridge running (checked for {:.1} s)",
        started.elapsed().as_secs_f64()
    ));
    adb(
        &d.sdk,
        Some(&d.serial),
        &[
            "forward",
            &format!("tcp:{BRIDGE_PORT}"),
            &format!("localabstract:{BRIDGE_SOCKET}"),
        ],
    )?;
    Ok(())
}

/// Adds the bridge to Android's enabled accessibility services.
fn enable_bridge_service(d: &Device) -> Result<()> {
    let current = adb(
        &d.sdk,
        Some(&d.serial),
        &[
            "shell",
            "settings",
            "get",
            "secure",
            "enabled_accessibility_services",
        ],
    )?;
    let others: Vec<&str> = current
        .split(':')
        .filter(|s| {
            !s.is_empty()
                && *s != "null"
                && *s != BRIDGE_SERVICE
                && !s.starts_with(LEGACY_BRIDGE_PACKAGE)
        })
        .collect();
    if !current.split(':').any(|s| s == BRIDGE_SERVICE) || current.contains(LEGACY_BRIDGE_PACKAGE) {
        let value = others
            .iter()
            .copied()
            .chain([BRIDGE_SERVICE])
            .collect::<Vec<_>>()
            .join(":");
        adb(
            &d.sdk,
            Some(&d.serial),
            &[
                "shell",
                "settings",
                "put",
                "secure",
                "enabled_accessibility_services",
                &value,
            ],
        )?;
    }
    adb(
        &d.sdk,
        Some(&d.serial),
        &[
            "shell",
            "settings",
            "put",
            "secure",
            "accessibility_enabled",
            "1",
        ],
    )?;
    Ok(())
}

/// Whether `dumpsys accessibility` output lists the bridge as bound. The
/// list may wrap onto following lines, depending on the Android version, so
/// the whole section is searched.
fn bridge_in_bound_services(dumpsys: &str) -> bool {
    dumpsys.split("Bound services").skip(1).any(|rest| {
        let section = rest.split("Enabled services").next().unwrap_or(rest);
        section.contains(BRIDGE_PACKAGE) || section.contains("Dromaius Bridge")
    })
}

/// Whether Android has started the bridge service.
fn bridge_bound(d: &Device) -> bool {
    adb(
        &d.sdk,
        Some(&d.serial),
        &["shell", "dumpsys", "accessibility"],
    )
    .is_ok_and(|out| bridge_in_bound_services(&out))
}

impl Device {
    pub fn display_mode(&self) -> Result<&'static str> {
        let size = adb(&self.sdk, Some(&self.serial), &["shell", "wm", "size"])?;
        let density = adb(&self.sdk, Some(&self.serial), &["shell", "wm", "density"])?;
        let narrow_tablet = size.contains("Override size: 1080x2880");
        if narrow_tablet {
            // Migrate the original tall, narrow tablet profile. Although its
            // diagonal was tablet-sized, it exposed only ~617 dp of width and
            // therefore still showed very few cards in horizontal carousels.
            adb(
                &self.sdk,
                Some(&self.serial),
                &["shell", "wm", "size", "1600x2560"],
            )?;
        }
        Ok(
            if narrow_tablet
                || size.contains("Override size: 1600x2560")
                || density.contains("Override density: 280")
            {
                "tablet"
            } else {
                "phone"
            },
        )
    }

    pub fn set_display_mode(&self, mode: &str) -> Result<&'static str> {
        match mode {
            "phone" => {
                adb(
                    &self.sdk,
                    Some(&self.serial),
                    &["shell", "wm", "size", "reset"],
                )?;
                adb(
                    &self.sdk,
                    Some(&self.serial),
                    &["shell", "wm", "density", "reset"],
                )?;
                Ok("phone")
            }
            "tablet" => {
                adb(
                    &self.sdk,
                    Some(&self.serial),
                    &["shell", "wm", "size", "1600x2560"],
                )?;
                adb(
                    &self.sdk,
                    Some(&self.serial),
                    &["shell", "wm", "density", "280"],
                )?;
                Ok("tablet")
            }
            _ => bail!("unknown display mode {mode}"),
        }
    }

    /// Pauses the virtual device so the next launch is instant.
    pub fn pause(&self) -> Result<()> {
        adb(&self.sdk, Some(&self.serial), &["emu", "avd", "stop"]).map(|_| ())
    }

    /// Shuts down, saving a Quick Boot snapshot.
    pub fn shutdown(&self) -> Result<()> {
        adb(&self.sdk, Some(&self.serial), &["emu", "kill"]).map(|_| ())
    }

    /// Shuts the emulator down (resuming it first if paused) and waits until
    /// it has exited, so its files can be replaced.
    pub fn stop_and_wait(&self) -> Result<()> {
        let _ = adb(&self.sdk, Some(&self.serial), &["emu", "avd", "start"]);
        let _ = self.shutdown();
        let started = Instant::now();
        while device_state(&self.sdk, &self.serial).is_some() {
            if started.elapsed() > Duration::from_secs(90) {
                bail!("Android did not shut down");
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        // The emulator process lingers briefly after adb loses it.
        std::thread::sleep(Duration::from_secs(3));
        Ok(())
    }

    /// Presses or releases the key used as a hardware push-to-talk button
    /// (F12; apps such as Zello let you assign it).
    pub fn ptt_key(&self, down: bool) -> Result<()> {
        let event = format!("EV_KEY:KEY_F12:{}", u8::from(down));
        adb(
            &self.sdk,
            Some(&self.serial),
            &["emu", "event", "send", &event],
        )
        .map(|_| ())
    }

    /// Stops an app (it restarts the next time it's used).
    pub fn force_stop(&self, package: &str) -> Result<()> {
        adb(
            &self.sdk,
            Some(&self.serial),
            &["shell", "am", "force-stop", package],
        )
        .map(|_| ())
    }

    /// Starts an app's launcher activity through adb.
    pub fn launch(&self, package: &str) -> Result<()> {
        adb(
            &self.sdk,
            Some(&self.serial),
            &[
                "shell",
                "monkey",
                "-p",
                package,
                "-c",
                "android.intent.category.LAUNCHER",
                "1",
            ],
        )
        .map(|_| ())
    }

    /// Sets the emulator's GPS position.
    pub fn set_location(&self, loc: &Location) -> Result<()> {
        // The emulator console takes longitude first.
        adb(
            &self.sdk,
            Some(&self.serial),
            &[
                "emu",
                "geo",
                "fix",
                &loc.lon.to_string(),
                &loc.lat.to_string(),
            ],
        )
        .map(|_| ())
    }

    pub fn open_play_page(&self, package: &str) -> Result<()> {
        self.open_play(&format!("market://details?id={package}"))
    }

    /// Opens Google Play's search results for apps matching `query`.
    pub fn open_play_search(&self, query: &str) -> Result<()> {
        self.open_play(&format!(
            "market://search?q={}&c=apps",
            percent_encode(query)
        ))
    }

    fn open_play(&self, uri: &str) -> Result<()> {
        adb(
            &self.sdk,
            Some(&self.serial),
            &[
                "shell",
                "am",
                "start",
                "-a",
                "android.intent.action.VIEW",
                // Quoted: the device shell would otherwise treat '&' as a separator.
                &format!("-d '{uri}'"),
                "-p",
                "com.android.vending",
            ],
        )
        .map(|_| ())
    }
}

#[derive(Debug, Clone)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
    /// Human description, e.g. "Brisbane, Queensland".
    pub place: String,
}

/// Where the PC is: Windows location services when available (Wi-Fi based,
/// often street-level), otherwise the internet connection's city.
pub fn pc_location() -> Result<Location> {
    #[cfg(windows)]
    match windows_location() {
        Ok(loc) => return Ok(loc),
        Err(e) => eprintln!("Windows location unavailable, using IP address: {e:#}"),
    }
    #[cfg(target_os = "macos")]
    match mac_location() {
        Ok(loc) => return Ok(loc),
        Err(e) => eprintln!("macOS location unavailable, using IP address: {e:#}"),
    }
    ip_location()
}

/// The Mac's position from Location Services (Wi-Fi based, usually street
/// level). macOS asks the user for permission the first time.
#[cfg(target_os = "macos")]
fn mac_location() -> Result<Location> {
    use std::cell::RefCell;
    use std::sync::mpsc;

    use dispatch2::DispatchQueue;
    use objc2::rc::Retained;
    use objc2_core_location::{
        CLAuthorizationStatus, CLLocationManager, kCLLocationAccuracyHundredMeters,
    };

    thread_local! {
        // CoreLocation delivers updates on the run loop of the thread that
        // created the manager, so it lives on the main thread.
        static MANAGER: RefCell<Option<Retained<CLLocationManager>>> = const { RefCell::new(None) };
    }

    enum Poll {
        Waiting,
        Denied,
        Fix(f64, f64, f64),
    }

    DispatchQueue::main().exec_sync(|| {
        MANAGER.with(|m| {
            // SAFETY: runs on the main thread, as CoreLocation requires.
            unsafe {
                let manager = CLLocationManager::new();
                manager.setDesiredAccuracy(kCLLocationAccuracyHundredMeters);
                manager.requestWhenInUseAuthorization();
                manager.startUpdatingLocation();
                *m.borrow_mut() = Some(manager);
            }
        })
    });

    let poll = || {
        let (tx, rx) = mpsc::channel();
        DispatchQueue::main().exec_sync(move || {
            let result = MANAGER.with(|m| {
                let m = m.borrow();
                let Some(manager) = m.as_ref() else {
                    return Poll::Waiting;
                };
                // SAFETY: main thread; the manager is alive.
                unsafe {
                    let status = manager.authorizationStatus();
                    if status == CLAuthorizationStatus::Denied
                        || status == CLAuthorizationStatus::Restricted
                    {
                        return Poll::Denied;
                    }
                    match manager.location() {
                        Some(loc) => {
                            let c = loc.coordinate();
                            Poll::Fix(c.latitude, c.longitude, loc.horizontalAccuracy())
                        }
                        None => Poll::Waiting,
                    }
                }
            });
            let _ = tx.send(result);
        });
        rx.recv().unwrap_or(Poll::Waiting)
    };

    let permission_name = || {
        let (tx, rx) = mpsc::channel();
        DispatchQueue::main().exec_sync(move || {
            let name = MANAGER.with(|m| {
                let m = m.borrow();
                // SAFETY: main thread; the manager is alive.
                let status = m
                    .as_ref()
                    .map(|manager| unsafe { manager.authorizationStatus() });
                match status {
                    Some(CLAuthorizationStatus::NotDetermined) => "not decided (no prompt shown?)",
                    Some(CLAuthorizationStatus::AuthorizedAlways) => "allowed",
                    Some(CLAuthorizationStatus::AuthorizedWhenInUse) => "allowed while in use",
                    Some(CLAuthorizationStatus::Denied) => "denied",
                    Some(CLAuthorizationStatus::Restricted) => "restricted",
                    _ => "unknown",
                }
            });
            let _ = tx.send(name);
        });
        rx.recv().unwrap_or("unknown")
    };

    let stop = || {
        DispatchQueue::main().exec_sync(|| {
            MANAGER.with(|m| {
                if let Some(manager) = m.borrow_mut().take() {
                    // SAFETY: main thread.
                    unsafe { manager.stopUpdatingLocation() };
                }
            })
        })
    };

    // Allow time for the user to answer macOS's permission prompt.
    let started = Instant::now();
    let result = loop {
        match poll() {
            Poll::Fix(lat, lon, accuracy) if accuracy >= 0.0 => {
                break Ok(Location {
                    lat,
                    lon,
                    place: format!(
                        "your Mac's location, accurate to about {} metres",
                        accuracy.round()
                    ),
                });
            }
            Poll::Denied => {
                break Err(anyhow!(
                    "Location Services are off or not allowed for Dromaius"
                ));
            }
            _ if started.elapsed() > Duration::from_secs(30) => {
                break Err(anyhow!(
                    "no position within 30 seconds (permission: {})",
                    permission_name()
                ));
            }
            _ => std::thread::sleep(Duration::from_millis(500)),
        }
    };
    stop();
    result
}

#[cfg(windows)]
fn windows_location() -> Result<Location> {
    use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator, PositionAccuracy};
    use windows::Foundation::TimeSpan;

    // Unpackaged desktop apps are governed by "Let desktop apps access your
    // location" in Windows Settings > Privacy & security > Location.
    let access = Geolocator::RequestAccessAsync()?.join()?;
    if access != GeolocationAccessStatus::Allowed {
        bail!("location access is turned off in Windows settings");
    }
    let locator = Geolocator::new()?;
    locator.SetDesiredAccuracy(PositionAccuracy::High)?;
    const SECOND: i64 = 10_000_000; // TimeSpan is in 100 ns units
    let position = locator
        .GetGeopositionAsyncWithAgeAndTimeout(
            TimeSpan {
                Duration: 10 * 60 * SECOND,
            }, // a fix up to 10 minutes old is fine
            TimeSpan {
                Duration: 15 * SECOND,
            },
        )?
        .join()?;
    let coordinate = position.Coordinate()?;
    let point = coordinate.Point()?.Position()?;
    let accuracy = coordinate.Accuracy()?;
    Ok(Location {
        lat: point.Latitude,
        lon: point.Longitude,
        place: format!(
            "your Windows location, accurate to about {} metres",
            accuracy.round()
        ),
    })
}

/// Approximate location of this internet connection (city level), from ip-api.com.
pub fn ip_location() -> Result<Location> {
    // Never expose or accept location data over plaintext HTTP. ipwho.is
    // supports HTTPS without an API key and derives location from the request.
    let out = quiet(&mut Command::new(if cfg!(windows) {
        "curl.exe"
    } else {
        "curl"
    }))
    .args([
        "-sSfL",
        "--retry",
        "2",
        "--max-time",
        "10",
        "https://ipwho.is/",
    ])
    .stdin(Stdio::null())
    .output()
    .context("running secure location lookup")?;
    if !out.status.success() {
        bail!(
            "secure location lookup failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).context("bad response from secure location lookup")?;
    if v["success"] != true {
        bail!("location lookup failed: {}", v["message"]);
    }
    let (Some(lat), Some(lon)) = (v["latitude"].as_f64(), v["longitude"].as_f64()) else {
        bail!("location lookup returned no coordinates");
    };
    let place = [v["city"].as_str(), v["region"].as_str()]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    Ok(Location {
        lat,
        lon,
        place: format!("{place}, from your internet address"),
    })
}

/// Percent-encodes everything except ASCII letters and digits, which also
/// keeps the text safe to pass through the device shell.
fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// Extracts a package name from a Play Store link, market link or bare package name.
pub fn package_from_link(link: &str) -> Option<String> {
    let link = link.trim();
    let id = if let Some(pos) = link.find("id=") {
        let rest = &link[pos + 3..];
        rest.split(['&', '#', ' ']).next().unwrap_or("")
    } else {
        link
    };
    let valid = id.contains('.')
        && id.split('.').all(|part| {
            part.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
    valid.then(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::package_from_link;

    #[test]
    fn detects_bound_bridge() {
        use super::bridge_in_bound_services as bound;
        let one_line =
            "  Bound services:{Service[label=Dromaius Bridge, feedbackType[FEEDBACK_SPOKEN]]}
  Enabled services:{{com.dromaius.bridge/com.dromaius.bridge.BridgeService}}";
        let wrapped = "  Bound services:{
    Service[label=Dromaius Bridge,
      feedbackType[FEEDBACK_SPOKEN]]
  }
  Enabled services:{}";
        let only_enabled = "  Bound services:{}
  Enabled services:{{com.dromaius.bridge/com.dromaius.bridge.BridgeService}}";
        assert!(bound(one_line));
        assert!(bound(wrapped));
        assert!(!bound(only_enabled), "enabled but not yet bound");
    }

    #[test]
    fn parses_links() {
        assert_eq!(
            package_from_link(
                "https://play.google.com/store/apps/details?id=com.whatsapp&hl=en_GB"
            )
            .as_deref(),
            Some("com.whatsapp")
        );
        assert_eq!(
            package_from_link("market://details?id=org.mozilla.firefox").as_deref(),
            Some("org.mozilla.firefox")
        );
        assert_eq!(
            package_from_link(" com.spotify.music ").as_deref(),
            Some("com.spotify.music")
        );
        assert_eq!(package_from_link("https://example.com"), None);
        assert_eq!(package_from_link("hello"), None);
        assert_eq!(super::percent_encode("face book&x"), "face%20book%26x");
    }

    #[test]
    #[ignore = "needs internet"]
    fn looks_up_pc_location() {
        let loc = super::pc_location().unwrap();
        println!("{loc:?}");
        assert!(loc.lat.abs() <= 90.0 && !loc.place.is_empty());
    }
}
