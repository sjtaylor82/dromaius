//! Messages exchanged with the Android bridge service.
//!
//! Transport: newline-delimited JSON over `adb forward tcp:PORT localabstract:dromaius_bridge`.
//! See `docs/protocol.md` for the full description.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const BRIDGE_PORT: u16 = 38300;
pub const BRIDGE_SOCKET: &str = "dromaius_bridge";

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum FromBridge {
    Hello {
        sdk: u32,
        #[serde(default)]
        device: String,
        /// Package of Android's home screen (launcher).
        #[serde(default)]
        home: String,
    },
    Tree(Snapshot),
    Apps {
        apps: Vec<AppInfo>,
    },
    Announce {
        text: String,
    },
    WindowChanged {},
    Result {
        req: u64,
        ok: bool,
        #[serde(default)]
        error: Option<String>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppInfo {
    pub package: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Snapshot {
    pub width: f64,
    pub height: f64,
    pub windows: Vec<AWindow>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AWindow {
    pub id: i64,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub package: String,
    #[serde(default)]
    pub layer: i32,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub root: Option<ANode>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ANode {
    pub id: u64,
    pub cls: String,
    pub text: Option<String>,
    pub desc: Option<String>,
    pub hint: Option<String>,
    pub state: Option<String>,
    pub pane: Option<String>,
    pub tooltip: Option<String>,
    pub error: Option<String>,
    pub view_id: Option<String>,
    pub role_desc: Option<String>,
    pub bounds: [f64; 4],
    /// Android reports the node as not visible to the user.
    pub invisible: bool,
    pub clickable: bool,
    pub long_clickable: bool,
    pub focusable: bool,
    pub focused: bool,
    pub a11y_focused: bool,
    pub checkable: bool,
    pub checked: bool,
    pub editable: bool,
    pub password: bool,
    pub scrollable: bool,
    pub selected: bool,
    pub disabled: bool,
    pub heading: bool,
    pub multi_line: bool,
    pub showing_hint: bool,
    pub sel_start: Option<i32>,
    pub sel_end: Option<i32>,
    pub actions: Vec<i64>,
    pub custom_actions: Vec<CustomAction>,
    pub collection: Option<Collection>,
    pub item: Option<CollectionItem>,
    pub range: Option<Range>,
    pub live: i32,
    pub children: Vec<ANode>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CustomAction {
    pub id: i64,
    pub label: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct Collection {
    pub rows: i32,
    pub cols: i32,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct CollectionItem {
    pub row: i32,
    pub col: i32,
    #[serde(default)]
    pub heading: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct Range {
    pub kind: i32,
    pub min: f64,
    pub max: f64,
    pub current: f64,
}

/// Android `AccessibilityNodeInfo` action ids we look for in `ANode::actions`.
pub mod android_action {
    pub const SCROLL_FORWARD: i64 = 0x1000;
    pub const SCROLL_BACKWARD: i64 = 0x2000;
    pub const EXPAND: i64 = 0x40000;
    pub const COLLAPSE: i64 = 0x80000;
    pub const SET_PROGRESS: i64 = 0x0102003d;
}

/// Commands sent to the bridge.
pub enum ToBridge<'a> {
    Refresh,
    Apps,
    Launch(&'a str),
    Action {
        id: u64,
        action: &'a str,
    },
    SetText {
        id: u64,
        text: &'a str,
        sel: Option<(usize, usize)>,
    },
    SetProgress {
        id: u64,
        value: f64,
    },
    Custom {
        id: u64,
        action_id: i64,
    },
    Global(&'a str),
}

impl ToBridge<'_> {
    /// `aid` maps a desktop node id to the id the bridge knows.
    pub fn to_json(&self, req: u64, aid: &dyn Fn(u64) -> u64) -> Value {
        match self {
            ToBridge::Refresh => json!({"type": "refresh", "req": req}),
            ToBridge::Apps => json!({"type": "apps", "req": req}),
            ToBridge::Launch(package) => json!({"type": "launch", "req": req, "package": package}),
            ToBridge::Action { id, action } => {
                json!({"type": "action", "req": req, "id": aid(*id), "action": action})
            }
            ToBridge::SetText { id, text, sel } => {
                let mut v = json!({"type": "action", "req": req, "id": aid(*id), "action": "setText", "text": text});
                if let Some((s, e)) = sel {
                    v["selStart"] = json!(s);
                    v["selEnd"] = json!(e);
                }
                v
            }
            ToBridge::SetProgress { id, value } => json!({
                "type": "action", "req": req, "id": aid(*id), "action": "setProgress", "value": value
            }),
            ToBridge::Custom { id, action_id } => json!({
                "type": "action", "req": req, "id": aid(*id), "action": "custom", "actionId": action_id
            }),
            ToBridge::Global(action) => json!({"type": "global", "req": req, "action": action}),
        }
    }
}
