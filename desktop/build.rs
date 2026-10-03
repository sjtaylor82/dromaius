use std::path::PathBuf;

fn main() {
    embed_bridge();
    tauri_build::build()
}

/// Embeds the signed Android bridge in the executable, so the download is a
/// single program. Taken from DROMAIUS_EMBED_APK (set by CI), or from the
/// bridge's release build output. Without one, Dromaius falls back to
/// looking for the APK next to the executable at run time.
fn embed_bridge() {
    println!("cargo:rustc-check-cfg=cfg(embedded_bridge)");
    println!("cargo:rerun-if-env-changed=DROMAIUS_EMBED_APK");
    let candidates = [
        std::env::var_os("DROMAIUS_EMBED_APK").map(PathBuf::from),
        Some(PathBuf::from(
            "../android-bridge/app/build/outputs/apk/release/app-release.apk",
        )),
    ];
    for apk in candidates.into_iter().flatten() {
        println!("cargo:rerun-if-changed={}", apk.display());
        if apk.exists() {
            let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("bridge.apk");
            std::fs::copy(&apk, &out).expect("copying the bridge APK");
            println!("cargo:rustc-cfg=embedded_bridge");
            return;
        }
    }
    println!(
        "cargo:warning=No signed bridge APK found to embed; build android-bridge (assembleRelease) first"
    );
}
