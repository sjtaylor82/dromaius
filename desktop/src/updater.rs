//! Checks GitHub releases and installs verified portable updates.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::header::{ACCEPT, USER_AGENT};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{Emitter, Manager};

const API: &str = "https://api.github.com/repos/sjtaylor82/dromaius/releases/latest";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub asset_name: String,
    pub asset_url: String,
    pub checksums_url: String,
    pub release_url: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn version(value: &str) -> Vec<u64> {
    value
        .trim_start_matches('v')
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

fn wanted_asset(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    #[cfg(target_os = "windows")]
    return name.ends_with("-windows-x64.zip");
    #[cfg(target_os = "macos")]
    return name.ends_with("-macos-universal.zip");
    #[allow(unreachable_code)]
    false
}

fn select(release: Release) -> Option<UpdateInfo> {
    if version(&release.tag_name) <= version(env!("CARGO_PKG_VERSION")) {
        return None;
    }
    #[cfg(target_os = "windows")]
    let checksum_name = "SHA256SUMS-windows.txt";
    #[cfg(target_os = "macos")]
    let checksum_name = "SHA256SUMS-macos.txt";
    let checksums_url = release
        .assets
        .iter()
        .find(|asset| asset.name.eq_ignore_ascii_case(checksum_name))?
        .browser_download_url
        .clone();
    let asset = release
        .assets
        .iter()
        .find(|asset| wanted_asset(&asset.name))?;
    Some(UpdateInfo {
        version: release.tag_name.trim_start_matches('v').to_string(),
        asset_name: asset.name.clone(),
        asset_url: asset.browser_download_url.clone(),
        checksums_url,
        release_url: release.html_url,
    })
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?)
}

pub async fn check() -> Result<Option<UpdateInfo>> {
    let release = client()?
        .get(API)
        .header(
            USER_AGENT,
            format!("Dromaius/{}", env!("CARGO_PKG_VERSION")),
        )
        .header(ACCEPT, "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json::<Release>()
        .await?;
    Ok(select(release))
}

fn expected_hash(text: &str, filename: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let hash = fields.next()?;
        let name = fields.next()?.trim_start_matches('*');
        (name == filename && hash.len() == 64).then(|| hash.to_ascii_lowercase())
    })
}

pub async fn install(app: &tauri::AppHandle, update: &UpdateInfo) -> Result<()> {
    if !update.asset_url.starts_with("https://github.com/")
        || !update.checksums_url.starts_with("https://github.com/")
    {
        bail!("The release contains an unexpected download address");
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&update.release_url)
            .spawn()
            .context("could not open the release page")?;
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let http = client()?;
        app.emit("app-update-status", "Downloading update")?;
        let checksums = http
            .get(&update.checksums_url)
            .header(
                USER_AGENT,
                format!("Dromaius/{}", env!("CARGO_PKG_VERSION")),
            )
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let expected = expected_hash(&checksums, &update.asset_name)
            .context("the release checksum does not list this update")?;
        let bytes = http
            .get(&update.asset_url)
            .header(
                USER_AGENT,
                format!("Dromaius/{}", env!("CARGO_PKG_VERSION")),
            )
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if actual != expected {
            bail!("the downloaded update failed SHA-256 verification");
        }

        let temp = std::env::temp_dir();
        let zip = temp.join(&update.asset_name);
        std::fs::write(&zip, bytes).context("could not save the update")?;
        let script = temp.join(format!("dromaius-update-{}.ps1", std::process::id()));
        std::fs::write(&script, include_str!("../../dist/portable-updater.ps1"))?;
        let exe = std::env::current_exe()?;
        let directory = exe
            .parent()
            .context("the application folder is unavailable")?;
        let log = app.path().app_log_dir()?.join("update.log");
        if let Some(parent) = log.parent() {
            std::fs::create_dir_all(parent)?;
        }

        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script)
            .arg("-ProcessId")
            .arg(std::process::id().to_string())
            .arg("-ZipPath")
            .arg(&zip)
            .arg("-AppDirectory")
            .arg(directory)
            .arg("-ExecutablePath")
            .arg(&exe)
            .arg("-LogPath")
            .arg(&log)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .context("could not start the update helper")?;
        app.emit("app-update-ready", ())?;
        Ok(())
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    bail!("automatic updates are unavailable on this platform")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_numerically() {
        assert!(version("v0.10.0") > version("0.2.9"));
    }

    #[test]
    fn reads_checksum_lines() {
        let hash = "a".repeat(64);
        let text = format!("{hash} *Dromaius-0.2.0-windows-x64.zip\n");
        assert_eq!(
            expected_hash(&text, "Dromaius-0.2.0-windows-x64.zip"),
            Some(hash)
        );
    }
}
