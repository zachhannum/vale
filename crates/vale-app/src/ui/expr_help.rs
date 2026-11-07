//! The help of an expression field in both layouts: the fields and the
//! functions that the user can insert at the cursor, and the result of the
//! expression for one feature.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use eframe::egui;
use eframe::egui::text::{CCursor, CCursorRange};
use vale_store::{FieldType, GeometryKind, Layer, LayerId, Value};
use vale_style::expr::{Context, Expr, FUNCTIONS, field_ref};

use super::AppState;

/// The result of a filter for the feature of the preview.
pub const MATCHES: &str = "Matches";
pub const NO_MATCH: &str = "Does not match";
/// The result of a label text that gives no text.
pub const NO_LABEL: &str = "No label";
/// The result of a value expression that gives null.
pub const NO_VALUE: &str = "No value";
/// The menu of a desktop expression field, and its submenus.
pub const INSERT: &str = "Insert";
pub const FIELDS: &str = "Fields";
pub const FUNCTIONS_MENU: &str = "Functions";

/// The map scale of the preview.
pub const PREVIEW_SCALE: f64 = 1_000_000.0;

/// What the result of an expression is.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ExprKind {
    /// True or false, for a filter.
    Filter,
    /// The text of a label.
    Text,
    /// A value, for the expression of Classify.
    Value,
}

/// A field or a function that the user can insert.
#[derive(Clone, Debug, PartialEq)]
pub struct Chip {
    /// The name of the field or of the function.
    pub label: String,
    /// The text that goes in the field.
    pub insert: String,
    /// A function call. The insert selects its first argument.
    pub call: bool,
}

/// The line under an expression field.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    /// The result for one feature.
    Preview(String),
    /// The message of the parser.
    Error(String),
}

/// The most text values of a field that the help lists.
pub const MAX_VALUES: usize = 200;

/// The fields of a layer, and the feature of the preview.
#[derive(Clone, Debug)]
pub struct ExprHelp {
    pub fields: Vec<String>,
    /// The type of each field, in the order of `fields`.
    pub types: Vec<FieldType>,
    sample: Option<BTreeMap<String, Value>>,
    /// True if the feature of the preview is a selected feature.
    selected: bool,
    geometry: GeometryKind,
    values: Arc<LayerValues>,
}

/// The values of the features of a layer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerValues {
    pub features: usize,
    /// The distinct values of each text field, sorted, at most
    /// `MAX_VALUES` of them.
    pub texts: BTreeMap<String, Vec<String>>,
}

impl LayerValues {
    pub fn of(layer: &Layer) -> Self {
        let mut texts = BTreeMap::new();
        for field in layer.fields.iter().filter(|f| f.kind == FieldType::Text) {
            let mut set = BTreeSet::new();
            for feature in &layer.features {
                if let Some(Value::Text(s)) = feature.attributes.get(&field.name)
                    && !s.trim().is_empty()
                {
                    set.insert(s.as_str());
                    if set.len() > 4 * MAX_VALUES {
                        break;
                    }
                }
            }
            let list = set.into_iter().take(MAX_VALUES).map(str::to_owned);
            texts.insert(field.name.clone(), list.collect());
        }
        LayerValues {
            features: layer.features.len(),
            texts,
        }
    }
}

/// The count of the features of `layer` that filter `text` matches.
/// `None` if the text does not parse.
pub fn count_matches(layer: &Layer, text: &str) -> Option<usize> {
    if text.trim().is_empty() {
        return Some(layer.features.len());
    }
    let expr = Expr::parse(text).ok()?;
    let geometry = layer.geometry().unwrap_or(GeometryKind::Point);
    let count = (layer.features.iter())
        .filter(|f| {
            let ctx = Context {
                attributes: &f.attributes,
                scale: PREVIEW_SCALE,
                geometry,
            };
            expr.eval(&ctx) == Value::Bool(true)
        })
        .count();
    Some(count)
}

/// What the values and the counts of a layer were computed from: the
/// count of the changes of the maps, and the features and the fields.
#[derive(Clone, Debug, PartialEq)]
struct LayerKey {
    generation: u64,
    features: usize,
    address: usize,
    fields: Vec<(String, FieldType)>,
}

impl LayerKey {
    fn of(layer: &Layer, generation: u64) -> Self {
        LayerKey {
            generation,
            features: layer.features.len(),
            address: layer.features.as_ptr() as usize,
            fields: (layer.fields.iter())
                .map(|f| (f.name.clone(), f.kind))
                .collect(),
        }
    }
}

/// The most match counts that the cache keeps for a layer.
const MAX_COUNTS: usize = 32;

struct CacheEntry {
    key: LayerKey,
    values: Arc<LayerValues>,
    counts: HashMap<String, Option<usize>>,
}

/// The values and the match counts of the layers. A layer computes them
/// again only after it changes, so a field can show them in each frame.
#[derive(Default)]
pub struct HelpCache {
    layers: Mutex<HashMap<LayerId, CacheEntry>>,
    /// The count of the computations, for the tests.
    computed: Mutex<usize>,
}

impl HelpCache {
    fn entry<T>(
        &self,
        layer: &Layer,
        generation: u64,
        f: impl FnOnce(&mut CacheEntry, &Self) -> T,
    ) -> T {
        let key = LayerKey::of(layer, generation);
        let mut layers = self.layers.lock().unwrap_or_else(|e| e.into_inner());
        let fresh = layers.get(&layer.id).is_some_and(|e| e.key == key);
        if !fresh {
            self.count_one();
            let entry = CacheEntry {
                key,
                values: Arc::new(LayerValues::of(layer)),
                counts: HashMap::new(),
            };
            layers.insert(layer.id, entry);
        }
        f(layers.get_mut(&layer.id).expect("inserted"), self)
    }

    fn count_one(&self) {
        *self.computed.lock().unwrap_or_else(|e| e.into_inner()) += 1;
    }

    /// The values of the features of `layer`.
    pub fn values(&self, layer: &Layer, generation: u64) -> Arc<LayerValues> {
        self.entry(layer, generation, |e, _| e.values.clone())
    }

    /// The count of the features of `layer` that filter `text` matches.
    pub fn matches(&self, layer: &Layer, generation: u64, text: &str) -> Option<usize> {
        self.entry(layer, generation, |e, cache| {
            if let Some(count) = e.counts.get(text) {
                return *count;
            }
            cache.count_one();
            if e.counts.len() >= MAX_COUNTS {
                e.counts.clear();
            }
            let count = count_matches(layer, text);
            e.counts.insert(text.to_owned(), count);
            count
        })
    }

    /// The count of the computations of values and counts.
    pub fn computed(&self) -> usize {
        *self.computed.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The count of the features of layer `id` that filter `text` matches,
/// and the count of its features. `None` if the text does not parse.
pub fn matches(state: &AppState, id: LayerId, text: &str) -> Option<(usize, usize)> {
    let layer = state.doc.layer(id)?;
    let count = (state.help_cache).matches(layer, state.map_generation, text)?;
    Some((count, layer.features.len()))
}

impl ExprHelp {
    /// The help for layer `id`. The feature of the preview is the first
    /// selected feature of the layer, or else its first feature.
    pub fn for_layer(state: &AppState, id: LayerId) -> Self {
        let Some(layer) = state.doc.layer(id) else {
            return Self::empty();
        };
        let selected = state
            .feature_selection
            .iter()
            .filter(|f| f.layer == id)
            .find_map(|f| layer.features.iter().find(|g| g.uuid() == f.uuid));
        let sample = selected.or(layer.features.first());
        ExprHelp {
            fields: layer.fields.iter().map(|f| f.name.clone()).collect(),
            types: layer.fields.iter().map(|f| f.kind).collect(),
            sample: sample.map(|f| f.attributes.clone()),
            selected: selected.is_some(),
            geometry: layer.geometry().unwrap_or(GeometryKind::Point),
            values: state.help_cache.values(layer, state.map_generation),
        }
    }

    /// No fields and no feature.
    pub fn empty() -> Self {
        ExprHelp {
            fields: Vec::new(),
            types: Vec::new(),
            sample: None,
            selected: false,
            geometry: GeometryKind::Point,
            values: Arc::default(),
        }
    }

    /// The type of field `name`.
    pub fn field_type(&self, name: &str) -> Option<FieldType> {
        let i = self.fields.iter().position(|f| f == name)?;
        self.types.get(i).copied()
    }

    /// The value of field `name` in the feature of the preview.
    pub fn sample(&self, name: &str) -> Option<&Value> {
        self.sample.as_ref()?.get(name)
    }

    /// True if the feature of the preview is a selected feature, and false
    /// if it is the first feature of the layer.
    pub fn sample_is_selected(&self) -> bool {
        self.selected
    }

    /// True if the layer has a feature for the preview.
    pub fn has_sample(&self) -> bool {
        self.sample.is_some()
    }

    /// The distinct values of text field `name` in the layer, sorted.
    pub fn values(&self, name: &str) -> &[String] {
        self.values.texts.get(name).map_or(&[], Vec::as_slice)
    }

    /// The count of the features of the layer.
    pub fn feature_count(&self) -> usize {
        self.values.features
    }

    /// The value of expression `text` for the feature of the preview.
    /// `None` if the text does not parse or the layer has no features.
    pub fn eval(&self, text: &str) -> Option<Value> {
        let expr = Expr::parse(text).ok()?;
        let attributes = self.sample.as_ref()?;
        Some(expr.eval(&Context {
            attributes,
            scale: PREVIEW_SCALE,
            geometry: self.geometry,
        }))
    }

    /// The fields, and then the functions.
    pub fn chips(&self) -> Vec<Chip> {
        let fields = self.fields.iter().map(|name| Chip {
            label: name.clone(),
            insert: field_ref(name),
            call: false,
        });
        let functions = FUNCTIONS.iter().map(|f| Chip {
            label: f.name.to_owned(),
            insert: f.signature.to_owned(),
            call: true,
        });
        fields.chain(functions).collect()
    }

    /// The line under a field with `text`. `None` for blank text, and for a
    /// layer with no features.
    pub fn line(&self, text: &str, kind: ExprKind) -> Option<Line> {
        if text.trim().is_empty() {
            return None;
        }
        let expr = match Expr::parse(text) {
            Ok(expr) => expr,
            Err(e) => return Some(Line::Error(e.short())),
        };
        let attributes = self.sample.as_ref()?;
        let value = expr.eval(&Context {
            attributes,
            scale: PREVIEW_SCALE,
            geometry: self.geometry,
        });
        let text = match kind {
            ExprKind::Filter if value == Value::Bool(true) => MATCHES.to_owned(),
            ExprKind::Filter => NO_MATCH.to_owned(),
            ExprKind::Text => value.label().unwrap_or_else(|| NO_LABEL.to_owned()),
            ExprKind::Value => value.label().unwrap_or_else(|| NO_VALUE.to_owned()),
        };
        Some(Line::Preview(text))
    }

    /// True if `text` parses, or is blank.
    pub fn valid(&self, text: &str) -> bool {
        text.trim().is_empty() || Expr::parse(text).is_ok()
    }
}

/// Replaces the selection of the text field `id` with the text of `chip`,
/// or inserts it at the cursor. The insert of a call selects its first
/// argument. A field with no cursor gets the text at its end.
pub fn insert_at_cursor(ctx: &egui::Context, id: egui::Id, text: &mut String, chip: &Chip) {
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    let count = text.chars().count();
    let (start, end) = state.cursor.char_range().map_or((count, count), |r| {
        let (a, b): (usize, usize) = (r.primary.index.into(), r.secondary.index.into());
        let (a, b) = (a.min(count), b.min(count));
        (a.min(b), a.max(b))
    });
    let byte = |c: usize| text.char_indices().nth(c).map_or(text.len(), |(i, _)| i);
    let (from, to) = (byte(start), byte(end));
    text.replace_range(from..to, &chip.insert);
    let (a, b) = if chip.call {
        first_argument(&chip.insert)
    } else {
        let n = chip.insert.chars().count();
        (n, n)
    };
    let range = CCursorRange::two(CCursor::new(start + a), CCursor::new(start + b));
    state.cursor.set_char_range(Some(range));
    state.store(ctx, id);
}

/// The first argument of `call`, as a range of chars. The place after the
/// opening parenthesis for a call with no arguments.
fn first_argument(call: &str) -> (usize, usize) {
    let chars: Vec<char> = call.chars().collect();
    let Some(open) = chars.iter().position(|c| *c == '(') else {
        return (chars.len(), chars.len());
    };
    let start = open + 1;
    let end = chars[start..]
        .iter()
        .position(|c| *c == ',' || *c == ')')
        .map_or(chars.len(), |p| start + p);
    (start, end)
}

/// The "Insert" menu of a desktop expression field `id`, with the fields
/// and the functions. Returns true when the user inserts one.
pub fn insert_menu(ui: &mut egui::Ui, help: &ExprHelp, id: egui::Id, text: &mut String) -> bool {
    let mut chosen = None;
    ui.menu_button(INSERT, |ui| {
        let chips = help.chips();
        let (fields, functions) = chips.split_at(help.fields.len());
        ui.add_enabled_ui(!fields.is_empty(), |ui| {
            ui.menu_button(FIELDS, |ui| {
                for chip in fields {
                    if ui.button(&chip.label).clicked() {
                        chosen = Some(chip.clone());
                    }
                }
            });
        });
        ui.menu_button(FUNCTIONS_MENU, |ui| {
            for (chip, f) in functions.iter().zip(FUNCTIONS) {
                let button = ui.button(&chip.insert).on_hover_text(f.summary);
                if button.clicked() {
                    chosen = Some(chip.clone());
                }
            }
        });
    });
    let Some(chip) = chosen else {
        return false;
    };
    insert_at_cursor(ui.ctx(), id, text, &chip);
    ui.memory_mut(|m| m.request_focus(id));
    true
}

/// The line under a desktop expression field: the result in weak text, or
/// the message of the parser in the warning color.
pub fn desktop_line(ui: &mut egui::Ui, help: &ExprHelp, text: &str, kind: ExprKind) {
    match help.line(text, kind) {
        Some(Line::Preview(result)) => {
            ui.weak(result);
        }
        Some(Line::Error(message)) => {
            ui.colored_label(ui.visuals().warn_fg_color, message);
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn help(attributes: &[(&str, Value)]) -> ExprHelp {
        let sample = attributes
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect();
        ExprHelp {
            fields: attributes.iter().map(|(k, _)| (*k).to_owned()).collect(),
            types: vec![FieldType::Text; attributes.len()],
            sample: Some(sample),
            selected: false,
            geometry: GeometryKind::Point,
            values: Arc::default(),
        }
    }

    fn towns() -> Layer {
        use vale_store::{Feature, Field, Geometry, LayerKind};
        let fields = vec![
            Field::new("name", FieldType::Text),
            Field::new("pop", FieldType::Integer),
            Field::new("capital", FieldType::Bool),
        ];
        let towns = [("Ardmere", 12400.0), ("Bree", 300.0), ("Ardmere", 9.0)];
        let features = towns
            .iter()
            .map(|(name, pop)| {
                let attributes = BTreeMap::from([
                    ("name".to_owned(), Value::Text((*name).into())),
                    ("pop".to_owned(), Value::Number(*pop)),
                    ("capital".to_owned(), Value::Bool(false)),
                ]);
                let at = Geometry::Points(vec![[0.0, 0.0]]);
                Feature::new(at, attributes)
            })
            .collect();
        Layer {
            id: LayerId(1),
            name: "Towns".into(),
            kind: LayerKind::Vector(GeometryKind::Point),
            visible: true,
            source: None,
            fields,
            features,
            style: None,
        }
    }

    #[test]
    fn help_lists_field_types() {
        let layer = towns();
        let values = LayerValues::of(&layer);
        assert_eq!(values.features, 3);
        assert_eq!(values.texts["name"], ["Ardmere", "Bree"]);
        assert!(!values.texts.contains_key("pop"));
        let mut h = help(&[("name", Value::Text("Ardmere".into()))]);
        h.values = Arc::new(values);
        assert_eq!(h.field_type("name"), Some(FieldType::Text));
        assert_eq!(h.field_type("elev"), None);
        assert_eq!(h.values("name"), ["Ardmere", "Bree"]);
        assert_eq!(h.sample("name"), Some(&Value::Text("Ardmere".into())));
        assert_eq!(h.eval("upper(name)"), Some(Value::Text("ARDMERE".into())));
    }

    #[test]
    fn help_counts_matching_features() {
        let layer = towns();
        assert_eq!(count_matches(&layer, "pop > 100"), Some(2));
        assert_eq!(count_matches(&layer, ""), Some(3));
        assert_eq!(count_matches(&layer, "pop >"), None);
        let cache = HelpCache::default();
        assert_eq!(cache.matches(&layer, 0, "pop > 100"), Some(2));
        let computed = cache.computed();
        // The same text and the same layer do not compute again.
        for _ in 0..3 {
            assert_eq!(cache.matches(&layer, 0, "pop > 100"), Some(2));
            cache.values(&layer, 0);
        }
        assert_eq!(cache.computed(), computed);
        // A change of the maps computes again.
        assert_eq!(cache.matches(&layer, 1, "name = 'Bree'"), Some(1));
        assert!(cache.computed() > computed);
    }

    #[test]
    fn the_line_gives_the_result_of_each_kind() {
        let h = help(&[("name", Value::Text("Ardent".into())), ("pop", Value::Null)]);
        let line = |text, kind| h.line(text, kind);
        let preview = |s: &str| Some(Line::Preview(s.to_owned()));
        assert_eq!(line("name = 'Ardent'", ExprKind::Filter), preview(MATCHES));
        assert_eq!(line("name = 'Bree'", ExprKind::Filter), preview(NO_MATCH));
        assert_eq!(line("upper(name)", ExprKind::Text), preview("ARDENT"));
        assert_eq!(line("pop", ExprKind::Text), preview(NO_LABEL));
        assert_eq!(line("pop", ExprKind::Value), preview(NO_VALUE));
        assert_eq!(line("round(2.46, 1)", ExprKind::Value), preview("2.5"));
        assert_eq!(line(" ", ExprKind::Filter), None);
        let error = Some(Line::Error("Unknown function 'shout'.".to_owned()));
        assert_eq!(line("shout(name)", ExprKind::Text), error);
        assert_eq!(ExprHelp::empty().line("name", ExprKind::Text), None);
    }

    #[test]
    fn the_chips_list_the_fields_and_then_the_functions() {
        let h = help(&[("river name", Value::Null), ("pop", Value::Null)]);
        let chips = h.chips();
        assert_eq!(chips[0].insert, "\"river name\"");
        assert_eq!(chips[1].insert, "pop");
        assert!(!chips[0].call);
        assert_eq!(chips.len(), 2 + FUNCTIONS.len());
        let round = chips.iter().find(|c| c.label == "round").unwrap();
        assert_eq!(round.insert, "round(x, digits)");
        assert!(round.call);
    }

    #[test]
    fn an_insert_replaces_the_selection_and_selects_the_first_argument() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("field");
        let select = |a: usize, b: usize| {
            let mut state = egui::TextEdit::load_state(&ctx, id).unwrap_or_default();
            let range = CCursorRange::two(CCursor::new(a), CCursor::new(b));
            state.cursor.set_char_range(Some(range));
            state.store(&ctx, id);
        };
        let selection = || {
            let r = egui::TextEdit::load_state(&ctx, id)
                .and_then(|s| s.cursor.char_range())
                .unwrap();
            (usize::from(r.secondary.index), usize::from(r.primary.index))
        };
        let h = help(&[("river name", Value::Null)]);
        let chips = h.chips();
        let round = chips.iter().find(|c| c.label == "round").unwrap();

        let mut text = "é = ".to_owned();
        select(4, 4);
        insert_at_cursor(&ctx, id, &mut text, &chips[0]);
        assert_eq!(text, "é = \"river name\"");
        assert_eq!(selection(), (16, 16));

        let mut text = "a + b".to_owned();
        select(4, 5);
        insert_at_cursor(&ctx, id, &mut text, round);
        assert_eq!(text, "a + round(x, digits)");
        assert_eq!(selection(), (10, 11));
    }
}
