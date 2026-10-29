//! The style overrides of a map: single properties that replace the properties of a layer style.
//!
//! An override is a JSON pointer into the style JSON and the value at that place. A pointer names a rule by its name and not by its index, for example `/rules/Rule 1/symbolizers/0/width`, so an override stays on its rule when the rules move. An override whose place is gone does nothing.

use std::collections::BTreeMap;

use serde_json::{Map, Value as Json};

use crate::LayerStyle;

/// The overrides of one layer in one map.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overrides(BTreeMap<String, Json>);

impl Overrides {
    /// Reads the JSON object of [`Overrides::to_json`]. Text that is not such an object gives no overrides.
    pub fn from_json(text: &str) -> Overrides {
        match serde_json::from_str::<Json>(text) {
            Ok(Json::Object(m)) => Overrides(m.into_iter().collect()),
            _ => Overrides::default(),
        }
    }

    /// The JSON text. The same overrides give the same text.
    pub fn to_json(&self) -> String {
        Json::Object(self.0.clone().into_iter().collect::<Map<_, _>>()).to_string()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The pointers, in order.
    pub fn pointers(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    /// The overrides that change `base` into `edited`. A rule that `base` does not have gives no override.
    pub fn between(base: &LayerStyle, edited: &LayerStyle) -> Overrides {
        let a = tree(base);
        let b = tree(edited);
        let mut out = BTreeMap::new();
        for (key, bv) in b.as_object().into_iter().flatten() {
            if key == "version" {
                continue;
            }
            let av = a.get(key);
            if key == "rules" {
                let (Some(ar), Some(br)) = (av.and_then(Json::as_array), bv.as_array()) else {
                    continue;
                };
                for rule in br {
                    let name = rule.get("name").and_then(Json::as_str).unwrap_or("");
                    let old = ar
                        .iter()
                        .find(|r| r.get("name").and_then(Json::as_str) == Some(name));
                    if let Some(old) = old {
                        walk(
                            Some(old),
                            Some(rule),
                            &format!("/rules/{}", escape(name)),
                            &mut out,
                        );
                    }
                }
            } else {
                walk(av, Some(bv), &format!("/{}", escape(key)), &mut out);
            }
        }
        Overrides(out)
    }

    /// The overrides after an edit of the style `base` with these overrides into `edited`. Each pointer of `self` that still has a place stays, with its value in `edited`, so an override stays also when its value is the value of `base`.
    pub fn merged(&self, base: &LayerStyle, edited: &LayerStyle) -> Overrides {
        let mut out = Overrides::between(base, edited);
        let b = tree(edited);
        for p in self.0.keys() {
            if out.0.contains_key(p) {
                continue;
            }
            if let Some(v) = value_at(&b, p) {
                out.0.insert(p.clone(), v);
            }
        }
        out
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

/// Adds the leaves where `a` and `b` differ. A property with an expression is one leaf, and so is an array whose length changes or a symbolizer whose type changes. A missing value is `null`.
fn walk(a: Option<&Json>, b: Option<&Json>, path: &str, out: &mut BTreeMap<String, Json>) {
    if a == b {
        return;
    }
    match (a, b) {
        (Some(Json::Object(x)), Some(Json::Object(y)))
            if !x.contains_key("expr")
                && !y.contains_key("expr")
                && x.get("type") == y.get("type") =>
        {
            // The stops and the colors of a raster go together, because a
            // band that is added or removed moves each index after it.
            let raster = y.get("type").and_then(Json::as_str) == Some("raster");
            let bands = ["stops", "colors"];
            if raster && bands.iter().any(|k| x.get(*k) != y.get(*k)) {
                for key in bands {
                    let value = y.get(key).cloned().unwrap_or(Json::Null);
                    out.insert(format!("{path}/{key}"), value);
                }
            }
            for key in x.keys().chain(y.keys().filter(|k| !x.contains_key(*k))) {
                if raster && bands.contains(&key.as_str()) {
                    continue;
                }
                walk(
                    x.get(key),
                    y.get(key),
                    &format!("{path}/{}", escape(key)),
                    out,
                );
            }
        }
        (Some(Json::Array(x)), Some(Json::Array(y))) if x.len() == y.len() => {
            for (i, (u, v)) in x.iter().zip(y).enumerate() {
                walk(Some(u), Some(v), &format!("{path}/{i}"), out);
            }
        }
        _ => {
            out.insert(path.to_string(), b.cloned().unwrap_or(Json::Null));
        }
    }
}

fn escape(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
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

fn child<'a>(j: &'a Json, token: &str) -> Option<&'a Json> {
    match j {
        Json::Object(m) => m.get(token),
        Json::Array(a) => a.get(token.parse::<usize>().ok()?),
        _ => None,
    }
}

/// The value at `pointer`. A missing key of an object that exists is `null`.
fn value_at(root: &Json, pointer: &str) -> Option<Json> {
    let t = tokens(root, pointer)?;
    let (last, parents) = t.split_last()?;
    let mut j = root;
    for token in parents {
        j = child(j, token)?;
    }
    match j {
        Json::Object(m) => Some(m.get(last).cloned().unwrap_or(Json::Null)),
        _ => child(j, last).cloned(),
    }
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
