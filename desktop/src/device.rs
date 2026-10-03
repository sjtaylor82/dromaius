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
    if avd_name() == LEGACY_AVD_NAME {
        return Ok(());
    }
    let home = avd_home()?;
    let ini = home.join(format!("{AVD_NAME}.ini"));
    let dir = home.join(format!("{AVD_NAME}.avd"));
    if ini.exists() && dir.join("config.ini").exists() {
        return Ok(());
    }
    let (platform, abi) = newest_play_image(sdk)?;
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        &ini,
        format!(
            "avd.ini.encoding=UTF-8\npath={}\npath.rel=avd/{AVD_NAME}.avd\ntarget={platform}\n",
            dir.display()
        ),
    )?;
    let sep = std::path::MAIN_SEPARATOR;
    let config = format!(
        "AvdId={AVD_NAME}
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
hw.audioInput=no
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

    if device_state(&sdk, &serial).as_deref() == Some("device") {
        status("Resuming Android");
        // Harmless if it is already running.
        let _ = adb(&sdk, Some(&serial), &["emu", "avd", "start"]);
    } else {
        ensure_avd(&sdk)?;
        status("Starting Android");
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
        quiet(&mut cmd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("starting the emulator")?;
    }

    let device = Device { sdk, serial };
    wait_for_boot(&device, status)?;
    Ok(device)
}

fn wait_for_boot(d: &Device, status: &dyn Fn(&str)) -> Result<()> {
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
        if started.elapsed() > Duration::from_secs(240) {
            bail!("Android did not finish starting within 4 minutes");
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Finds the bridge APK: next to the executable, or in the development tree.
pub fn find_bridge_apk() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("DROMAIUS_BRIDGE_APK") {
        return Some(PathBuf::from(p));
    }
    let exe = std::env::current_exe().ok()?;
    for dir in exe.ancestors().skip(1) {
        for candidate in [
            dir.join("dromaius-bridge.apk"),
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
pub fn ensure_bridge(d: &Device, status: &dyn Fn(&str)) -> Result<()> {
    let apk = find_bridge_apk().ok_or_else(|| anyhow!("Bridge APK not found"))?;
    let meta = std::fs::metadata(&apk)?;
    let stamp = format!(
        "{}:{}:{}",
        d.serial,
        meta.len(),
        meta.modified()?
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs()
    );
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

impl Device {
    /// Pauses the virtual device so the next launch is instant.
    pub fn pause(&self) -> Result<()> {
        adb(&self.sdk, Some(&self.serial), &["emu", "avd", "stop"]).map(|_| ())
    }

    /// Shuts down, saving a Quick Boot snapshot.
    pub fn shutdown(&self) -> Result<()> {
        adb(&self.sdk, Some(&self.serial), &["emu", "kill"]).map(|_| ())
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
    ip_location()
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
    use std::io::{Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};

    let addr = ("ip-api.com", 80)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow!("cannot resolve ip-api.com"))?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(
        b"GET /json/?fields=status,message,city,regionName,lat,lon HTTP/1.0\r\nHost: ip-api.com\r\n\r\n",
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let body = response.split_once("\r\n\r\n").map_or("", |(_, b)| b);
    let v: serde_json::Value =
        serde_json::from_str(body).context("bad response from ip-api.com")?;
    if v["status"] != "success" {
        bail!("location lookup failed: {}", v["message"]);
    }
    let (Some(lat), Some(lon)) = (v["lat"].as_f64(), v["lon"].as_f64()) else {
        bail!("location lookup returned no coordinates");
    };
    let place = [v["city"].as_str(), v["regionName"].as_str()]
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
