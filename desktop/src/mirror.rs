//! Turns an Android accessibility snapshot into the model we expose to the
//! desktop screen reader.
//!
//! Android trees are deep and full of layout containers. Like TalkBack, we
//! flatten them into "stops": actionable nodes (buttons, fields, switches…)
//! whose label merges the text of their non-actionable descendants, plus
//! stand-alone text outside any actionable node. Collections (lists, grids)
//! are kept as containers so the screen reader can report "3 of 12".

use std::collections::{HashMap, HashSet};

use crate::protocol::{ANode, CustomAction, Range, Snapshot, android_action};

/// What a mirrored element is, independent of how the UI presents it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Button,
    CheckBox,
    Switch,
    RadioButton,
    Tab,
    ComboBox,
    TextInput,
    PasswordInput,
    MultilineTextInput,
    Slider,
    ProgressIndicator,
    Heading,
    Label,
    Image,
    ListItem,
    List,
    Grid,
    /// A scrollable area that isn't a list.
    Group,
}

/// Marks ids that come from Android so they never collide with our own nodes.
pub const ANDROID_ID_BIT: u64 = 1 << 63;

#[derive(Debug, Clone)]
pub struct MNode {
    pub id: u64,
    /// Id understood by the bridge (without the marker bit).
    pub android_id: u64,
    pub is_stop: bool,
    pub role: Role,
    pub label: String,
    pub role_desc: Option<String>,
    pub state_desc: Option<String>,
    pub description: Option<String>,
    pub placeholder: Option<String>,
    pub error: Option<String>,
    /// Real field contents for edit fields (empty when Android shows the hint).
    pub text: String,
    pub clickable: bool,
    pub long_clickable: bool,
    pub editable: bool,
    pub password: bool,
    pub multi_line: bool,
    pub checkable: bool,
    pub checked: bool,
    pub selected: bool,
    pub disabled: bool,
    pub expanded: Option<bool>,
    pub input_focused: bool,
    pub range: Option<Range>,
    pub position: Option<(usize, usize)>,
    /// (row, column) within a grid, 0-based.
    pub cell: Option<(i32, i32)>,
    /// (rows, columns) of a grid container.
    pub grid_size: Option<(i32, i32)>,
    pub custom_actions: Vec<CustomAction>,
    pub can_scroll_forward: bool,
    pub can_scroll_backward: bool,
    pub children: Vec<u64>,
}

#[derive(Debug, Clone)]
pub struct WinModel {
    pub android_id: i64,
    pub kind: String,
    pub title: String,
    pub package: String,
    pub active: bool,
    pub focused: bool,
    pub children: Vec<u64>,
    /// Navigation stops in reading order.
    pub order: Vec<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct Model {
    pub windows: Vec<WinModel>,
    pub nodes: HashMap<u64, MNode>,
}

impl Model {
    pub fn from_snapshot(snap: &Snapshot) -> Self {
        let mut b = Builder {
            nodes: HashMap::new(),
            used: HashSet::new(),
        };
        let mut windows = Vec::new();
        let mut sorted: Vec<_> = snap.windows.iter().collect();
        // Topmost first, which is also what a sighted user sees first.
        sorted.sort_by_key(|w| std::cmp::Reverse(w.layer));
        for w in sorted {
            let Some(root) = &w.root else { continue };
            let mut wm = WinModel {
                android_id: w.id,
                kind: w.kind.clone(),
                title: w.title.clone(),
                package: w.package.clone(),
                active: w.active,
                focused: w.focused,
                children: Vec::new(),
                order: Vec::new(),
            };
            let ctx = Ctx {
                inside_stop: false,
                collection: None,
            };
            b.walk(root, &ctx, &mut wm.children, &mut wm.order);
            if wm.title.is_empty() {
                wm.title = root.pane.clone().unwrap_or_else(|| w.package.clone());
            }
            windows.push(wm);
        }
        Model {
            windows,
            nodes: b.nodes,
        }
    }

    /// The window keyboard navigation happens in: the focused one (an app,
    /// a dialog, the notification shade…), never the keyboard or overlays.
    pub fn nav_window(&self) -> Option<&WinModel> {
        let usable = |w: &&WinModel| {
            w.kind != "inputMethod" && w.kind != "accessibilityOverlay" && !w.order.is_empty()
        };
        self.windows
            .iter()
            .filter(usable)
            .find(|w| w.focused)
            .or_else(|| self.windows.iter().filter(usable).find(|w| w.active))
            .or_else(|| {
                self.windows
                    .iter()
                    .filter(usable)
                    .find(|w| w.kind == "application")
            })
    }
}

#[derive(Clone)]
struct Ctx {
    inside_stop: bool,
    collection: Option<(i32, i32)>,
}

struct Builder {
    nodes: HashMap<u64, MNode>,
    used: HashSet<u64>,
}

impl Builder {
    fn unique_id(&mut self, android_id: u64) -> u64 {
        let mut id = android_id | ANDROID_ID_BIT;
        while !self.used.insert(id) {
            // Hash collisions are rare but would corrupt the tree; derive a new id.
            id = (id.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 2) | ANDROID_ID_BIT;
        }
        id
    }

    fn walk(&mut self, n: &ANode, ctx: &Ctx, out: &mut Vec<u64>, order: &mut Vec<u64>) {
        // Like TalkBack, skip what Android says the user cannot see.
        if n.invisible {
            return;
        }
        let own = own_text(n);
        let actionable = is_stop_action(n);
        let is_item = n.item.is_some() && ctx.collection.is_some() && !ctx.inside_stop;
        let make_stop = actionable
            || (!ctx.inside_stop && (own.is_some() || (is_item && !merged_text(n).is_empty())));

        let id = self.unique_id(n.id);
        let mut child_ctx = ctx.clone();

        if make_stop {
            let node = self.make_node(n, id, ctx, actionable, own);
            out.push(id);
            order.push(id);
            self.nodes.insert(id, node);
            // Actionable stops absorb their text (it became their label). A
            // plain text node only does if its own text already covers its
            // children; otherwise (e.g. a WebView titled with the page name)
            // the children are content in their own right.
            child_ctx.inside_stop = actionable || covers_children(n);
            for c in &n.children {
                self.walk(c, &child_ctx, out, order);
            }
        } else if n.collection.is_some() || n.scrollable {
            if let Some(col) = n.collection {
                child_ctx.collection = Some((col.rows, col.cols));
            }
            let mut kids = Vec::new();
            for c in &n.children {
                self.walk(c, &child_ctx, &mut kids, order);
            }
            if !kids.is_empty() {
                let mut node = self.make_node(n, id, ctx, false, None);
                node.is_stop = false;
                node.grid_size = n.collection.map(|c| (c.rows, c.cols));
                node.role = match n.collection {
                    Some(col) if col.cols > 1 && col.rows > 1 => Role::Grid,
                    Some(_) => Role::List,
                    None => Role::Group,
                };
                node.label = n
                    .pane
                    .clone()
                    .or_else(|| n.desc.clone())
                    .unwrap_or_default();
                node.children = kids;
                out.push(id);
                self.nodes.insert(id, node);
            }
        } else {
            for c in &n.children {
                self.walk(c, &child_ctx, out, order);
            }
        }
    }

    fn make_node(
        &self,
        n: &ANode,
        id: u64,
        ctx: &Ctx,
        actionable: bool,
        own: Option<String>,
    ) -> MNode {
        let mut label = if n.editable {
            // A field's text is its value; its name comes from the description or hint.
            n.desc
                .clone()
                .or_else(|| n.hint.clone())
                .or_else(|| n.tooltip.clone())
                .unwrap_or_default()
        } else {
            match own {
                Some(t) => t,
                None => merged_text(n),
            }
        };
        if label.is_empty() && actionable {
            label = n
                .tooltip
                .clone()
                .or_else(|| {
                    n.view_id
                        .as_deref()
                        .filter(|_| control_like(n))
                        .and_then(readable_view_id)
                })
                .unwrap_or_default();
        }
        let text = if n.editable && !n.showing_hint {
            n.text.clone().unwrap_or_default()
        } else {
            String::new()
        };
        let expanded = if n.actions.contains(&android_action::EXPAND) {
            Some(false)
        } else if n.actions.contains(&android_action::COLLAPSE) {
            Some(true)
        } else {
            None
        };
        let position = match (n.item, ctx.collection) {
            (Some(item), Some((rows, cols))) => {
                let index = if cols > 1 && rows <= 1 {
                    item.col
                } else if cols <= 1 {
                    item.row
                } else {
                    item.row * cols + item.col
                };
                let size = if rows > 0 && cols > 0 {
                    (rows * cols) as usize
                } else {
                    0
                };
                (index >= 0 && size > 0).then(|| (index as usize + 1, size))
            }
            _ => None,
        };
        MNode {
            id,
            android_id: n.id,
            is_stop: true,
            role: role_for(n, ctx),
            label: label.trim().to_string(),
            role_desc: n.role_desc.clone(),
            state_desc: if n.checkable { None } else { n.state.clone() },
            description: if n.editable { None } else { n.hint.clone() },
            placeholder: if n.editable { n.hint.clone() } else { None },
            error: n.error.clone(),
            text,
            clickable: n.clickable,
            long_clickable: n.long_clickable,
            editable: n.editable,
            password: n.password,
            multi_line: n.multi_line,
            checkable: n.checkable,
            checked: n.checked,
            selected: n.selected,
            disabled: n.disabled,
            expanded,
            input_focused: n.focused,
            range: n.range,
            position,
            cell: match (n.item, ctx.collection) {
                (Some(item), Some(_)) => Some((item.row, item.col)),
                _ => None,
            },
            grid_size: None,
            custom_actions: n.custom_actions.clone(),
            can_scroll_forward: n.actions.contains(&android_action::SCROLL_FORWARD),
            can_scroll_backward: n.actions.contains(&android_action::SCROLL_BACKWARD),
            children: Vec::new(),
        }
    }
}

fn is_actionable(n: &ANode) -> bool {
    n.clickable
        || n.long_clickable
        || n.editable
        || n.checkable
        || (n.range.is_some() && n.actions.contains(&android_action::SET_PROGRESS))
}

/// Actionable and worth stopping on (not just a wrapper around other controls).
fn is_stop_action(n: &ANode) -> bool {
    is_actionable(n) && !is_wrapper(n, own_text(n).is_some())
}

/// The text, unless it's empty or only a visual separator such as "•".
fn non_empty(s: &Option<String>) -> Option<String> {
    s.as_ref()
        .map(|s| s.trim())
        .filter(|s| {
            s.chars()
                .any(|c| !c.is_whitespace() && !"•·|-–—/\\,.:;".contains(c))
        })
        .map(str::to_string)
}

/// Classes that are controls in their own right: their internal id may be
/// the only hint of what they do (e.g. an unlabelled icon button).
fn control_like(n: &ANode) -> bool {
    let cls = n.cls.rsplit('.').next().unwrap_or("");
    cls.ends_with("Button")
        || cls.contains("Image")
        || cls.contains("Switch")
        || cls.ends_with("CheckBox")
}

/// Text that belongs to the node itself (content description wins, like on Android).
fn own_text(n: &ANode) -> Option<String> {
    if n.editable {
        return None;
    }
    non_empty(&n.desc).or_else(|| non_empty(&n.text))
}

/// Text of all non-actionable descendants, joined like TalkBack reads them.
fn merged_text(n: &ANode) -> String {
    let mut parts = Vec::new();
    collect_text(n, &mut parts);
    parts.join(", ")
}

fn collect_text(n: &ANode, parts: &mut Vec<String>) {
    for c in &n.children {
        if c.invisible || is_stop_action(c) {
            continue;
        }
        if let Some(d) = non_empty(&c.desc) {
            parts.push(d);
            continue;
        }
        if let Some(t) = non_empty(&c.text) {
            parts.push(t);
        }
        if let Some(s) = non_empty(&c.state) {
            parts.push(s);
        }
        collect_text(c, parts);
    }
}

fn role_for(n: &ANode, ctx: &Ctx) -> Role {
    let cls = n.cls.rsplit('.').next().unwrap_or("");
    if n.editable {
        return if n.password {
            Role::PasswordInput
        } else if n.multi_line {
            Role::MultilineTextInput
        } else {
            Role::TextInput
        };
    }
    if cls.contains("Switch") {
        return Role::Switch;
    }
    if cls.ends_with("RadioButton") {
        return Role::RadioButton;
    }
    if cls.ends_with("CheckBox") || cls.ends_with("CheckedTextView") || n.checkable {
        return Role::CheckBox;
    }
    if let Some(r) = n.range {
        if is_actionable(n) || cls.ends_with("SeekBar") {
            return Role::Slider;
        }
        if r.kind != 0 || cls.ends_with("ProgressBar") {
            return Role::ProgressIndicator;
        }
    }
    if cls.ends_with("Spinner") {
        return Role::ComboBox;
    }
    match n
        .role_desc
        .as_deref()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("tab") => return Role::Tab,
        Some("button") => return Role::Button,
        _ => {}
    }
    if n.heading || n.item.is_some_and(|i| i.heading) {
        return Role::Heading;
    }
    if n.item.is_some() && ctx.collection.is_some() {
        return Role::ListItem;
    }
    if n.clickable || n.long_clickable || cls.ends_with("Button") {
        return Role::Button;
    }
    if cls.ends_with("ImageView") || cls.ends_with("Image") {
        return Role::Image;
    }
    Role::Label
}

/// "com.example:id/wifi_toggle_button" -> "wifi toggle button". Ids that
/// don't read as words (e.g. web content's "root_0_0_Kz") give nothing.
fn readable_view_id(id: &str) -> Option<String> {
    let name = id.rsplit('/').next().unwrap_or(id);
    let words: Vec<&str> = name.split(['_', '-']).filter(|w| !w.is_empty()).collect();
    let readable = !words.is_empty()
        && words
            .iter()
            .all(|w| w.chars().all(|c| c.is_ascii_alphabetic()))
        && words.iter().any(|w| w.len() >= 3);
    readable.then(|| words.join(" "))
}

/// Whether the node's own text already includes all of its descendants' text.
fn covers_children(n: &ANode) -> bool {
    let own = own_text(n).unwrap_or_default();
    let mut parts = Vec::new();
    collect_text(n, &mut parts);
    parts.iter().all(|p| own.contains(p.as_str()))
}

fn has_text(n: &ANode) -> bool {
    !n.invisible
        && (non_empty(&n.text).is_some()
            || non_empty(&n.desc).is_some()
            || n.children.iter().any(has_text))
}

fn count_actionable(n: &ANode) -> usize {
    n.children
        .iter()
        .filter(|c| !c.invisible)
        .map(|c| usize::from(is_actionable(c)) + count_actionable(c))
        .sum()
}

/// A clickable node that isn't worth stopping on: a container wrapping
/// several controls (e.g. a whole web page), or an empty generic wrapper
/// around a field. Its contents are still shown individually.
fn is_wrapper(n: &ANode, has_own_text: bool) -> bool {
    if has_own_text || n.editable || n.checkable {
        return false;
    }
    if count_actionable(n) >= 2 {
        return true;
    }
    // A clickable area with no text anywhere that isn't a control itself:
    // e.g. a field's wrapper, or the notification shade's "scrim" backdrop.
    !control_like(n) && !has_text(n) && n.tooltip.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(json: &str) -> Snapshot {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn merges_row_text_into_clickable_item() {
        let s = snapshot(
            r#"{"width":1080,"height":2400,"windows":[{"id":5,"type":"application","layer":1,"focused":true,
            "root":{"id":1,"cls":"android.widget.FrameLayout","bounds":[0,0,1080,2400],"children":[
              {"id":2,"cls":"androidx.recyclerview.widget.RecyclerView","scrollable":true,"collection":{"rows":2,"cols":1},"bounds":[0,0,1080,2400],"children":[
                {"id":3,"cls":"android.widget.LinearLayout","clickable":true,"item":{"row":0,"col":0},"bounds":[0,0,1080,200],"children":[
                  {"id":4,"cls":"android.widget.TextView","text":"Wi-Fi","bounds":[0,0,500,100]},
                  {"id":6,"cls":"android.widget.TextView","text":"Connected","bounds":[0,100,500,200]},
                  {"id":7,"cls":"android.widget.Switch","checkable":true,"checked":true,"clickable":true,"viewId":"android:id/switch_widget","bounds":[900,0,1000,200]}
                ]},
                {"id":8,"cls":"android.widget.EditText","editable":true,"hint":"Search","text":"Search","showingHint":true,"item":{"row":1,"col":0},"bounds":[0,200,1080,400]}
              ]}
            ]}}]}"#,
        );
        let m = Model::from_snapshot(&s);
        let w = m.nav_window().unwrap();
        assert_eq!(w.order.len(), 3);
        let row = &m.nodes[&w.order[0]];
        assert_eq!(row.label, "Wi-Fi, Connected");
        assert_eq!(row.role, Role::ListItem);
        assert_eq!(row.position, Some((1, 2)));
        let sw = &m.nodes[&w.order[1]];
        assert_eq!(sw.role, Role::Switch);
        assert_eq!(sw.label, "switch widget");
        let edit = &m.nodes[&w.order[2]];
        assert_eq!(edit.role, Role::TextInput);
        assert_eq!(edit.label, "Search");
        assert_eq!(edit.text, "");
        assert_eq!(
            w.children.len(),
            1,
            "list container is the only top-level child"
        );
    }

    #[test]
    fn notification_shade_reads_cleanly() {
        let s = snapshot(
            r#"{"width":1080,"height":2400,"windows":[{"id":5,"type":"system","layer":1,"focused":true,
            "root":{"id":1,"cls":"android.widget.FrameLayout","children":[
              {"id":2,"cls":"com.android.systemui.scrim.ScrimView","clickable":true,"viewId":"com.android.systemui:id/scrim_behind"},
              {"id":3,"cls":"android.view.View","clickable":true,"viewId":"com.android.systemui:id/alternate_expand_target"},
              {"id":4,"cls":"android.widget.TextView","text":"Google Play services"},
              {"id":6,"cls":"android.widget.TextView","text":"•"},
              {"id":7,"cls":"android.widget.TextView","text":"1 minute ago"},
              {"id":8,"cls":"android.widget.ImageButton","clickable":true,"viewId":"com.android.systemui:id/expand_button"}
            ]}}]}"#,
        );
        let m = Model::from_snapshot(&s);
        let w = m.nav_window().unwrap();
        let labels: Vec<_> = w.order.iter().map(|id| m.nodes[id].label.clone()).collect();
        assert_eq!(
            labels,
            ["Google Play services", "1 minute ago", "expand button"]
        );
    }

    #[test]
    fn skips_web_wrappers() {
        let s = snapshot(
            r#"{"width":1080,"height":2400,"windows":[{"id":5,"type":"application","layer":1,"focused":true,
            "root":{"id":1,"cls":"android.view.View","clickable":true,"viewId":"root_0_0_Kz","children":[
              {"id":2,"cls":"android.view.View","clickable":true,"children":[]},
              {"id":3,"cls":"android.widget.EditText","editable":true,"hint":"Email","viewId":"m_login_email"},
              {"id":4,"cls":"android.widget.Button","clickable":true,"text":"Log in"},
              {"id":6,"cls":"android.widget.ImageButton","clickable":true}
            ]}}]}"#,
        );
        let m = Model::from_snapshot(&s);
        let w = m.nav_window().unwrap();
        let labels: Vec<_> = w.order.iter().map(|id| m.nodes[id].label.clone()).collect();
        // The page wrapper and the empty field wrapper are gone; a real icon
        // button without a label is kept (shown as "Unlabelled button").
        assert_eq!(labels, ["Email", "Log in", ""]);
        assert_eq!(
            readable_view_id("com.app:id/wifi_toggle").as_deref(),
            Some("wifi toggle")
        );
        assert_eq!(readable_view_id("root_0_0_Kz"), None);
    }
}

#[cfg(test)]
mod snapshot_debug {
    #[test]
    #[ignore]
    fn print_snapshot_file() {
        let path = std::env::var("SNAPSHOT").unwrap();
        let s = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let m = super::Model::from_snapshot(&s);
        let w = m.nav_window().unwrap();
        for id in &w.order {
            let n = &m.nodes[id];
            println!("{:?} {:?}", n.role, n.label);
        }
    }
}
