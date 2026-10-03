//! The JSON the web front end renders: one element per mirrored node.
//!
//! Ids are hex strings because JavaScript numbers can't hold 64-bit ids exactly.

use serde::Serialize;

use crate::mirror::{MNode, Model, Role};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewNode {
    pub id: String,
    pub kind: &'static str,
    pub label: String,
    /// The app's own name for the role (e.g. Compose's "Tab"), when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub selected: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub clickable: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub long_clickable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
    /// (position, set size), 1-based.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pos: Option<(usize, usize)>,
    /// (min, max, current)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<(f64, f64, f64)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub password: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub multiline: bool,
    /// Scrollable containers: whether more content exists after / before.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub more: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub less: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub custom_actions: Vec<(i64, String)>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ViewNode>,
}

pub fn id_string(id: u64) -> String {
    format!("{id:x}")
}

pub fn parse_id(id: &str) -> Option<u64> {
    u64::from_str_radix(id, 16).ok()
}

fn kind(role: Role) -> &'static str {
    match role {
        Role::Button => "button",
        Role::CheckBox => "checkbox",
        Role::Switch => "switch",
        Role::RadioButton => "radio",
        Role::Tab => "tab",
        Role::ComboBox => "combobox",
        Role::TextInput | Role::PasswordInput | Role::MultilineTextInput => "edit",
        Role::Slider => "slider",
        Role::ProgressIndicator => "progress",
        Role::Heading => "heading",
        Role::Label => "text",
        Role::Image => "image",
        Role::ListItem => "listitem",
        Role::List | Role::Grid => "list",
        Role::Group => "group",
    }
}

/// The navigation window (the current app or dialog) as view nodes.
pub fn screen_nodes(model: &Model) -> Vec<ViewNode> {
    let Some(w) = model.nav_window() else {
        return Vec::new();
    };
    w.children
        .iter()
        .filter_map(|id| model.nodes.get(id))
        .map(|n| node(model, n))
        .collect()
}

fn node(model: &Model, m: &MNode) -> ViewNode {
    let mut description = Vec::new();
    if let Some(s) = &m.state_desc {
        description.push(s.clone());
    }
    if let Some(d) = &m.description {
        description.push(d.clone());
    }
    ViewNode {
        id: id_string(m.id),
        kind: kind(m.role),
        label: m.label.clone(),
        role_description: m
            .role_desc
            .clone()
            .filter(|_| !matches!(m.role, Role::Tab | Role::Button)),
        value: m.editable.then(|| m.text.clone()),
        checked: m.checkable.then_some(m.checked),
        selected: m.selected,
        disabled: m.disabled,
        clickable: m.clickable,
        long_clickable: m.long_clickable,
        expanded: m.expanded,
        pos: m.position,
        range: m.range.map(|r| (r.min, r.max, r.current)),
        placeholder: m.placeholder.clone().filter(|p| *p != m.label),
        description: (!description.is_empty()).then(|| description.join(". ")),
        error: m.error.clone(),
        password: m.password,
        multiline: m.multi_line,
        more: !m.is_stop && m.can_scroll_forward,
        less: !m.is_stop && m.can_scroll_backward,
        custom_actions: m
            .custom_actions
            .iter()
            .map(|a| (a.id, a.label.clone()))
            .collect(),
        children: m
            .children
            .iter()
            .filter_map(|id| model.nodes.get(id))
            .map(|c| node(model, c))
            .collect(),
    }
}
