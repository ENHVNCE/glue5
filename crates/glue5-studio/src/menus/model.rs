//! The menus as the game sends them (`menus`, the mods' `ui.menu`): all
//! menus flat, top-level and submenus; the tree is `parent` and a submenu
//! row's `menu`. Indices are 1-based, as in Luau.

use serde::{Deserialize, Deserializer};
use serde_json::Value;

#[derive(Clone, Deserialize, PartialEq, Debug)]
pub(crate) struct MenuInfo {
    #[serde(rename = "mod")]
    pub(crate) module: String,
    pub(crate) id: u32,
    /// The menu whose submenu row opens this one; None: top-level.
    #[serde(default)]
    pub(crate) parent: Option<u32>,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) note: Option<String>,
    #[serde(deserialize_with = "items")]
    pub(crate) items: Vec<Item>,
}

#[derive(Clone, Deserialize, PartialEq, Debug)]
pub(crate) struct Item {
    pub(crate) id: u32,
    #[serde(default)]
    pub(crate) label: String,
    #[serde(default)]
    pub(crate) note: Option<String>,
    #[serde(default = "yes")]
    pub(crate) enabled: bool,
    #[serde(default = "yes")]
    pub(crate) visible: bool,
    #[serde(flatten)]
    pub(crate) kind: Kind,
}

#[derive(Clone, Deserialize, PartialEq, Debug)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum Kind {
    Button {
        #[serde(default)]
        action: Option<String>,
    },
    Toggle {
        #[serde(default)]
        value: bool,
    },
    Slider {
        value: f64,
        min: f64,
        max: f64,
        step: f64,
        #[serde(default)]
        unit: Option<String>,
    },
    Number {
        value: f64,
        min: Option<f64>,
        max: Option<f64>,
        #[serde(default = "one")]
        step: f64,
        #[serde(default)]
        unit: Option<String>,
    },
    Choice {
        value: usize,
        options: Vec<String>,
    },
    Multi {
        #[serde(default)]
        value: Vec<usize>,
        options: Vec<String>,
    },
    Text {
        #[serde(default)]
        value: String,
        #[serde(default)]
        placeholder: Option<String>,
        #[serde(default = "text_max")]
        max: u32,
    },
    Color {
        value: String,
        #[serde(default)]
        alpha: bool,
    },
    Label,
    Separator,
    Progress {
        value: f64,
    },
    Submenu {
        menu: u32,
    },
    /// A kind this page does not know (a newer game): not shown.
    #[serde(other)]
    Other,
}

impl Kind {
    /// Its name, for keying rows: a row is made again only if this changes.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Kind::Button { .. } => "button",
            Kind::Toggle { .. } => "toggle",
            Kind::Slider { .. } => "slider",
            Kind::Number { .. } => "number",
            Kind::Choice { .. } => "choice",
            Kind::Multi { .. } => "multi",
            Kind::Text { .. } => "text",
            Kind::Color { .. } => "color",
            Kind::Label => "label",
            Kind::Separator => "separator",
            Kind::Progress { .. } => "progress",
            Kind::Submenu { .. } => "submenu",
            Kind::Other => "other",
        }
    }
}

fn yes() -> bool {
    true
}

fn one() -> f64 {
    1.0
}

fn text_max() -> u32 {
    64
}

/// The items one by one: one the page cannot read is left out, not the menu.
fn items<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Item>, D::Error> {
    let raw = Vec::<Value>::deserialize(d)?;
    Ok(raw.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect())
}

/// The decimals a step needs to show (0 to 3): 1 → 0, 0.25 → 2.
pub(crate) fn decimals(step: f64) -> usize {
    (0..3).find(|&d| {
        let scaled = step * 10f64.powi(d as i32);
        (scaled - scaled.round()).abs() < 1e-6 * scaled.abs().max(1.0)
    })
    .unwrap_or(3)
}

/// A slider's or number's value as shown: with the step's decimals and the
/// unit after it; `%` shows the value times 100, as the app's sliders do.
pub(crate) fn shown(value: f64, step: f64, unit: Option<&str>) -> String {
    let percent = unit == Some("%");
    let k = if percent { 100.0 } else { 1.0 };
    format!("{:.*}{}", decimals(step * k), value * k, unit.unwrap_or(""))
}
