//! First-run setup: downloads Android (platform tools, emulator and the newest
//! stable Google Play system image) straight from Google's package repository.
//!
//! This does what Google's `sdkmanager` does, minus the Java requirement:
//! read the repository XML, show the licence, download with the system's
//! `curl`, verify SHA-1 checksums and unpack with the system's `tar`.

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
        && sdk.join("emulator").join(format!("emulator{EXE}")).exists()
        && installed_image(sdk).is_some()
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
    pub license_id: String,
    pub license_text: String,
}

impl Plan {
    pub fn total_bytes(&self) -> u64 {
        self.downloads.iter().map(|d| d.size).sum()
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

/// The stable-channel archive of a package for this host.
fn find_archive(doc: &roxmltree::Document, path: &str) -> Option<(String, String, u64, String)> {
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
    let archive = package
        .descendants()
        .filter(|n| n.has_tag_name("archive"))
        .find(
            |a| match a.descendants().find(|n| n.has_tag_name("host-os")) {
                Some(os) => os.text() == Some(HOST),
                None => true,
            },
        )?;
    let text = |tag: &str| {
        archive
            .descendants()
            .find(|n| n.has_tag_name(tag))
            .and_then(|n| n.text())
            .map(str::to_string)
    };
    Some((
        text("url")?,
        text("checksum")?,
        text("size")?.parse().ok()?,
        license,
    ))
}

fn license_text(doc: &roxmltree::Document, id: &str) -> Option<String> {
    doc.descendants()
        .find(|n| n.has_tag_name("license") && n.attribute("id") == Some(id))
        .and_then(|n| n.text())
        .map(|t| t.trim().to_string())
}

/// Works out what needs downloading into `sdk`.
pub fn plan(sdk: &Path) -> Result<Plan> {
    let repo_xml = fetch_text(&format!("{REPO}repository2-3.xml"))?;
    let repo = roxmltree::Document::parse(&repo_xml).context("reading Google's package list")?;
    let mut downloads = Vec::new();
    let mut license_id = "android-sdk-license".to_string();

    for (path, label, exe) in [
        ("platform-tools", "Android tools", "adb"),
        ("emulator", "Android emulator", "emulator"),
    ] {
        if sdk.join(path).join(format!("{exe}{EXE}")).exists() {
            continue;
        }
        let (url, sha1, size, license) = find_archive(&repo, path)
            .ok_or_else(|| anyhow!("{path} not found in Google's package list"))?;
        if license != "android-sdk-license" {
            license_id = license;
        }
        downloads.push(Download {
            label,
            url: format!("{REPO}{url}"),
            sha1,
            size,
            dest: sdk.join(path),
        });
    }

    let mut license_text_value = license_text(&repo, &license_id);
    if installed_image(sdk).is_none() {
        let img_xml = fetch_text(&format!("{PLAY_IMAGES}sys-img2-3.xml"))?;
        let images = roxmltree::Document::parse(&img_xml).context("reading Google's image list")?;
        // Newest stable "system-images;android-N;google_apis_playstore;<abi>".
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
        let (url, sha1, size, license, path) = candidates
            .iter()
            .rev()
            .find_map(|(_, path)| {
                find_archive(&images, path).map(|(u, s, z, l)| (u, s, z, l, path.clone()))
            })
            .ok_or_else(|| anyhow!("no Google Play system image available for {}", image_abi()))?;
        let platform = path.split(';').nth(1).unwrap_or_default();
        if license != license_id {
            license_text_value = license_text(&images, &license).or(license_text_value);
            license_id = license;
        }
        downloads.push(Download {
            label: "Android system (Google Play)",
            url: format!("{PLAY_IMAGES}{url}"),
            sha1,
            size,
            dest: sdk
                .join("system-images")
                .join(platform)
                .join("google_apis_playstore")
                .join(image_abi()),
        });
    }

    let license_text =
        license_text_value.ok_or_else(|| anyhow!("licence {license_id} not found"))?;
    Ok(Plan {
        downloads,
        license_id,
        license_text,
    })
}

/// Records licence acceptance the way sdkmanager does (a hash of the text).
pub fn record_license(sdk: &Path, plan: &Plan) -> Result<()> {
    let dir = sdk.join("licenses");
    std::fs::create_dir_all(&dir)?;
    let hash = sha1_smol::Sha1::from(plan.license_text.as_bytes())
        .digest()
        .to_string();
    std::fs::write(dir.join(&plan.license_id), format!("\n{hash}"))?;
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

    progress(Progress::Unpacking { label: d.label });
    if d.dest.exists() {
        std::fs::remove_dir_all(&d.dest)?;
    }
    std::fs::create_dir_all(&d.dest)?;
    // Archives contain one top-level folder (e.g. "emulator/"); unpack its contents.
    let out = quiet(&mut Command::new(tar()))
        .args(["-xf"])
        .arg(&part)
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
    let _ = std::fs::remove_file(&part);
    Ok(())
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
    fn versions() {
        assert_eq!(super::parse_version("android-37.0"), Some(vec![37, 0]));
        assert_eq!(super::parse_version("android-36"), Some(vec![36]));
        assert_eq!(super::parse_version("android-36-ext19"), None);
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
            "licence {} ({} chars)",
            plan.license_id,
            plan.license_text.len()
        );
        assert_eq!(plan.downloads.len(), 3);
    }
}
