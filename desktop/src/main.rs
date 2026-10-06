#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod core;
mod device;
mod mirror;
mod protocol;
mod setup;
mod timing;
mod updater;
mod view;

use std::path::PathBuf;
#[cfg(target_os = "windows")]
use std::process::Command;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};

use tauri::{Manager, State, WindowEvent};

use crate::core::{BackendEvent, Core, InitPayload};
use crate::view::parse_id;

/// Set DROMAIUS_DEBUG=1 to save each Android snapshot to the temp folder.
pub fn debug_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("DROMAIUS_DEBUG").is_some())
}

#[derive(Default)]
pub struct Options {
    /// Load a saved snapshot instead of starting Android (for testing).
    pub mock: Option<PathBuf>,
    /// Show the emulator's own window (useful for sighted helpers and debugging).
    pub show_emulator: bool,
    /// Shut the emulator down on exit instead of pausing it.
    pub shutdown_on_exit: bool,
    /// A Play Store link to open once connected.
    pub install: Option<String>,
    /// Don't set the emulator's GPS position from the PC's location.
    pub no_location: bool,
}

const USAGE: &str = "Usage: dromaius [--mock FILE] [--show-emulator] [--shutdown-on-exit] [--no-location] [--install PLAY_LINK]";

fn parse_args() -> Result<Options, String> {
    let mut opts = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--mock" => opts.mock = Some(args.next().ok_or("--mock needs a file")?.into()),
            "--show-emulator" => opts.show_emulator = true,
            "--shutdown-on-exit" => opts.shutdown_on_exit = true,
            "--no-location" => opts.no_location = true,
            "--install" => opts.install = Some(args.next().ok_or("--install needs a link")?),
            "-h" | "--help" => return Err(USAGE.into()),
            // A bare Play link, e.g. when registered as a link handler.
            other if other.contains("play.google.com") || other.starts_with("market://") => {
                opts.install = Some(other.to_string())
            }
            other => return Err(format!("Unknown argument {other}\n{USAGE}")),
        }
    }
    Ok(opts)
}

type Shared = Arc<Mutex<Core>>;

#[cfg(target_os = "windows")]
pub(crate) fn open_messenger_web() -> Result<(), String> {
    Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", "https://www.messenger.com/"])
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open Messenger in your browser: {e}"))
}

fn with_node(id: &str, f: impl FnOnce(u64)) -> Result<(), String> {
    f(parse_id(id).ok_or_else(|| format!("bad id {id}"))?);
    Ok(())
}

#[tauri::command]
fn init(core: State<Shared>, window: tauri::WebviewWindow) -> InitPayload {
    // WebView2 doesn't always take keyboard focus when the window opens,
    // leaving screen reader users unable to navigate until they click in it.
    let _ = window.set_focus();
    let webview: &tauri::Webview = window.as_ref();
    let _ = webview.set_focus();
    core.lock().unwrap().init_payload()
}

#[tauri::command]
fn app_info(core: State<Shared>, package: String) {
    core.lock().unwrap().app_screen(&package, false);
}

#[tauri::command]
fn uninstall(core: State<Shared>, package: String) {
    core.lock().unwrap().app_screen(&package, true);
}

#[tauri::command]
fn release(core: State<Shared>) {
    core.lock().unwrap().release();
}

#[tauri::command]
fn ctrl_key(core: State<Shared>, key: String) {
    core.lock().unwrap().ctrl_key(&key);
}

#[tauri::command]
fn navigation_key(core: State<Shared>, key: String) {
    core.lock().unwrap().navigation_key(&key);
}

#[tauri::command]
fn act(core: State<Shared>, id: String, action: String) -> Result<(), String> {
    with_node(&id, |id| core.lock().unwrap().act(id, &action))
}

#[tauri::command]
fn set_text(
    core: State<Shared>,
    id: String,
    text: String,
    start: usize,
    end: usize,
) -> Result<(), String> {
    with_node(&id, |id| {
        core.lock().unwrap().set_text(id, &text, start, end)
    })
}

#[tauri::command]
fn set_progress(core: State<Shared>, id: String, value: f64) -> Result<(), String> {
    with_node(&id, |id| core.lock().unwrap().set_progress(id, value))
}

#[tauri::command]
fn custom_action(core: State<Shared>, id: String, action_id: i64) -> Result<(), String> {
    with_node(&id, |id| core.lock().unwrap().custom_action(id, action_id))
}

#[tauri::command]
fn scroll(core: State<Shared>, id: String, forward: bool) -> Result<(), String> {
    with_node(&id, |id| core.lock().unwrap().scroll(id, forward))
}

#[tauri::command]
fn global(core: State<Shared>, action: String) {
    let allowed = ["back", "home", "recents", "notifications", "quickSettings"];
    if allowed.contains(&action.as_str()) {
        core.lock().unwrap().global(&action);
    }
}

#[tauri::command]
fn start_update(core: State<Shared>, kind: String, keep_backup: Option<bool>) {
    core.lock()
        .unwrap()
        .start_update(&kind, keep_backup.unwrap_or(true));
}

#[tauri::command]
fn answer_license(core: State<Shared>, accepted: bool) {
    core.lock().unwrap().answer_license(accepted);
}

#[tauri::command]
fn refresh(core: State<Shared>) {
    core.lock().unwrap().refresh();
}

#[tauri::command]
fn display_mode(core: State<Shared>) -> Result<String, String> {
    core.lock().unwrap().display_mode()
}

#[tauri::command]
fn set_display_mode(core: State<Shared>, mode: String) -> Result<String, String> {
    core.lock().unwrap().set_display_mode(&mode)
}

#[tauri::command]
fn show_apps(core: State<Shared>) {
    core.lock().unwrap().show_apps();
}

#[tauri::command]
fn launch(core: State<Shared>, package: String) -> Result<(), String> {
    // Google Play currently supplies Messenger only as ARM native code. On an
    // x86 Windows emulator it is translated at runtime and crashes during
    // startup in libsuperpack-jni.so. Use Meta's supported web client in the
    // system browser, which also owns its login, notification, camera and
    // microphone permissions. Apple Silicon can run the Android build natively.
    #[cfg(target_os = "windows")]
    if package == "com.facebook.orca" {
        return open_messenger_web();
    }

    core.lock().unwrap().launch(&package);
    Ok(())
}

#[tauri::command]
fn install_link(core: State<Shared>, link: String) {
    core.lock().unwrap().install_link(&link);
}

#[tauri::command]
async fn check_app_update() -> Result<Option<updater::UpdateInfo>, String> {
    updater::check().await.map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn install_app_update(
    app: tauri::AppHandle,
    update: updater::UpdateInfo,
) -> Result<(), String> {
    updater::install(&app, &update)
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn quit_for_update(app: tauri::AppHandle, core: State<Shared>) {
    core.lock().unwrap().on_exit();
    app.exit(0);
}

fn main() {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };
    let mut opts = Some(opts);
    timing::start();

    tauri::Builder::default()
        .setup(move |app| {
            let (tx, rx) = mpsc::channel::<BackendEvent>();
            let core: Shared = Arc::new(Mutex::new(Core::new(
                app.handle().clone(),
                tx,
                opts.take().unwrap_or_default(),
            )));
            app.manage(core.clone());
            core.lock().unwrap().start();
            std::thread::spawn(move || {
                for ev in rx {
                    core.lock().unwrap().handle(ev);
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let core = window.state::<Shared>();
                let mut core = core.lock().unwrap();
                // Closing mid-update could leave Android half switched over.
                if core.updating() {
                    api.prevent_close();
                    core.explain_busy();
                } else {
                    core.on_exit();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            init,
            act,
            set_text,
            set_progress,
            custom_action,
            scroll,
            global,
            refresh,
            display_mode,
            set_display_mode,
            show_apps,
            launch,
            install_link,
            answer_license,
            start_update,
            release,
            ctrl_key,
            navigation_key,
            app_info,
            uninstall,
            check_app_update,
            install_app_update,
            quit_for_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running Dromaius");
}
