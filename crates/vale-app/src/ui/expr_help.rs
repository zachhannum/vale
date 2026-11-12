//! The help of an expression field in both layouts: the fields and the
//! functions that the user can insert at the cursor, and the result of the
//! expression for one feature.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use eframe::egui;
use eframe::egui::text::{CCursor, CCursorRange};
use vale_store::FurnitureId;
use vale_store::{FieldType, Geometry, GeometryKind, Layer, LayerId, Value};
use vale_style::expr::editor::{Subject, measures_for, rasters_for};
use vale_style::expr::{Context, Expr, PageValues, Stat, field_ref, layer_functions};

use super::AppState;
use crate::furniture::PageSample;

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
/// The measures of the geometry, in the filter field menu and the label text.
pub const GEOMETRY_MENU: &str = "Geometry";
/// The `@` variables, in the value editor.
pub const VARIABLES_MENU: &str = "Variables";
/// The raster layers, in the filter field menu, the label text, and the
/// value editor.
pub const RASTER_MENU: &str = "Raster";

/// The warning of a raster call that names no raster layer.
pub fn no_raster(name: &str) -> String {
    format!("No raster layer is named '{name}'.")
}

/// The page values of a text item.
pub const SCALE: &str = "Scale";
pub const RELATIVE_SCALE: &str = "Relative scale";
pub const MAP_NAME: &str = "Map name";
pub const PAGE_NAME: &str = "Page name";
pub const PAGE_NUMBER: &str = "Page number";
pub const PAGE_COUNT: &str = "Page count";
pub const DATE: &str = "Date";

/// The page values that the iPad bar inserts: the name of each, and its
/// text.
pub const PAGE_VALUES: [(&str, &str); 6] = [
    (SCALE, "scale('fraction')"),
    (MAP_NAME, "map_name()"),
    (PAGE_NAME, "page_name()"),
    (PAGE_NUMBER, "page_number()"),
    (PAGE_COUNT, "page_count()"),
    (DATE, "date('long')"),
];

/// The page values of the desktop Insert menu and completion list: also the
/// relative scale.
pub const DESKTOP_PAGE_VALUES: [(&str, &str); 7] = [
    (SCALE, "scale('fraction')"),
    (RELATIVE_SCALE, "scale('relative', 'cm')"),
    (MAP_NAME, "map_name()"),
    (PAGE_NAME, "page_name()"),
    (PAGE_NUMBER, "page_number()"),
    (PAGE_COUNT, "page_count()"),
    (DATE, "date('long')"),
];

/// The name of page function `name` in the UI.
pub fn page_label(name: &str) -> &'static str {
    match name {
        "scale" => SCALE,
        "map_name" => MAP_NAME,
        "page_name" => PAGE_NAME,
        "page_number" => PAGE_NUMBER,
        "page_count" => PAGE_COUNT,
        _ => DATE,
    }
}

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
    pub geometry: GeometryKind,
    /// The geometry of the feature of the preview, for the measures.
    shape: Option<Geometry>,
    radius_km: f64,
    values: Arc<LayerValues>,
    /// The raster layers of the project: the name and the unit of each, in
    /// the list order.
    pub raster_layers: Vec<(String, String)>,
    /// The page values of a text item. The help of a text item has no
    /// fields, and it lists the page values in place of the fields and the
    /// functions.
    page: Option<PageSample>,
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

/// The count of the features of `layer` that filter `text` matches, on a
/// world of radius `radius_km`. `None` if the text does not parse.
pub fn count_matches(layer: &Layer, radius_km: f64, text: &str) -> Option<usize> {
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
                shape: Some(&f.geometry),
                radius_km,
                page: None,
                rasters: None,
                index: 0,
            };
            expr.eval(&ctx) == Value::Bool(true)
        })
        .count();
    Some(count)
}

/// The count of the bins of the histogram of a value.
pub const BINS: usize = 12;

/// The numbers that a value gives across the features of a layer.
#[derive(Clone, Debug, PartialEq)]
pub struct ValueRange {
    pub min: f64,
    pub max: f64,
    /// The count of the features with a number.
    pub count: usize,
    /// The count of the numbers in each of `BINS` equal parts of the range.
    pub bins: Vec<usize>,
}

/// The range of the numbers that value `text` gives across `layer`, on a
/// world of radius `radius_km`. `None` if the text does not parse or gives
/// no number.
pub fn value_range(layer: &Layer, radius_km: f64, text: &str) -> Option<ValueRange> {
    let expr = Expr::parse(text).ok()?;
    let geometry = layer.geometry().unwrap_or(GeometryKind::Point);
    let numbers: Vec<f64> = (layer.features.iter())
        .filter_map(|f| {
            let ctx = Context {
                attributes: &f.attributes,
                scale: PREVIEW_SCALE,
                geometry,
                shape: Some(&f.geometry),
                radius_km,
                page: None,
                rasters: None,
                index: 0,
            };
            match expr.eval(&ctx) {
                Value::Number(n) if n.is_finite() => Some(n),
                _ => None,
            }
        })
        .collect();
    let min = numbers.iter().copied().reduce(f64::min)?;
    let max = numbers.iter().copied().reduce(f64::max)?;
    let mut bins = vec![0; BINS];
    for n in &numbers {
        let t = if max > min {
            (n - min) / (max - min)
        } else {
            0.0
        };
        bins[((t * BINS as f64) as usize).min(BINS - 1)] += 1;
    }
    Some(ValueRange {
        min,
        max,
        count: numbers.len(),
        bins,
    })
}

/// A number of a range in short.
fn range_number(n: f64) -> String {
    if n.fract() == 0.0 {
        crate::classify::number_text(n)
    } else {
        format!("{n:.1}")
    }
}

/// The line of a range, for example "4.0 to 22.8 across 310 features".
pub fn range_text(r: &ValueRange) -> String {
    let (a, b) = (range_number(r.min), range_number(r.max));
    format!("{a} to {b} across {} features", r.count)
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
    /// The radius of the world of `counts` and `ranges`.
    radius_km: f64,
    counts: HashMap<String, Option<usize>>,
    ranges: HashMap<String, Option<Arc<ValueRange>>>,
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
        radius_km: Option<f64>,
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
                radius_km: radius_km.unwrap_or_default(),
                counts: HashMap::new(),
                ranges: HashMap::new(),
            };
            layers.insert(layer.id, entry);
        }
        let entry = layers.get_mut(&layer.id).expect("inserted");
        // The values do not depend on the radius.
        if radius_km.is_some_and(|r| r != entry.radius_km) {
            entry.radius_km = radius_km.unwrap_or_default();
            entry.counts.clear();
            entry.ranges.clear();
        }
        f(entry, self)
    }

    fn count_one(&self) {
        *self.computed.lock().unwrap_or_else(|e| e.into_inner()) += 1;
    }

    /// The values of the features of `layer`.
    pub fn values(&self, layer: &Layer, generation: u64) -> Arc<LayerValues> {
        self.entry(layer, generation, None, |e, _| e.values.clone())
    }

    /// The count of the features of `layer` that filter `text` matches.
    pub fn matches(
        &self,
        layer: &Layer,
        generation: u64,
        radius_km: f64,
        text: &str,
    ) -> Option<usize> {
        self.entry(layer, generation, Some(radius_km), |e, cache| {
            if let Some(count) = e.counts.get(text) {
                return *count;
            }
            cache.count_one();
            if e.counts.len() >= MAX_COUNTS {
                e.counts.clear();
            }
            let count = count_matches(layer, radius_km, text);
            e.counts.insert(text.to_owned(), count);
            count
        })
    }

    /// The range of the numbers that value `text` gives across `layer`.
    pub fn range(
        &self,
        layer: &Layer,
        generation: u64,
        radius_km: f64,
        text: &str,
    ) -> Option<Arc<ValueRange>> {
        self.entry(layer, generation, Some(radius_km), |e, cache| {
            if let Some(range) = e.ranges.get(text) {
                return range.clone();
            }
            cache.count_one();
            if e.ranges.len() >= MAX_COUNTS {
                e.ranges.clear();
            }
            let range = value_range(layer, radius_km, text).map(Arc::new);
            e.ranges.insert(text.to_owned(), range.clone());
            range
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
    let radius_km = state.doc.project.world.radius_km;
    let count = (state.help_cache).matches(layer, state.map_generation, radius_km, text)?;
    Some((count, layer.features.len()))
}

/// The range of the numbers that value `text` gives across layer `id`.
pub fn range(state: &AppState, id: LayerId, text: &str) -> Option<Arc<ValueRange>> {
    let layer = state.doc.layer(id)?;
    let radius_km = state.doc.project.world.radius_km;
    (state.help_cache).range(layer, state.map_generation, radius_km, text)
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
            shape: sample.map(|f| f.geometry.clone()),
            radius_km: state.doc.project.world.radius_km,
            values: state.help_cache.values(layer, state.map_generation),
            raster_layers: raster_layers(state),
            page: None,
        }
    }

    /// The help for the text of text item `id`, with its page values.
    pub fn for_text_item(state: &mut AppState, id: FurnitureId) -> Self {
        let Some(rec) = state.doc.project.atlas.furniture(id) else {
            return Self::empty();
        };
        let today = crate::furniture::today();
        let sample = crate::furniture::page_sample(&mut state.pipeline, &state.doc, rec, today);
        Self::for_page(sample)
    }

    /// The help with the page values of `sample`.
    pub fn for_page(sample: PageSample) -> Self {
        ExprHelp {
            page: Some(sample),
            ..Self::empty()
        }
    }

    /// True if the help is for the text of a text item.
    pub fn is_page(&self) -> bool {
        self.page.is_some()
    }

    /// No fields and no feature.
    pub fn empty() -> Self {
        ExprHelp {
            fields: Vec::new(),
            types: Vec::new(),
            sample: None,
            selected: false,
            geometry: GeometryKind::Point,
            shape: None,
            radius_km: 0.0,
            values: Arc::default(),
            raster_layers: Vec::new(),
            page: None,
        }
    }

    /// The value of `expr` for the feature of the preview, or for the page
    /// of a text item.
    fn value_of(&self, expr: &Expr) -> Option<Value> {
        static NONE: BTreeMap<String, Value> = BTreeMap::new();
        let (attributes, page) = match &self.page {
            Some(p) => (&NONE, Some(p as &dyn PageValues)),
            None => (self.sample.as_ref()?, None),
        };
        Some(expr.eval(&Context {
            attributes,
            scale: PREVIEW_SCALE,
            geometry: self.geometry,
            shape: self.shape.as_ref(),
            radius_km: self.radius_km,
            page,
            rasters: None,
            index: 0,
        }))
    }

    /// The type of field `name`.
    pub fn field_type(&self, name: &str) -> Option<FieldType> {
        let i = self.fields.iter().position(|f| f == name)?;
        self.types.get(i).copied()
    }

    /// The type of the value of `subject`.
    pub fn subject_type(&self, subject: &Subject) -> Option<FieldType> {
        subject.field_type(|name| self.field_type(name))
    }

    /// The measures of the geometry of the layer.
    pub fn measures(&self) -> Vec<Subject> {
        measures_for(self.geometry)
    }

    /// The raster values of the layer, one for each raster layer, with no
    /// stat.
    pub fn rasters(&self) -> Vec<Subject> {
        let names: Vec<String> = self.raster_layers.iter().map(|(n, _)| n.clone()).collect();
        rasters_for(self.geometry, &names)
    }

    /// The unit of raster layer `name`.
    pub fn raster_unit(&self, name: &str) -> Option<&str> {
        let (_, unit) = self.raster_layers.iter().find(|(n, _)| n == name)?;
        Some(unit)
    }

    /// The raster values of the Insert menus, for each raster layer: its
    /// name and its values. A point layer has one value with no stat, and a
    /// line or a polygon layer has one value for each stat.
    pub fn raster_menu(&self) -> Vec<(String, Vec<Subject>)> {
        (self.raster_layers.iter())
            .map(|(name, _)| {
                let values = match self.geometry {
                    GeometryKind::Point => vec![Subject::Raster(name.clone(), None)],
                    _ => (Stat::ALL.into_iter())
                        .map(|st| Subject::Raster(name.clone(), Some(st)))
                        .collect(),
                };
                (name.clone(), values)
            })
            .collect()
    }

    /// The warning for an expression that parses: a raster call that names
    /// no raster layer.
    fn warning(&self, expr: &Expr) -> Option<String> {
        let names = expr.raster_layers();
        let missing = (names.iter()).find(|n| !self.raster_layers.iter().any(|(r, _)| r == *n))?;
        Some(no_raster(missing))
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

    /// The value of expression `text` for the feature of the preview, or
    /// for the page of a text item. `None` if the text does not parse or
    /// the layer has no features.
    pub fn eval(&self, text: &str) -> Option<Value> {
        self.value_of(&Expr::parse(text).ok()?)
    }

    /// The fields, and then the functions. For a text item, the page
    /// values.
    pub fn chips(&self) -> Vec<Chip> {
        if self.is_page() {
            let page = DESKTOP_PAGE_VALUES.iter().map(|(label, insert)| Chip {
                label: (*label).to_owned(),
                insert: (*insert).to_owned(),
                call: false,
            });
            return page.collect();
        }
        let fields = self.fields.iter().map(|name| Chip {
            label: name.clone(),
            insert: field_ref(name),
            call: false,
        });
        let functions = layer_functions().map(|f| Chip {
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
        if let Some(warning) = self.warning(&expr) {
            return Some(Line::Error(warning));
        }
        let value = self.value_of(&expr)?;
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

/// The raster layers of the project, with their names and units, in the
/// list order.
fn raster_layers(state: &AppState) -> Vec<(String, String)> {
    let project = &state.doc.project;
    (project.layers().iter())
        .filter_map(|l| {
            let raster = project.raster(l.raster()?)?;
            Some((l.name.clone(), raster.unit.clone()))
        })
        .collect()
}

/// The "Insert" menu of a desktop expression field `id`, with the fields,
/// the raster layers, and the functions. Returns true when the user inserts one.
pub fn insert_menu(ui: &mut egui::Ui, help: &ExprHelp, id: egui::Id, text: &mut String) -> bool {
    let mut chosen = None;
    ui.menu_button(INSERT, |ui| {
        let chips = help.chips();
        if help.is_page() {
            for chip in &chips {
                if ui.button(&chip.label).clicked() {
                    chosen = Some(chip.clone());
                }
            }
            return;
        }
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
        let rasters = help.raster_menu();
        if !rasters.is_empty() {
            ui.menu_button(RASTER_MENU, |ui| {
                if let Some(s) = raster_menu_items(ui, &rasters) {
                    chosen = Some(Chip {
                        label: s.label(),
                        insert: s.to_text(),
                        call: false,
                    });
                }
            });
        }
        ui.menu_button(FUNCTIONS_MENU, |ui| {
            for (chip, f) in functions.iter().zip(layer_functions()) {
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

/// The items of the Raster submenu of an Insert menu: a button for each
/// raster value of a point layer, or a submenu of the stats for each raster
/// layer of a line or a polygon layer. Returns the value that the user
/// picked.
pub fn raster_menu_items(ui: &mut egui::Ui, rasters: &[(String, Vec<Subject>)]) -> Option<Subject> {
    let mut chosen = None;
    for (name, values) in rasters {
        match values.as_slice() {
            [one] => {
                if ui.button(name).clicked() {
                    chosen = Some(one.clone());
                }
            }
            _ => {
                ui.menu_button(name, |ui| {
                    for s in values {
                        let Subject::Raster(_, Some(st)) = s else {
                            continue;
                        };
                        if ui.button(st.label()).clicked() {
                            chosen = Some(s.clone());
                        }
                    }
                });
            }
        }
    }
    chosen
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

    const R: f64 = 6371.0;

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
            shape: None,
            radius_km: 0.0,
            values: Arc::default(),
            raster_layers: vec![("Height".into(), "m".into())],
            page: None,
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
        assert_eq!(count_matches(&layer, R, "pop > 100"), Some(2));
        assert_eq!(count_matches(&layer, R, ""), Some(3));
        assert_eq!(count_matches(&layer, R, "pop >"), None);
        let cache = HelpCache::default();
        assert_eq!(cache.matches(&layer, 0, R, "pop > 100"), Some(2));
        let computed = cache.computed();
        // The same text and the same layer do not compute again.
        for _ in 0..3 {
            assert_eq!(cache.matches(&layer, 0, R, "pop > 100"), Some(2));
            cache.values(&layer, 0);
        }
        assert_eq!(cache.computed(), computed);
        // A change of the maps computes again.
        assert_eq!(cache.matches(&layer, 1, R, "name = 'Bree'"), Some(1));
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
        // A raster call that names no raster layer gives a warning.
        let warning = Some(Line::Error("No raster layer is named 'Heat'.".to_owned()));
        assert_eq!(line("raster('Heat') > 3", ExprKind::Filter), warning);
        assert_eq!(
            line("raster('Height') > 3", ExprKind::Filter),
            preview(NO_MATCH)
        );
    }

    #[test]
    fn the_chips_list_the_fields_and_then_the_functions() {
        let h = help(&[("river name", Value::Null), ("pop", Value::Null)]);
        let chips = h.chips();
        assert_eq!(chips[0].insert, "\"river name\"");
        assert_eq!(chips[1].insert, "pop");
        assert!(!chips[0].call);
        assert_eq!(chips.len(), 2 + layer_functions().count());
        let round = chips.iter().find(|c| c.label == "round").unwrap();
        assert_eq!(round.insert, "round(x, digits)");
        assert!(round.call);
    }

    #[test]
    fn a_text_item_lists_the_page_values_and_a_layer_lists_no_page_values() {
        let layer = help(&[("name", Value::Text("Ardent".into()))]);
        let chips = layer.chips();
        assert!(chips.iter().any(|c| c.insert == "name"));
        let page = |c: &Chip| DESKTOP_PAGE_VALUES.iter().any(|(_, t)| *t == c.insert);
        assert!(!chips.iter().any(page));
        assert!(!layer.is_page());

        let item = ExprHelp::for_page(PageSample::default());
        assert!(item.is_page());
        assert!(item.fields.is_empty());
        let chips = item.chips();
        assert_eq!(chips.len(), DESKTOP_PAGE_VALUES.len());
        assert!(chips.iter().all(page));
        assert!(!item.has_sample());
        let text = "concat('Page ', page_number(), ' of ', page_count())";
        assert_eq!(item.eval(text), Some(Value::Text("Page 0 of 0".into())));
        assert_eq!(
            item.eval("date('iso')"),
            Some(Value::Text("1970-01-01".into()))
        );
        assert_eq!(
            item.line("date('long')", ExprKind::Text),
            Some(Line::Preview("January 1, 1970".into()))
        );
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
