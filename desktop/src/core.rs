//! Backend state: the mirrored Android screen, the app list, and what the web
//! front end should show and where its focus should go.

use std::collections::HashSet;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::Options;
use crate::bridge::Bridge;
use crate::device::{self, Device};
use crate::mirror::{ANDROID_ID_BIT, Model};
use crate::protocol::{AppInfo, FromBridge, Snapshot, ToBridge};
use crate::view::{self, ViewNode};

pub enum BackendEvent {
    Setup(SetupPayload),
    LicenseRequest {
        text: String,
        download_bytes: u64,
        reply: Sender<bool>,
    },
    Bridge(FromBridge),
    BridgeDisconnected,
    Status(String),
    DeviceReady(Device),
    DeviceFailed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Android is starting or not connected yet.
    Starting,
    /// Our own list of installed apps (Android's home screen stays hidden).
    Apps,
    /// Inside an Android app.
    App,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatePayload {
    pub connected: bool,
    pub status: String,
    pub mode: Mode,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenPayload {
    pub title: String,
    pub package: String,
    pub nodes: Vec<ViewNode>,
    /// The user moved to a different screen: the UI moves to its heading.
    pub new_screen: bool,
    /// An element the UI should move focus to.
    pub focus: Option<String>,
}

/// First-run setup, as shown in the starting view.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "stage", rename_all = "camelCase")]
pub enum SetupPayload {
    Checking,
    #[serde(rename_all = "camelCase")]
    License {
        text: String,
        download_mb: u64,
    },
    #[serde(rename_all = "camelCase")]
    Downloading {
        label: String,
        percent: u8,
        done_mb: u64,
        total_mb: u64,
    },
    Unpacking {
        label: String,
    },
    Virtualization {
        message: String,
    },
    Failed {
        message: String,
    },
    Done,
}

#[derive(Debug, Clone, Serialize)]
pub struct InitPayload {
    pub setup: Option<SetupPayload>,
    pub state: StatePayload,
    pub apps: Vec<AppInfo>,
    pub screen: Option<ScreenPayload>,
}

struct PendingMore {
    container: u64,
    forward: bool,
    before: HashSet<u64>,
    req: Option<u64>,
    snapshots: u8,
}

pub struct Core {
    app: AppHandle,
    events: Sender<BackendEvent>,
    opts: Options,
    bridge: Bridge,
    device: Option<Device>,
    connected: bool,
    status: String,
    mode: Mode,

    model: Model,
    home_package: String,
    apps: Vec<AppInfo>,
    launching: Option<(String, Instant, Option<u64>)>,
    screen_key: String,
    last_input_focus: Option<u64>,
    install_watch: Option<(String, Instant)>,
    pending_more: Option<PendingMore>,
    last_screen: Option<ScreenPayload>,
    setup: Option<SetupPayload>,
    license_reply: Option<Sender<bool>>,
}

impl Core {
    pub fn new(app: AppHandle, events: Sender<BackendEvent>, opts: Options) -> Self {
        Self {
            app,
            events,
            opts,
            bridge: Bridge::default(),
            device: None,
            connected: false,
            status: "Starting Android".into(),
            mode: Mode::Starting,
            model: Model::default(),
            home_package: String::new(),
            apps: Vec::new(),
            launching: None,
            screen_key: String::new(),
            last_input_focus: None,
            install_watch: None,
            pending_more: None,
            last_screen: None,
            setup: None,
            license_reply: None,
        }
    }

    /// Starts the emulator (or loads a mock screen) in the background.
    pub fn start(&mut self) {
        if let Some(path) = self.opts.mock.clone() {
            let loaded = std::fs::read_to_string(&path)
                .map_err(anyhow::Error::from)
                .and_then(|s| Ok(serde_json::from_str::<Snapshot>(&s)?));
            match loaded {
                Ok(snap) => {
                    self.connected = true;
                    self.set_status("Mock screen loaded");
                    self.on_snapshot(snap);
                }
                Err(e) => self.set_status(&format!("Could not load mock screen: {e}")),
            }
            return;
        }
        let events = self.events.clone();
        let show = self.opts.show_emulator;
        let set_location = !self.opts.no_location;
        std::thread::spawn(move || {
            let status = |s: &str| {
                let _ = events.send(BackendEvent::Status(s.to_string()));
            };
            let sdk = match device::find_sdk() {
                Some(sdk) => Ok(sdk),
                None => run_setup(&events),
            };
            let result = sdk
                .and_then(|sdk| {
                    device::start(
                        sdk,
                        &device::StartOptions {
                            show_emulator: show,
                        },
                        &status,
                    )
                })
                .and_then(|d| device::ensure_bridge(&d, &status).map(|_| d));
            // Give apps the PC's position: the emulator's default GPS fix is
            // an arbitrary spot in California.
            if let (Ok(d), true) = (&result, set_location) {
                match device::pc_location().and_then(|loc| d.set_location(&loc).map(|_| loc)) {
                    Ok(loc) => status(&format!("Location set to {}", loc.place)),
                    Err(e) => eprintln!("could not set the location: {e:#}"),
                }
            }
            let _ = events.send(match result {
                Ok(d) => BackendEvent::DeviceReady(d),
                Err(e) => BackendEvent::DeviceFailed(format!("{e:#}")),
            });
        });
    }

    pub fn init_payload(&self) -> InitPayload {
        InitPayload {
            setup: self.setup.clone(),
            state: self.state(),
            apps: self.apps.clone(),
            screen: self.last_screen.clone(),
        }
    }

    fn state(&self) -> StatePayload {
        StatePayload {
            connected: self.connected,
            status: self.status.clone(),
            mode: self.mode,
        }
    }

    fn emit_state(&self) {
        let _ = self.app.emit("state", self.state());
    }

    fn set_status(&mut self, status: &str) {
        self.status = status.to_string();
        self.emit_state();
        self.announce(status);
    }

    fn announce(&self, text: &str) {
        let _ = self.app.emit("announce", text);
    }

    fn set_mode(&mut self, mode: Mode) {
        if self.mode != mode {
            self.mode = mode;
            self.emit_state();
            if mode == Mode::Apps {
                self.send(ToBridge::Apps);
            }
        }
    }

    fn send(&self, cmd: ToBridge) -> Option<u64> {
        let nodes = &self.model.nodes;
        self.bridge.send(cmd, &|id| {
            nodes
                .get(&id)
                .map_or(id & !ANDROID_ID_BIT, |n| n.android_id)
        })
    }

    // ------------------------------------------------------------ events

    pub fn handle(&mut self, ev: BackendEvent) {
        match ev {
            BackendEvent::Setup(payload) => {
                let _ = self.app.emit("setup", &payload);
                self.setup = Some(payload);
            }
            BackendEvent::LicenseRequest {
                text,
                download_bytes,
                reply,
            } => {
                self.license_reply = Some(reply);
                let payload = SetupPayload::License {
                    text,
                    download_mb: download_bytes / 1_000_000,
                };
                let _ = self.app.emit("setup", &payload);
                self.setup = Some(payload);
            }
            BackendEvent::Status(s) => self.set_status(&s),
            BackendEvent::DeviceReady(d) => {
                self.device = Some(d);
                self.set_status("Android is ready, connecting");
                self.bridge.spawn(self.events.clone());
            }
            BackendEvent::DeviceFailed(e) => {
                self.set_status(&format!("Could not start Android: {e}"))
            }
            BackendEvent::BridgeDisconnected => {
                if self.connected {
                    self.connected = false;
                    self.set_status("Lost connection to Android, reconnecting");
                }
            }
            BackendEvent::Bridge(msg) => self.on_bridge(msg),
        }
    }

    fn on_bridge(&mut self, msg: FromBridge) {
        match msg {
            FromBridge::Hello { home, device, sdk } => {
                self.connected = true;
                self.home_package = home;
                self.status = format!("Connected to {device}, Android API {sdk}");
                self.emit_state();
                if let Some(link) = self.opts.install.take() {
                    self.install_link(&link);
                }
            }
            FromBridge::Apps { apps } => {
                self.apps = apps;
                let _ = self.app.emit("apps", &self.apps);
            }
            FromBridge::Tree(snap) => self.on_snapshot(snap),
            FromBridge::Announce { text } => self.announce(&text),
            FromBridge::WindowChanged { .. } => {}
            FromBridge::Result { req, ok, error } => {
                if ok {
                    return;
                }
                if self
                    .pending_more
                    .as_ref()
                    .is_some_and(|p| p.req == Some(req))
                {
                    self.pending_more = None;
                    self.announce("No more items");
                } else if let Some((pkg, _, Some(r))) = &self.launching
                    && *r == req
                {
                    // The bridge couldn't start it; fall back to adb.
                    let (pkg, device) = (pkg.clone(), self.device.clone());
                    std::thread::spawn(move || {
                        if let Some(d) = device {
                            let _ = d.launch(&pkg);
                        }
                    });
                } else {
                    eprintln!("bridge request {req} failed: {}", error.unwrap_or_default());
                }
            }
        }
    }

    fn on_snapshot(&mut self, snap: Snapshot) {
        if crate::debug_enabled()
            && let Ok(json) = serde_json::to_string(&snap)
        {
            let _ = std::fs::write(std::env::temp_dir().join("dromaius-snapshot.json"), json);
        }
        self.model = Model::from_snapshot(&snap);
        let Some(nav) = self.model.nav_window() else {
            return;
        };
        let (package, title, nav_id) = (nav.package.clone(), nav.title.clone(), nav.android_id);
        let input_focus = nav.order.iter().copied().find(|id| {
            let n = &self.model.nodes[id];
            n.editable && n.input_focused
        });

        // Android's own home screen is replaced by our app list.
        let launching = self
            .launching
            .as_ref()
            .is_some_and(|(_, t, _)| t.elapsed() < Duration::from_secs(4));
        if !launching {
            self.launching = None;
        }
        if !self.home_package.is_empty() && package == self.home_package {
            if !launching {
                self.set_mode(Mode::Apps);
            }
            return;
        }
        if package != self.home_package {
            self.launching = None;
        }
        self.set_mode(Mode::App);

        let key = format!("{nav_id}|{title}");
        let new_screen = key != self.screen_key;
        self.screen_key = key;

        let mut focus = None;

        // Follow Android when an edit field gains input focus (e.g. a search screen).
        if input_focus != self.last_input_focus {
            self.last_input_focus = input_focus;
            focus = input_focus;
        }

        if let Some(id) = self.check_pending_more() {
            focus = Some(id);
        }
        if let Some(id) = self.check_install_watch() {
            focus = Some(id);
        }

        let screen = ScreenPayload {
            title: if title.is_empty() {
                package.clone()
            } else {
                title
            },
            package,
            nodes: view::screen_nodes(&self.model),
            new_screen,
            focus: focus.map(view::id_string),
        };
        let _ = self.app.emit("screen", &screen);
        self.last_screen = Some(ScreenPayload {
            new_screen: false,
            focus: None,
            ..screen
        });
    }

    /// Stops inside a container, in reading order.
    fn stops_in(&self, container: u64) -> Vec<u64> {
        fn walk(model: &Model, id: u64, out: &mut Vec<u64>) {
            let Some(n) = model.nodes.get(&id) else {
                return;
            };
            if n.is_stop {
                out.push(id);
            }
            for c in &n.children {
                walk(model, *c, out);
            }
        }
        let mut out = Vec::new();
        if let Some(n) = self.model.nodes.get(&container) {
            for c in &n.children {
                walk(&self.model, *c, &mut out);
            }
        }
        out
    }

    /// After "Show more items", focus the first newly loaded item.
    fn check_pending_more(&mut self) -> Option<u64> {
        let p = self.pending_more.as_mut()?;
        p.snapshots += 1;
        let (container, forward) = (p.container, p.forward);
        let stops = self.stops_in(container);
        let p = self.pending_more.as_ref()?;
        let mut fresh = stops.iter().copied().filter(|id| !p.before.contains(id));
        let target = if forward {
            fresh.next()
        } else {
            fresh.next_back()
        };
        if target.is_some() || p.snapshots > 6 {
            if target.is_none() {
                self.announce("No more items");
            }
            self.pending_more = None;
        }
        target
    }

    fn check_install_watch(&mut self) -> Option<u64> {
        let (package, started) = self.install_watch.as_ref()?;
        if started.elapsed() > Duration::from_secs(30) {
            self.install_watch = None;
            return None;
        }
        let package = package.clone();
        let w = self.model.nav_window()?;
        if w.package != "com.android.vending" {
            return None;
        }
        let (id, label) = w.order.iter().find_map(|id| {
            let n = &self.model.nodes[id];
            let l = n.label.to_lowercase();
            ["install", "update", "open", "play"]
                .contains(&l.as_str())
                .then(|| (*id, n.label.clone()))
        })?;
        self.install_watch = None;
        self.announce(&if label.eq_ignore_ascii_case("install") {
            format!("{package} is ready to install. Press Enter on Install.")
        } else {
            format!("{package}: {label} button.")
        });
        Some(id)
    }

    // ------------------------------------------------------------ commands

    pub fn act(&mut self, id: u64, action: &str) {
        let known = [
            "click",
            "longClick",
            "focus",
            "imeEnter",
            "expand",
            "collapse",
            "dismiss",
            "showOnScreen",
        ];
        if known.contains(&action) && self.send(ToBridge::Action { id, action }).is_none() {
            self.announce("Not connected to Android");
        }
    }

    pub fn set_text(&mut self, id: u64, text: &str, start: usize, end: usize) {
        self.send(ToBridge::SetText {
            id,
            text,
            sel: Some((start, end)),
        });
    }

    pub fn set_progress(&mut self, id: u64, value: f64) {
        self.send(ToBridge::SetProgress { id, value });
    }

    pub fn custom_action(&mut self, id: u64, action_id: i64) {
        self.send(ToBridge::Custom { id, action_id });
    }

    pub fn global(&mut self, action: &str) {
        if self.send(ToBridge::Global(action)).is_none() {
            self.announce("Not connected to Android");
        }
    }

    pub fn scroll(&mut self, container: u64, forward: bool) {
        let before = self.stops_in(container).into_iter().collect();
        let req = self.send(ToBridge::Action {
            id: container,
            action: if forward {
                "scrollForward"
            } else {
                "scrollBackward"
            },
        });
        self.pending_more = Some(PendingMore {
            container,
            forward,
            before,
            req,
            snapshots: 0,
        });
    }

    pub fn refresh(&mut self) {
        self.send(ToBridge::Refresh);
        self.send(ToBridge::Apps);
    }

    pub fn show_apps(&mut self) {
        self.launching = None;
        self.global("home");
        self.set_mode(Mode::Apps);
        self.send(ToBridge::Apps);
    }

    pub fn launch(&mut self, package: &str) {
        let label = self
            .apps
            .iter()
            .find(|a| a.package == package)
            .map_or(package, |a| a.label.as_str());
        self.announce(&format!("Opening {label}"));
        let req = self.send(ToBridge::Launch(package));
        self.launching = Some((package.to_string(), Instant::now(), req));
        self.screen_key.clear();
        self.set_mode(Mode::App);
    }

    /// Opens Google Play for a link or package name, or searches for anything else.
    pub fn install_link(&mut self, input: &str) {
        let input = input.trim();
        if input.is_empty() {
            return;
        }
        let Some(device) = self.device.clone() else {
            return self.announce("Android is not running yet");
        };
        let package = device::package_from_link(input);
        match &package {
            Some(p) => {
                self.announce(&format!("Opening Google Play for {p}"));
                self.install_watch = Some((p.clone(), Instant::now()));
            }
            None => self.announce(&format!("Searching Google Play for {input}")),
        }
        self.launching = Some(("com.android.vending".into(), Instant::now(), None));
        self.set_mode(Mode::App);
        let events = self.events.clone();
        let input = input.to_string();
        std::thread::spawn(move || {
            let result = match package {
                Some(p) => device.open_play_page(&p),
                None => device.open_play_search(&input),
            };
            if let Err(e) = result {
                let _ = events.send(BackendEvent::Status(format!(
                    "Could not open Google Play: {e}"
                )));
            }
        });
    }

    /// The user's answer to Google's licence during first-run setup.
    pub fn answer_license(&mut self, accepted: bool) {
        if let Some(reply) = self.license_reply.take() {
            let _ = reply.send(accepted);
        }
    }

    /// Called when the window closes: pause (or shut down) the emulator.
    pub fn on_exit(&self) {
        if let Some(d) = &self.device {
            let result = if self.opts.shutdown_on_exit {
                d.shutdown()
            } else {
                d.pause()
            };
            if let Err(e) = result {
                eprintln!("could not stop the emulator: {e:#}");
            }
        }
    }
}

/// Downloads Android into Dromaius's own folder. Runs on the startup thread.
fn run_setup(events: &Sender<BackendEvent>) -> anyhow::Result<std::path::PathBuf> {
    use crate::setup;

    let send = |p: SetupPayload| {
        let _ = events.send(BackendEvent::Setup(p));
    };
    let fail = |message: String| {
        send(SetupPayload::Failed {
            message: message.clone(),
        });
        anyhow::anyhow!(message)
    };
    send(SetupPayload::Checking);
    let sdk = setup::own_sdk_dir();
    let plan = setup::plan(&sdk)
        .map_err(|e| fail(format!("Could not reach Google's download server: {e:#}")))?;

    let (reply, answer) = std::sync::mpsc::channel();
    let _ = events.send(BackendEvent::LicenseRequest {
        text: plan.license_text.clone(),
        download_bytes: plan.total_bytes(),
        reply,
    });
    if !answer.recv().unwrap_or(false) {
        return Err(fail(
            "Android can't be installed without accepting Google's licence. Close and reopen Dromaius to see it again.".into(),
        ));
    }
    setup::record_license(&sdk, &plan)?;

    let total = plan.total_bytes().max(1);
    let mut before = 0;
    for d in &plan.downloads {
        // Check virtualization before the big system image download.
        if d.dest.starts_with(sdk.join("system-images"))
            && let Err(detail) = setup::check_acceleration(&sdk)
        {
            send(SetupPayload::Virtualization {
                message: virtualization_help(&detail),
            });
            anyhow::bail!("virtualization is not available: {detail}");
        }
        setup::install(&sdk, d, before, total, &|p| {
            send(match p {
                setup::Progress::Downloading { label, done, total } => SetupPayload::Downloading {
                    label: label.into(),
                    percent: (done * 100 / total.max(1)).min(100) as u8,
                    done_mb: done / 1_000_000,
                    total_mb: total / 1_000_000,
                },
                setup::Progress::Unpacking { label } => SetupPayload::Unpacking {
                    label: label.into(),
                },
            })
        })
        .map_err(|e| fail(format!("{e:#}")))?;
        before += d.size;
    }
    send(SetupPayload::Done);
    Ok(sdk)
}

fn virtualization_help(detail: &str) -> String {
    let fix = if cfg!(windows) {
        "Android needs hardware virtualization. To turn it on: press Windows+R, type optionalfeatures and press Enter, check \"Windows Hypervisor Platform\", press OK, and restart your PC. If it still doesn't work, virtualization (Intel VT-x or AMD-V) may be turned off in your PC's BIOS or UEFI settings."
    } else {
        "Android needs hardware virtualization, which this computer doesn't seem to provide."
    };
    format!("{fix} Then open Dromaius again. (Emulator said: {detail})")
}
