//! First-run setup: downloads Android (platform tools, emulator and the newest
//! stable Google Play system image) straight from Google's package repository.
//!
//! This does what Google's `sdkmanager` does, minus the Java requirement:
//! read the repository XML, show the licence, download with the system's
//! `curl`, verify SHA-1 checksums and unpack with the system's `tar`.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};

use crate::device::quiet;

const REPO: &str = "https://dl.google.com/android/repository/";
const PLAY_IMAGES: &str = "https://dl.google.com/android/repository/sys-img/google_apis_playstore/";

#[cfg(windows)]
const HOST: &str = "windows";
#[cfg(target_os = "macos")]
const HOST: &str = "macosx";
#[cfg(all(unix, not(target_os = "macos")))]
const HOST: &str = "linux";

#[cfg(windows)]
const EXE: &str = ".exe";
#[cfg(not(windows))]
const EXE: &str = "";

pub fn image_abi() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64-v8a"
    } else {
        "x86_64"
    }
}

/// Where Dromaius keeps its own copy of Android.
pub fn own_sdk_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("DROMAIUS_SDK_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| {
                let home = PathBuf::from(h);
                if cfg!(target_os = "macos") {
                    home.join("Library/Application Support")
                } else {
                    home.join(".local/share")
                }
            })
        })
        .unwrap_or_else(std::env::temp_dir);
    base.join("Dromaius").join("sdk")
}

/// The newest installed Google Play system image, as ("android-37.0", abi).
pub fn installed_image(sdk: &Path) -> Option<String> {
    let mut best: Option<(Vec<u32>, String)> = None;
    for entry in std::fs::read_dir(sdk.join("system-images")).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(version) = parse_version(&name) else {
            continue;
        };
        let dir = entry.path().join("google_apis_playstore").join(image_abi());
        if dir.join("system.img").exists() && best.as_ref().is_none_or(|(b, _)| version > *b) {
            best = Some((version, name));
        }
    }
    best.map(|(_, name)| name)
}

/// "android-37.0" -> [37, 0]. Extension variants ("android-36-ext19") are skipped.
fn parse_version(platform: &str) -> Option<Vec<u32>> {
    platform
        .strip_prefix("android-")?
        .split('.')
        .map(|p| p.parse().ok())
        .collect()
}

/// Whether `sdk` has everything Dromaius needs to run Android.
pub fn is_complete(sdk: &Path) -> bool {
    sdk.join("platform-tools")
        .join(format!("adb{EXE}"))
        .exists()
        && emulator_is_native(sdk)
        && installed_image(sdk).is_some()
}

/// Whether the installed emulator can run natively on this build's CPU.
///
/// macOS can transparently launch an old Intel emulator through Rosetta on an
/// Apple Silicon Mac. That looks usable until QEMU rejects the ARM64 Android
/// image, so existence alone is not a sufficient installation check there.
fn emulator_is_native(sdk: &Path) -> bool {
    let emulator = sdk.join("emulator").join(format!("emulator{EXE}"));
    if !emulator.exists() {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        let Ok(out) = Command::new("/usr/bin/file").arg(&emulator).output() else {
            // Do not force a large redownload merely because inspection is
            // unavailable. Starting the emulator will still report the error.
            return true;
        };
        if !out.status.success() {
            return true;
        }
        let description = String::from_utf8_lossy(&out.stdout);
        return file_description_has_arch(&description, host_arch());
    }
    #[cfg(not(target_os = "macos"))]
    true
}

#[cfg(any(target_os = "macos", test))]
fn file_description_has_arch(description: &str, arch: &str) -> bool {
    let wanted = if arch == "aarch64" { "arm64" } else { arch };
    description
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .any(|word| word == wanted)
}

#[derive(Debug, Clone)]
pub struct Download {
    pub label: &'static str,
    pub url: String,
    pub sha1: String,
    pub size: u64,
    /// Folder inside the SDK that the archive's contents go into.
    pub dest: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub downloads: Vec<Download>,
    /// Every distinct licence required by the packages in `downloads`.
    pub licenses: Vec<(String, String)>,
    /// The Android release that will be installed, e.g. "Android 17 (API 37)".
    pub android: Option<String>,
}

impl Plan {
    pub fn total_bytes(&self) -> u64 {
        self.downloads.iter().map(|d| d.size).sum()
    }

    /// Text shown before downloading. Keep each licence identifiable so one
    /// acceptance cannot silently stand in for a different package licence.
    pub fn license_text(&self) -> String {
        self.licenses
            .iter()
            .map(|(id, text)| format!("Licence: {id}\n\n{text}"))
            .collect::<Vec<_>>()
            .join("\n\n------------------------------------------------------------\n\n")
    }
}

fn fetch_text(url: &str) -> Result<String> {
    let out = quiet(&mut Command::new(curl()))
        .args(["-sSfL", "--retry", "3", url])
        .stdin(Stdio::null())
        .output()
        .context("running curl")?;
    if !out.status.success() {
        bail!(
            "could not download {url}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8(out.stdout)?)
}

fn curl() -> &'static str {
    if cfg!(windows) { "curl.exe" } else { "curl" }
}

fn tar() -> &'static str {
    if cfg!(windows) { "tar.exe" } else { "tar" }
}

/// One downloadable package version, from Google's repository.
struct Archive {
    url: String,
    sha1: String,
    size: u64,
    license: String,
    revision: String,
}

/// The stable-channel archive of a package for this host.
fn find_archive(doc: &roxmltree::Document, path: &str) -> Option<Archive> {
    find_archive_for(doc, path, HOST, host_arch())
}

fn host_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86_64"
    }
}

/// Finds the package for this operating system and CPU. Google's macOS
/// repository can contain both Intel and Apple Silicon emulator archives;
/// choosing by operating system alone can install an emulator that cannot run.
fn find_archive_for(
    doc: &roxmltree::Document,
    path: &str,
    host: &str,
    host_arch: &str,
) -> Option<Archive> {
    let stable = doc
        .descendants()
        .find(|n| n.has_tag_name("channel") && n.text() == Some("stable"))
        .and_then(|n| n.attribute("id"))
        .unwrap_or("channel-0");
    let package = doc.descendants().find(|n| {
        n.has_tag_name("remotePackage")
            && n.attribute("path") == Some(path)
            && n.descendants()
                .any(|c| c.has_tag_name("channelRef") && c.attribute("ref") == Some(stable))
    })?;
    let license = package
        .descendants()
        .find(|n| n.has_tag_name("uses-license"))
        .and_then(|n| n.attribute("ref"))
        .unwrap_or("android-sdk-license")
        .to_string();
    let revision = package
        .children()
        .find(|n| n.has_tag_name("revision"))
        .map(|r| {
            ["major", "minor", "micro"]
                .iter()
                .filter_map(|t| {
                    r.children()
                        .find(|n| n.has_tag_name(*t))
                        .and_then(|n| n.text())
                })
                .collect::<Vec<_>>()
                .join(".")
        })
        .unwrap_or_default();
    let candidates = package
        .descendants()
        .filter(|n| n.has_tag_name("archive"))
        .filter(|a| {
            a.descendants()
                .find(|n| n.has_tag_name("host-os"))
                .is_none_or(|os| os.text() == Some(host))
        });
    // Prefer an explicitly matching architecture. Older packages without a
    // host-arch remain a fallback, but an archive for the other CPU never is.
    let archive = candidates
        .clone()
        .find(|a| {
            a.descendants()
                .find(|n| n.has_tag_name("host-arch"))
                .is_some_and(|arch| arch.text() == Some(host_arch))
        })
        .or_else(|| {
            candidates.into_iter().find(|a| {
                a.descendants()
                    .find(|n| n.has_tag_name("host-arch"))
                    .is_none()
            })
        })?;
    let text = |tag: &str| {
        archive
            .descendants()
            .find(|n| n.has_tag_name(tag))
            .and_then(|n| n.text())
            .map(str::to_string)
    };
    Some(Archive {
        url: text("url")?,
        sha1: text("checksum")?,
        size: text("size")?.parse().ok()?,
        license,
        revision,
    })
}

/// The newest stable Google Play image for this CPU, as (platform, archive).
/// `pin` restricts the choice to one release (a testing aid, see `plan`).
fn newest_image(images: &roxmltree::Document, pin: Option<&str>) -> Option<(String, Archive)> {
    let suffix = format!(";google_apis_playstore;{}", image_abi());
    let mut candidates: Vec<(Vec<u32>, String)> = images
        .descendants()
        .filter(|n| n.has_tag_name("remotePackage"))
        .filter_map(|n| {
            let path = n.attribute("path")?;
            let platform = path
                .strip_prefix("system-images;")?
                .strip_suffix(suffix.as_str())?;
            Some((parse_version(platform)?, path.to_string()))
        })
        .collect();
    candidates.sort();
    if let Some(pin) = pin {
        candidates.retain(|(_, path)| path.split(';').nth(1) == Some(pin));
    }
    candidates.iter().rev().find_map(|(_, path)| {
        let platform = path.split(';').nth(1)?.to_string();
        find_archive(images, path).map(|a| (platform, a))
    })
}

fn license_text(doc: &roxmltree::Document, id: &str) -> Option<String> {
    doc.descendants()
        .find(|n| n.has_tag_name("license") && n.attribute("id") == Some(id))
        .and_then(|n| n.text())
        .map(|t| t.trim().to_string())
}

/// "android-37.0" or "37" -> "Android 17 (API 37)". Since API 33 (Android
/// 13), the Android version is the API level minus 20.
pub fn android_name(platform_or_api: &str) -> String {
    let api = platform_or_api.trim_start_matches("android-");
    let major: u32 = api
        .split('.')
        .next()
        .and_then(|m| m.parse().ok())
        .unwrap_or(0);
    let api = api.trim_end_matches(".0");
    if major >= 33 {
        format!("Android {} (API {api})", major - 20)
    } else {
        format!("Android API {api}")
    }
}

/// Works out what needs downloading into `sdk`.
pub fn plan(sdk: &Path) -> Result<Plan> {
    let repo_xml = fetch_text(&format!("{REPO}repository2-3.xml"))?;
    let repo = roxmltree::Document::parse(&repo_xml).context("reading Google's package list")?;
    let mut downloads = Vec::new();
    let mut licenses = BTreeMap::<String, String>::new();

    for (path, label, exe) in [
        ("platform-tools", "Android tools", "adb"),
        ("emulator", "Android emulator", "emulator"),
    ] {
        let installed = sdk.join(path).join(format!("{exe}{EXE}")).exists();
        if installed && (path != "emulator" || emulator_is_native(sdk)) {
            continue;
        }
        let a = find_archive(&repo, path)
            .ok_or_else(|| anyhow!("{path} not found in Google's package list"))?;
        let text = license_text(&repo, &a.license)
            .ok_or_else(|| anyhow!("licence {} not found", a.license))?;
        licenses.insert(a.license.clone(), text);
        downloads.push(Download {
            label,
            url: format!("{REPO}{}", a.url),
            sha1: a.sha1,
            size: a.size,
            dest: sdk.join(path),
        });
    }

    let mut android = installed_image(sdk);
    if android.is_none() {
        let img_xml = fetch_text(&format!("{PLAY_IMAGES}sys-img2-3.xml"))?;
        let images = roxmltree::Document::parse(&img_xml).context("reading Google's image list")?;
        // Testing aid: DROMAIUS_TEST_ANDROID=android-36 installs that release
        // instead of the newest, so upgrades can be tried out.
        let pin = std::env::var("DROMAIUS_TEST_ANDROID").ok();
        let (platform, a) = newest_image(&images, pin.as_deref())
            .ok_or_else(|| anyhow!("no Google Play system image available for {}", image_abi()))?;
        let text = license_text(&images, &a.license)
            .or_else(|| license_text(&repo, &a.license))
            .ok_or_else(|| anyhow!("licence {} not found", a.license))?;
        licenses.insert(a.license.clone(), text);
        downloads.push(Download {
            label: "Android system (Google Play)",
            url: format!("{PLAY_IMAGES}{}", a.url),
            sha1: a.sha1,
            size: a.size,
            dest: sdk
                .join("system-images")
                .join(&platform)
                .join("google_apis_playstore")
                .join(image_abi()),
        });
        android = Some(platform);
    }

    Ok(Plan {
        downloads,
        licenses: licenses.into_iter().collect(),
        android: android.map(|p| android_name(&p)),
    })
}

fn package_revision(dir: &Path) -> Option<String> {
    let props = std::fs::read_to_string(dir.join("source.properties")).ok()?;
    props
        .lines()
        .find_map(|l| l.strip_prefix("Pkg.Revision="))
        .map(|r| r.trim().to_string())
}

fn newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| {
        v.split('.')
            .map(|p| p.parse::<u32>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    parse(candidate) > parse(current)
}

/// Installed versions, and any newer stable versions Google offers.
#[derive(Debug, Clone, Default)]
pub struct Versions {
    pub emulator: Option<String>,
    pub newer_emulator: Option<String>,
    /// Newest Android release offered, with its download size, when newer
    /// than the one in use.
    pub newer_android: Option<(String, u64)>,
}

/// Compares what's installed in `sdk` (and the Android release `in_use`,
/// e.g. "android-37.0") with Google's newest stable releases.
pub fn versions(sdk: &Path, in_use: Option<&str>) -> Versions {
    let mut v = Versions {
        emulator: package_revision(&sdk.join("emulator")),
        ..Default::default()
    };
    if let Ok(xml) = fetch_text(&format!("{REPO}repository2-3.xml"))
        && let Ok(repo) = roxmltree::Document::parse(&xml)
        && let (Some(a), Some(current)) = (find_archive(&repo, "emulator"), v.emulator.as_deref())
        && newer(&a.revision, current)
    {
        v.newer_emulator = Some(a.revision);
    }
    if let Ok(xml) = fetch_text(&format!("{PLAY_IMAGES}sys-img2-3.xml"))
        && let Ok(images) = roxmltree::Document::parse(&xml)
        && let Some((platform, a)) = newest_image(&images, None)
        && let Some(current) = in_use.and_then(parse_version)
        && parse_version(&platform).is_some_and(|p| p > current)
    {
        v.newer_android = Some((android_name(&platform), a.size));
    }
    v
}

/// Records licence acceptance the way sdkmanager does (a hash of the text).
pub fn record_license(sdk: &Path, plan: &Plan) -> Result<()> {
    let dir = sdk.join("licenses");
    std::fs::create_dir_all(&dir)?;
    for (id, text) in &plan.licenses {
        let hash = sha1_smol::Sha1::from(text.as_bytes()).digest().to_string();
        std::fs::write(dir.join(id), format!("\n{hash}"))?;
    }
    Ok(())
}

pub enum Progress {
    Downloading {
        label: &'static str,
        done: u64,
        total: u64,
    },
    Unpacking {
        label: &'static str,
    },
}

/// Downloads, verifies and unpacks one archive. `before` is the number of
/// bytes already downloaded by earlier items, for overall progress.
pub fn install(
    sdk: &Path,
    d: &Download,
    before: u64,
    total: u64,
    progress: &dyn Fn(Progress),
) -> Result<()> {
    let file = fetch(sdk, d, before, total, progress)?;
    progress(Progress::Unpacking { label: d.label });
    unpack(&file, d)
}

/// Downloads an archive (resuming if interrupted) and verifies its checksum.
/// Returns the downloaded file.
pub fn fetch(
    sdk: &Path,
    d: &Download,
    before: u64,
    total: u64,
    progress: &dyn Fn(Progress),
) -> Result<PathBuf> {
    let tmp = sdk.join(".downloads");
    std::fs::create_dir_all(&tmp)?;
    let file_name = d.url.rsplit('/').next().unwrap_or("download.zip");
    let part = tmp.join(format!("{file_name}.part"));

    // Resume partial downloads (-C -); retry transient failures.
    let mut child = quiet(&mut Command::new(curl()))
        .args(["-sSfL", "--retry", "5", "-C", "-", "-o"])
        .arg(&part)
        .arg(&d.url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("starting curl")?;
    loop {
        let done = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        progress(Progress::Downloading {
            label: d.label,
            done: before + done,
            total,
        });
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                let mut err = String::new();
                if let Some(mut e) = child.stderr.take() {
                    let _ = e.read_to_string(&mut err);
                }
                bail!("downloading {} failed: {}", d.label, err.trim());
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    let digest = sha1_file(&part)?;
    if !digest.eq_ignore_ascii_case(&d.sha1) {
        let _ = std::fs::remove_file(&part);
        bail!(
            "{} download was corrupted (checksum mismatch); please try again",
            d.label
        );
    }
    Ok(part)
}

/// Replaces `d.dest` with the contents of a downloaded archive, then deletes it.
pub fn unpack(part: &Path, d: &Download) -> Result<()> {
    if d.dest.exists() {
        std::fs::remove_dir_all(&d.dest)?;
    }
    std::fs::create_dir_all(&d.dest)?;
    // Archives contain one top-level folder (e.g. "emulator/"); unpack its contents.
    let out = quiet(&mut Command::new(tar()))
        .args(["-xf"])
        .arg(part)
        .args(["--strip-components", "1", "-C"])
        .arg(&d.dest)
        .stdin(Stdio::null())
        .output()
        .context("starting tar")?;
    if !out.status.success() {
        bail!(
            "unpacking {} failed: {}",
            d.label,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let _ = std::fs::remove_file(part);
    Ok(())
}

/// The newest stable emulator, ready to download into `sdk`.
pub fn emulator_download(sdk: &Path) -> Result<Download> {
    let xml = fetch_text(&format!("{REPO}repository2-3.xml"))?;
    let repo = roxmltree::Document::parse(&xml).context("reading Google's package list")?;
    let a = find_archive(&repo, "emulator")
        .ok_or_else(|| anyhow!("emulator not found in Google's package list"))?;
    Ok(Download {
        label: "Android emulator update",
        url: format!("{REPO}{}", a.url),
        sha1: a.sha1,
        size: a.size,
        dest: sdk.join("emulator"),
    })
}

/// The newest stable Google Play image, ready to download into `sdk`, with
/// its platform name (e.g. "android-38.0").
pub fn android_download(sdk: &Path) -> Result<(String, Download)> {
    let xml = fetch_text(&format!("{PLAY_IMAGES}sys-img2-3.xml"))?;
    let images = roxmltree::Document::parse(&xml).context("reading Google's image list")?;
    let (platform, a) = newest_image(&images, None)
        .ok_or_else(|| anyhow!("no Google Play system image available for {}", image_abi()))?;
    let download = Download {
        label: "Android system update",
        url: format!("{PLAY_IMAGES}{}", a.url),
        sha1: a.sha1,
        size: a.size,
        dest: sdk
            .join("system-images")
            .join(&platform)
            .join("google_apis_playstore")
            .join(image_abi()),
    };
    Ok((platform, download))
}

fn sha1_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = sha1_smol::Sha1::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.digest().to_string())
}

/// Disk space needed: the downloads, their unpacked contents, and room for
/// the first Quick Boot snapshot (about the size of Android's 4 GB of memory).
pub fn space_needed(plan: &Plan) -> u64 {
    plan.total_bytes() * 5 / 2 + 4_000_000_000
}

/// Free space available on the drive holding `dir` (or its nearest existing parent).
pub fn free_space(dir: &Path) -> Option<u64> {
    let existing = dir.ancestors().find(|p| p.exists())?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = existing.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut free = 0u64;
        // SAFETY: `wide` is a NUL-terminated path that outlives the call.
        unsafe {
            windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
                windows::core::PCWSTR(wide.as_ptr()),
                Some(&mut free),
                None,
                None,
            )
            .ok()?;
        }
        Some(free)
    }
    #[cfg(not(windows))]
    {
        let out = Command::new("df")
            .args(["-Pk"])
            .arg(existing)
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let kb: u64 = text
            .lines()
            .nth(1)?
            .split_whitespace()
            .nth(3)?
            .parse()
            .ok()?;
        Some(kb * 1024)
    }
}

/// Quick check, before downloading anything, that Windows' hypervisor
/// platform is installed (the emulator needs it). The definitive check is
/// `check_acceleration`, once the emulator is downloaded.
pub fn hypervisor_platform_installed() -> bool {
    #[cfg(windows)]
    {
        let root = std::env::var_os("SystemRoot")
            .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        root.join("System32").join("WinHvPlatform.dll").exists()
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Checks that the emulator can use hardware virtualization. Returns the
/// emulator's explanation when it can't.
pub fn check_acceleration(sdk: &Path) -> Result<(), String> {
    let out = quiet(&mut Command::new(
        sdk.join("emulator").join(format!("emulator{EXE}")),
    ))
    .arg("-accel-check")
    .stdin(Stdio::null())
    .output()
    .map_err(|e| format!("could not run the emulator: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() {
        Ok(())
    } else {
        Err(text)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_and_versions() {
        assert_eq!(super::android_name("android-37.0"), "Android 17 (API 37)");
        assert_eq!(super::android_name("36"), "Android 16 (API 36)");
        assert_eq!(super::android_name("android-36.1"), "Android 16 (API 36.1)");
        assert!(super::newer("37.3.1", "37.2.12"));
        assert!(!super::newer("37.2.12", "37.2.12"));
    }

    #[test]
    fn versions() {
        assert_eq!(super::parse_version("android-37.0"), Some(vec![37, 0]));
        assert_eq!(super::parse_version("android-36"), Some(vec![36]));
        assert_eq!(super::parse_version("android-36-ext19"), None);
    }

    #[test]
    fn selects_the_native_macos_emulator() {
        let xml = r#"
          <repository>
            <channel id="channel-0">stable</channel>
            <license id="android-sdk-license">licence</license>
            <remotePackage path="emulator">
              <revision><major>36</major><minor>2</minor><micro>0</micro></revision>
              <channelRef ref="channel-0"/>
              <uses-license ref="android-sdk-license"/>
              <archives>
                <archive><host-os>macosx</host-os><host-arch>x86_64</host-arch><complete><size>1</size><checksum>intel</checksum><url>emulator-darwin_x64.zip</url></complete></archive>
                <archive><host-os>macosx</host-os><host-arch>aarch64</host-arch><complete><size>2</size><checksum>arm</checksum><url>emulator-darwin_aarch64.zip</url></complete></archive>
              </archives>
            </remotePackage>
          </repository>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let arm = super::find_archive_for(&doc, "emulator", "macosx", "aarch64").unwrap();
        let intel = super::find_archive_for(&doc, "emulator", "macosx", "x86_64").unwrap();
        assert_eq!(arm.url, "emulator-darwin_aarch64.zip");
        assert_eq!(intel.url, "emulator-darwin_x64.zip");
    }

    #[test]
    fn recognises_macos_binary_architectures() {
        let arm = "emulator: Mach-O 64-bit executable arm64";
        let intel = "emulator: Mach-O 64-bit executable x86_64";
        let universal = "emulator: Mach-O universal binary with 2 architectures: [x86_64] [arm64]";
        assert!(super::file_description_has_arch(arm, "aarch64"));
        assert!(!super::file_description_has_arch(arm, "x86_64"));
        assert!(super::file_description_has_arch(intel, "x86_64"));
        assert!(!super::file_description_has_arch(intel, "aarch64"));
        assert!(super::file_description_has_arch(universal, "aarch64"));
        assert!(super::file_description_has_arch(universal, "x86_64"));
    }

    #[test]
    #[ignore = "needs internet"]
    fn plans_a_fresh_install() {
        let dir = std::env::temp_dir().join("dromaius-plan-test");
        let plan = super::plan(&dir).unwrap();
        for d in &plan.downloads {
            println!(
                "{} {} MB -> {}",
                d.label,
                d.size / 1_000_000,
                d.dest.display()
            );
        }
        println!(
            "{} licence(s) ({} chars)",
            plan.licenses.len(),
            plan.license_text().len()
        );
        assert_eq!(plan.downloads.len(), 3);
    }
}
