//! The style overrides of files of version 34 and older. The app applies them once, when it opens such a file.
//!
//! An override is a JSON pointer into the style JSON and the value at that place. A pointer names a rule by its name and not by its index, for example `/rules/Rule 1/symbolizers/0/width`. An override whose place is gone does nothing.

use std::collections::BTreeMap;

use serde_json::Value as Json;

use crate::LayerStyle;

/// The overrides of one layer in one map.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overrides(BTreeMap<String, Json>);

impl Overrides {
    /// Reads a JSON object of pointers and values. Text that is not such an object gives no overrides.
    pub fn from_json(text: &str) -> Overrides {
        match serde_json::from_str::<Json>(text) {
            Ok(Json::Object(m)) => Overrides(m.into_iter().collect()),
            _ => Overrides::default(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl LayerStyle {
    /// This style with the overrides. An override whose place is gone does nothing. If the result is not a style, the result is this style.
    pub fn with_overrides(&self, overrides: &Overrides) -> LayerStyle {
        if overrides.is_empty() {
            return self.clone();
        }
        let mut root = tree(self);
        for (p, v) in &overrides.0 {
            set(&mut root, p, v.clone());
        }
        LayerStyle::from_json(&root.to_string()).unwrap_or_else(|_| self.clone())
    }
}

fn tree(style: &LayerStyle) -> Json {
    serde_json::from_str(&style.to_json()).expect("the style JSON is JSON")
}

fn unescape(token: &str) -> String {
    token.replace("~1", "/").replace("~0", "~")
}

/// The tokens of `pointer`, with the rule name replaced by the index of the rule in `root`.
fn tokens(root: &Json, pointer: &str) -> Option<Vec<String>> {
    let mut t: Vec<String> = pointer
        .strip_prefix('/')?
        .split('/')
        .map(unescape)
        .collect();
    if t.first().map(String::as_str) == Some("rules") {
        let name = t.get(1)?;
        let i = root
            .get("rules")?
            .as_array()?
            .iter()
            .position(|r| r.get("name").and_then(Json::as_str) == Some(name))?;
        t[1] = i.to_string();
    }
    Some(t)
}

fn set(root: &mut Json, pointer: &str, value: Json) {
    let Some(t) = tokens(root, pointer) else {
        return;
    };
    let Some((last, parents)) = t.split_last() else {
        return;
    };
    let mut j = root;
    for token in parents {
        let next = match j {
            Json::Object(m) => m.get_mut(token),
            Json::Array(a) => token.parse::<usize>().ok().and_then(|i| a.get_mut(i)),
            _ => None,
        };
        let Some(next) = next else {
            return;
        };
        j = next;
    }
    match j {
        Json::Object(m) if value.is_null() => {
            m.remove(last);
        }
        Json::Object(m) => {
            m.insert(last.clone(), value);
        }
        Json::Array(a) => {
            if let Some(slot) = last.parse::<usize>().ok().and_then(|i| a.get_mut(i)) {
                *slot = value;
            }
        }
        _ => {}
    }
}
