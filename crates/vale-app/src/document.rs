//! The document: project, map frame, and layer entries.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use vale_import::ImportError;
use vale_import::geojson::{self, Imported};
use vale_render::{Symbol, Symbols};
use vale_sphere::ProjectionSpec;
use vale_store::{
    Field, FieldType, GeometryKind, Layer, LayerId, MapId, MapRecord, MapStyleId, MapStyleRecord,
    Project, ProjectionPreset, Units, World,
};
use vale_style::{Color, LabelClass, LayerStyle, MapStyle, Stroke};

use crate::view::View;
use crate::world;

/// One layer in the drawing order.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerEntry {
    pub layer: LayerId,
    pub style: LayerStyle,
}

/// How the map looks: view, layer styles, and map style. The projection is
/// on the map of the atlas.
#[derive(Clone, Debug, PartialEq)]
pub struct MapFrame {
    /// `None` means fit the world at the next compose.
    pub view: Option<View>,
    /// One entry for each vector layer of the project, in the order of the
    /// project. The first entry is at the bottom.
    pub entries: Vec<LayerEntry>,
    pub graticule: bool,
    pub labels: bool,
    pub style: MapStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub project: Project,
    pub frame: MapFrame,
    symbols: SymbolLibrary,
}

/// The symbols that the styles can use: the symbols of the project.
#[derive(Clone, Debug)]
struct SymbolLibrary {
    symbols: Arc<Symbols>,
    /// The names of the project symbols that the library holds.
    names: Vec<String>,
}

impl PartialEq for SymbolLibrary {
    fn eq(&self, other: &Self) -> bool {
        self.names == other.names
    }
}

impl SymbolLibrary {
    fn of(project: &Project) -> SymbolLibrary {
        let mut symbols = Symbols::default();
        for record in project.symbols() {
            match Symbol::from_svg(&record.svg) {
                Ok(symbol) => symbols.insert(&record.name, symbol),
                Err(e) => eprintln!("The symbol {:?} is not valid: {e}", record.name),
            }
        }
        SymbolLibrary {
            symbols: Arc::new(symbols),
            names: project.symbols().iter().map(|r| r.name.clone()).collect(),
        }
    }
}

/// The largest SVG file that a project takes as a symbol, in bytes.
pub const MAX_SYMBOL_BYTES: u64 = 1 << 20;

/// The style that a layer holds. A style that does not parse is the default style.
fn stored_style(layer: &Layer) -> Option<LayerStyle> {
    let text = layer.style.as_deref()?;
    LayerStyle::from_json(text)
        .inspect_err(|e| eprintln!("The style of layer {:?} is not valid: {e}", layer.name))
        .ok()
}

fn rank(kind: GeometryKind) -> u8 {
    match kind {
        GeometryKind::Polygon => 0,
        GeometryKind::Line => 1,
        GeometryKind::Point => 2,
    }
}

/// The rank of a layer in the order of an import. A raster layer draws
/// below each imported layer.
fn layer_rank(layer: &Layer) -> u8 {
    layer.geometry().map_or(0, rank)
}

impl Document {
    pub fn empty() -> Self {
        let project = Project::new(World::default());
        Document {
            symbols: SymbolLibrary::of(&project),
            project,
            frame: MapFrame {
                view: None,
                entries: Vec::new(),
                graticule: true,
                labels: true,
                style: MapStyle::default(),
            },
        }
    }

    /// The bundled Natural Earth 110m world.
    pub fn sample() -> Self {
        let mut doc = Document::empty();
        doc.project.world = World {
            name: "Earth (sample)".to_string(),
            radius_km: 6371.0,
            units: Units::Metric,
            ..World::default()
        };
        let sources: [(&str, &str); 3] = [
            (
                "Land",
                include_str!("../../../data/natural-earth/110m/ne_110m_land.geojson"),
            ),
            (
                "Rivers",
                include_str!(
                    "../../../data/natural-earth/110m/ne_110m_rivers_lake_centerlines.geojson"
                ),
            ),
            (
                "Places",
                include_str!(
                    "../../../data/natural-earth/110m/ne_110m_populated_places_simple.geojson"
                ),
            ),
        ];
        let mut ids = Vec::new();
        for (name, text) in sources {
            let imported = geojson::read_str(name, text).expect("bundled sample data is valid");
            ids.extend(doc.add_imported(imported, None));
        }
        let (land, rivers, places) = (ids[0], ids[1], ids[2]);

        let s = &mut doc.entry_mut(land).unwrap().style;
        s.set_fill(Some(Color::rgb(244, 241, 232)));
        s.set_stroke(Some(Stroke::solid(Color::rgb(150, 160, 165), 0.6)));

        let s = &mut doc.entry_mut(rivers).unwrap().style;
        s.set_stroke(Some(Stroke::solid(Color::rgb(90, 150, 200), 1.2)));
        s.label.enabled = true;
        s.label.field = Some("name".to_string());
        s.label.size = 11.0;
        s.label.italic = true;
        s.label.color = Color::rgb(40, 100, 160);

        let s = &mut doc.entry_mut(places).unwrap().style;
        s.set_fill(Some(Color::rgb(60, 60, 60)));
        s.set_stroke(Some(Stroke::solid(Color::rgb(255, 255, 255), 1.0)));
        s.set_point_radius(2.5);
        s.label.enabled = true;
        s.label.field = Some("name".to_string());
        s.label.priority_field = Some("pop_max".to_string());
        s.label.size = 11.0;
        s.label.italic = false;
        s.label.color = Color::rgb(30, 30, 30);
        doc
    }

    /// Adds each layer above the layers of its kind and of the kinds that
    /// draw below it. Polygons draw below lines, and lines draw below points.
    pub fn add_imported(&mut self, imported: Imported, source: Option<&Path>) -> Vec<LayerId> {
        let mut ids = Vec::new();
        for l in imported.layers {
            let file = source
                .and_then(Path::file_name)
                .map(|n| n.to_string_lossy().into_owned());
            let new_rank = rank(l.kind);
            let to = self
                .project
                .layers()
                .iter()
                .rposition(|x| layer_rank(x) <= new_rank)
                .map_or(0, |i| i + 1);
            let id = self.project.add_layer(l.name, l.kind, file, l.features);
            self.project.move_layer(id, to);
            self.sync_entries();
            ids.push(id);
        }
        ids
    }

    /// Adds an empty layer on top. Its name is the kind and the first number
    /// that no layer of the project has, for example "Lines 2". A point
    /// layer gets a `name` text field, and its points get labels from it.
    pub fn add_layer(&mut self, kind: GeometryKind) -> LayerId {
        let noun = match kind {
            GeometryKind::Point => "Points",
            GeometryKind::Line => "Lines",
            GeometryKind::Polygon => "Polygons",
        };
        let name = (1..)
            .map(|n| format!("{noun} {n}"))
            .find(|name| self.project.layers().iter().all(|l| &l.name != name))
            .expect("a free number");
        let id = self.project.add_layer(name, kind, None, Vec::new());
        if kind == GeometryKind::Point {
            self.project
                .add_field(id, Field::new("name", FieldType::Text));
        }
        self.sync_entries();
        id
    }

    /// Puts the entries in the order of the vector layers of the project. A
    /// vector layer with no entry gets one with the default style. A raster
    /// layer has no entry.
    fn sync_entries(&mut self) {
        let mut old = std::mem::take(&mut self.frame.entries);
        let mut counts = [0; 3];
        for layer in self.project.layers() {
            let Some(kind) = layer.geometry() else {
                continue;
            };
            let ordinal = &mut counts[rank(kind) as usize];
            let entry = match old.iter().position(|e| e.layer == layer.id) {
                Some(i) => old.swap_remove(i),
                None => LayerEntry {
                    layer: layer.id,
                    style: stored_style(layer).unwrap_or_else(|| {
                        LayerStyle::default_for(kind, *ordinal, &layer.field_names())
                    }),
                },
            };
            *ordinal += 1;
            self.frame.entries.push(entry);
        }
    }

    /// Replaces the project. An entry stays if the new project has a layer
    /// with its ID and its kind. Each other layer gets a new entry.
    pub fn set_project(&mut self, project: Project) {
        let projection = self.projection(self.first_map());
        let old = std::mem::replace(&mut self.project, project);
        if old.world.radius_km != self.project.world.radius_km
            || projection != self.projection(self.first_map())
        {
            self.frame.view = None;
        }
        let kind = |p: &Project, id| p.layer(id).map(|l| l.kind);
        let new = &self.project;
        self.frame
            .entries
            .retain(|e| kind(new, e.layer).is_some() && kind(new, e.layer) == kind(&old, e.layer));
        for entry in &mut self.frame.entries {
            let Some(layer) = new.layer(entry.layer) else {
                continue;
            };
            if layer
                .style
                .as_ref()
                .is_some_and(|t| *t != entry.style.to_json())
                && let Some(style) = stored_style(layer)
            {
                entry.style = style;
            }
        }
        self.sync_entries();
        self.sync_symbols();
    }

    /// The symbols that the styles can use: the symbols of the project.
    pub fn symbols(&self) -> &Arc<Symbols> {
        &self.symbols.symbols
    }

    /// Reads the symbols of the project again, if they changed.
    pub fn sync_symbols(&mut self) {
        let same = self.symbols.names.iter().map(String::as_str).eq(self
            .project
            .symbols()
            .iter()
            .map(|r| r.name.as_str()));
        if !same {
            self.symbols = SymbolLibrary::of(&self.project);
        }
    }

    /// Adds an SVG file to the symbols of the project, and returns its name.
    /// The name comes from `name`, and it is not the name of another symbol of
    /// the project.
    pub fn add_symbol(&mut self, name: &str, svg: String) -> Result<String, String> {
        if svg.len() as u64 > MAX_SYMBOL_BYTES {
            return Err("The file is larger than 1 MB".to_string());
        }
        Symbol::from_svg(&svg).map_err(|e| {
            let mut text = e.to_string();
            if let Some(first) = text.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            text
        })?;
        let name = self.project.add_symbol(name, svg, &[]);
        self.sync_symbols();
        Ok(name)
    }

    /// The layers whose label font is missing, with the warning of each.
    /// The fonts are the fonts of the label styles and of the label classes
    /// of each map and each map style. `families` are the families that the
    /// labels can use.
    pub fn missing_fonts(&self, families: &[String]) -> Vec<(LayerId, String)> {
        let atlas = &self.project.atlas;
        (self.frame.entries.iter())
            .filter_map(|e| {
                let maps = atlas.maps.iter().map(|m| &m.labels);
                let stored = (maps.chain(atlas.map_styles.iter().map(|s| &s.labels)))
                    .filter_map(|labels| labels.get(&e.layer))
                    .filter_map(|t| vale_style::classes_from_json(t).ok())
                    .flatten()
                    .map(|c| c.font);
                let warning = std::iter::once(e.style.label.font.clone())
                    .chain(stored)
                    .find_map(|font| crate::fonts::missing(font.as_deref(), families))?;
                Some((e.layer, warning))
            })
            .collect()
    }

    /// The style of each vector layer whose project layer does not hold it, as JSON.
    pub fn changed_styles(&self) -> Vec<(LayerId, String)> {
        self.frame
            .entries
            .iter()
            .filter_map(|e| {
                let stored = self.project.layer(e.layer)?.style.as_deref();
                let json = e.style.to_json();
                (stored != Some(json.as_str())).then_some((e.layer, json))
            })
            .collect()
    }

    /// Writes each changed style into the project.
    pub fn store_styles(&mut self) {
        for (id, json) in self.changed_styles() {
            self.project.set_layer_style(id, Some(json));
        }
    }

    pub fn open_geojson(&mut self, path: &Path) -> Result<Vec<LayerId>, ImportError> {
        let imported = geojson::read_file(path)?;
        Ok(self.add_imported(imported, Some(path)))
    }

    /// Removes the entry and the layer, and each mask of the layer.
    pub fn remove_layer(&mut self, id: LayerId) -> bool {
        self.frame.entries.retain(|e| e.layer != id);
        for entry in &mut self.frame.entries {
            if entry.style.style.mask.is_some_and(|m| m.layer() == id) {
                entry.style.style.mask = None;
            }
        }
        self.project.remove_layer(id).is_some()
    }

    /// Moves a layer of the map past the next layer of the map. `up` moves
    /// toward the top of the drawing. Returns false at the end.
    pub fn move_entry(&mut self, id: LayerId, up: bool) -> bool {
        let entries = &self.frame.entries;
        let Some(i) = entries.iter().position(|e| e.layer == id) else {
            return false;
        };
        let next = if up {
            i.checked_add(1)
        } else {
            i.checked_sub(1)
        };
        let Some(next) = next.and_then(|i| entries.get(i)).map(|e| e.layer) else {
            return false;
        };
        let layers = self.project.layers();
        let to = layers.iter().position(|l| l.id == next);
        to.is_some_and(|to| self.move_layer(id, to))
    }

    /// Moves a layer one place in the draw order of the layers of each kind.
    /// `up` moves toward the top of the drawing. Returns false at the end.
    pub fn move_by_one(&mut self, id: LayerId, up: bool) -> bool {
        let layers = self.project.layers();
        let Some(i) = layers.iter().position(|l| l.id == id) else {
            return false;
        };
        if (up && i + 1 >= layers.len()) || (!up && i == 0) {
            return false;
        }
        self.move_layer(id, if up { i + 1 } else { i - 1 })
    }

    /// Moves a layer to this index of the draw order. Index 0 is the bottom.
    pub fn move_layer(&mut self, id: LayerId, to: usize) -> bool {
        let moved = self.project.move_layer(id, to);
        self.sync_entries();
        moved
    }

    /// Gives a layer a new name. A blank name changes nothing.
    pub fn rename_layer(&mut self, id: LayerId, name: &str) -> bool {
        let name = name.trim();
        !name.is_empty() && self.project.rename_layer(id, name.to_string())
    }

    /// Gives a field of a vector layer a new name. The label field and the
    /// priority field of the style keep the field.
    pub fn rename_field(&mut self, id: LayerId, old: &str, new: &str) -> bool {
        if !self.project.rename_field(id, old, new) {
            return false;
        }
        if let Some(entry) = self.entry_mut(id) {
            let label = &mut entry.style.label;
            for field in [&mut label.field, &mut label.priority_field] {
                if field.as_deref() == Some(old) {
                    *field = Some(new.to_string());
                }
            }
        }
        true
    }

    pub fn set_visible(&mut self, id: LayerId, visible: bool) -> bool {
        self.project.set_layer_visible(id, visible)
    }

    pub fn visible(&self, id: LayerId) -> bool {
        self.project.layer(id).is_some_and(|l| l.visible)
    }

    pub fn entry(&self, id: LayerId) -> Option<&LayerEntry> {
        self.frame.entries.iter().find(|e| e.layer == id)
    }

    pub fn entry_mut(&mut self, id: LayerId) -> Option<&mut LayerEntry> {
        self.frame.entries.iter_mut().find(|e| e.layer == id)
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.project.layer(id)
    }

    /// Clamps to `world::RADIUS_KM` and rescales the view so the map looks the same.
    pub fn set_radius_km(&mut self, km: f64) {
        let new = km.clamp(*world::RADIUS_KM.start(), *world::RADIUS_KM.end());
        let old = self.project.world.radius_km;
        if let Some(v) = &mut self.frame.view {
            v.center = kurbo::Point::new(v.center.x * new / old, v.center.y * new / old);
            v.scale *= old / new;
        }
        self.project.world.radius_km = new;
    }

    /// The first map of the atlas. The single map view draws it.
    pub fn first_map(&self) -> MapId {
        self.project.atlas.maps[0].id
    }

    /// The projection of a map. An unknown map, or a string that the app
    /// cannot read, is the default projection.
    pub fn projection(&self, map: MapId) -> ProjectionSpec {
        self.project
            .atlas
            .map(map)
            .and_then(|m| ProjectionSpec::from_proj(&m.projection))
            .unwrap_or_default()
    }

    pub fn set_projection(&mut self, map: MapId, spec: ProjectionSpec) {
        let text = spec.to_proj();
        let first = self.first_map();
        let Some(record) = self.project.atlas.map_mut(map) else {
            return;
        };
        if record.projection == text {
            return;
        }
        record.projection = text;
        if map == first {
            self.frame.view = None;
        }
    }

    /// The custom projections of the project.
    pub fn presets(&self) -> &[ProjectionPreset] {
        &self.project.atlas.presets
    }

    /// Saves the projection of a map as a custom projection. The name must
    /// not be empty. A custom projection with that name changes.
    pub fn save_preset(&mut self, map: MapId, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let text = self.projection(map).to_proj();
        self.project.atlas.save_preset(name, &text);
        true
    }

    /// Removes a custom projection. A map that uses it keeps its projection.
    pub fn remove_preset(&mut self, name: &str) -> bool {
        self.project.atlas.remove_preset(name)
    }

    /// Gives a map the projection of a custom projection. Returns false if
    /// the project has none with that name, or if the string is not one of the app.
    pub fn apply_preset(&mut self, map: MapId, name: &str) -> bool {
        let spec = self
            .presets()
            .iter()
            .find(|p| p.name == name)
            .and_then(|p| ProjectionSpec::from_proj(&p.projection));
        let Some(spec) = spec else {
            return false;
        };
        self.set_projection(map, spec);
        true
    }

    /// Gives each map with no map style and each map style a copy of the
    /// globe style of each layer that it has no style for, and turns the
    /// style overrides of an older file into styles. Returns `true` if the
    /// project changed.
    pub fn fill_map_styles(&mut self) -> bool {
        let layers = crate::map_styles::needed(&self.project);
        if layers.is_empty() {
            return false;
        }
        let globe: Vec<_> = (layers.into_iter())
            .filter_map(|id| Some((id, self.layer_style(id)?.to_json())))
            .collect();
        crate::map_styles::fill_with(&mut self.project, &globe)
    }

    /// The stored style of a layer in a target, as JSON. A map with no entry
    /// gives the entry of its map style.
    fn style_text(&self, target: Target, layer: LayerId) -> Option<&String> {
        let atlas = &self.project.atlas;
        match target {
            Target::Map(m) => atlas.style_text(m, layer),
            Target::Style(s) => atlas.map_style(s)?.styles.get(&layer),
        }
    }

    /// True if a map links to a map style and holds its own style for a
    /// layer.
    pub fn own_style(&self, map: MapId, layer: LayerId) -> bool {
        let record = self.project.atlas.map(map);
        record.is_some_and(|m| m.style.is_some() && m.styles.contains_key(&layer))
    }

    /// The label classes of a layer in a map. A map with no classes for the
    /// layer, a map whose classes do not read, and `None` give the class of
    /// the label style of the layer. The priority of that class is the place
    /// of the layer in the draw order.
    pub fn label_classes(&self, map: Option<MapId>, layer: LayerId) -> Vec<LabelClass> {
        match map {
            Some(map) => self.label_classes_in(Target::Map(map), layer),
            None => self.default_classes(layer, self.layer_style(layer)),
        }
    }

    /// The label classes of a layer in a target. A map gives the classes
    /// that it draws. A target with no classes for the layer gives the class
    /// of the label style of the layer in the target.
    pub fn label_classes_in(&self, target: impl Into<Target>, layer: LayerId) -> Vec<LabelClass> {
        let target = target.into();
        let stored = self
            .label_text(target, layer)
            .and_then(|t| vale_style::classes_from_json(t).ok());
        if let Some(classes) = stored {
            return classes;
        }
        self.default_classes(layer, self.map_style(target, layer))
    }

    /// The stored label classes of a layer in a target, as JSON. A map
    /// with no entry gives the entry of its map style.
    fn label_text(&self, target: Target, layer: LayerId) -> Option<&String> {
        let atlas = &self.project.atlas;
        match target {
            Target::Map(m) => atlas.label_text(m, layer),
            Target::Style(s) => atlas.map_style(s)?.labels.get(&layer),
        }
    }

    /// The class of a label style, with the place of the layer in the draw
    /// order as its priority.
    fn default_classes(&self, layer: LayerId, style: Option<LayerStyle>) -> Vec<LabelClass> {
        let Some(position) = self.frame.entries.iter().position(|e| e.layer == layer) else {
            return Vec::new();
        };
        let (Some(l), Some(style)) = (self.layer(layer), style) else {
            return Vec::new();
        };
        let Some(kind) = l.geometry() else {
            return Vec::new();
        };
        LabelClass::from_style(&l.name, &style.label, kind, position as i32)
            .into_iter()
            .collect()
    }

    /// Stores the label classes of a layer in a target. Returns false if
    /// nothing changed.
    pub fn set_label_classes(
        &mut self,
        target: impl Into<Target>,
        layer: LayerId,
        classes: &[LabelClass],
    ) -> bool {
        let Some((_, _, labels)) = self.parts_mut(target.into()) else {
            return false;
        };
        let text = vale_style::classes_to_json(classes);
        if labels.get(&layer) == Some(&text) {
            return false;
        }
        labels.insert(layer, text);
        true
    }

    /// The hidden layers, the layer styles, and the label classes of a
    /// target.
    fn parts_mut(&mut self, target: Target) -> Option<Parts<'_>> {
        let atlas = &mut self.project.atlas;
        match target {
            Target::Map(m) => {
                let m = atlas.map_mut(m)?;
                Some((&mut m.hidden, &mut m.styles, &mut m.labels))
            }
            Target::Style(s) => {
                let s = atlas.map_style_mut(s)?;
                Some((&mut s.hidden, &mut s.styles, &mut s.labels))
            }
        }
    }

    /// The layer with the style of a map. `None` is the globe style.
    pub fn map_entry(&self, map: Option<MapId>, layer: LayerId) -> Option<Cow<'_, LayerEntry>> {
        let entry = self.entry(layer)?;
        let style = map.and_then(|m| self.parsed_style(Target::Map(m), layer));
        Some(match style {
            Some(style) => Cow::Owned(LayerEntry { layer, style }),
            None => Cow::Borrowed(entry),
        })
    }

    /// The stored style of a layer in a target, if it parses.
    fn parsed_style(&self, target: Target, layer: LayerId) -> Option<LayerStyle> {
        let text = self.style_text(target, layer)?;
        LayerStyle::from_json(text)
            .inspect_err(|e| eprintln!("The style of layer {layer:?} in a map is not valid: {e}"))
            .ok()
    }

    /// The style of a layer: the style of its entry, or the stored style of
    /// a raster layer or an image layer.
    pub fn layer_style(&self, layer: LayerId) -> Option<LayerStyle> {
        match self.entry(layer) {
            Some(entry) => Some(entry.style.clone()),
            None => stored_style(self.project.layer(layer)?),
        }
    }

    /// The style of a layer in a target. A map gives the style that it
    /// draws. A target with no style for the layer gives the globe style.
    pub fn map_style(&self, target: impl Into<Target>, layer: LayerId) -> Option<LayerStyle> {
        self.layer(layer)?;
        (self.parsed_style(target.into(), layer)).or_else(|| self.layer_style(layer))
    }

    /// Makes `edited` the style of a layer in a target. In a map that links
    /// to a map style, the style becomes the own style of the map. Returns
    /// false if nothing changed.
    pub fn set_map_style(
        &mut self,
        target: impl Into<Target>,
        layer: LayerId,
        edited: &LayerStyle,
    ) -> bool {
        if self.layer(layer).is_none() {
            return false;
        }
        let Some((_, styles, _)) = self.parts_mut(target.into()) else {
            return false;
        };
        let text = edited.to_json();
        if styles.get(&layer) == Some(&text) {
            return false;
        }
        styles.insert(layer, text);
        true
    }

    /// Removes the own style of a layer in a map that links to a map style,
    /// so that the map draws the style of the map style. Returns false if
    /// nothing changed.
    pub fn use_map_style(&mut self, map: MapId, layer: LayerId) -> bool {
        let record = self.project.atlas.map_mut(map);
        let Some(record) = record.filter(|m| m.style.is_some()) else {
            return false;
        };
        record.styles.remove(&layer).is_some()
    }

    /// Makes a copy of the globe style the style of a layer in a target.
    /// Returns false if nothing changed.
    pub fn copy_globe_style(&mut self, target: impl Into<Target>, layer: LayerId) -> bool {
        let target = target.into();
        let Some(globe) = self.layer_style(layer) else {
            return false;
        };
        self.set_map_style(target, layer, &globe)
    }

    /// True if a target shows a layer.
    pub fn shown(&self, target: impl Into<Target>, layer: LayerId) -> bool {
        let atlas = &self.project.atlas;
        match target.into() {
            Target::Map(m) => atlas.map(m).is_some() && atlas.shows(m, layer),
            Target::Style(s) => atlas
                .map_style(s)
                .is_some_and(|s| !s.hidden.contains(&layer)),
        }
    }

    /// Shows or hides a layer in a target. A map with a map style holds
    /// only the switches that differ from its map style. Returns false if
    /// nothing changed.
    pub fn set_shown(&mut self, target: impl Into<Target>, layer: LayerId, on: bool) -> bool {
        match target.into() {
            Target::Style(s) => {
                let Some(style) = self.project.atlas.map_style_mut(s) else {
                    return false;
                };
                set_member(&mut style.hidden, layer, !on)
            }
            Target::Map(m) => {
                let atlas = &mut self.project.atlas;
                let style_on = (atlas.style_of(m)).is_none_or(|s| !s.hidden.contains(&layer));
                let Some(map) = atlas.map_mut(m) else {
                    return false;
                };
                let own = on != style_on;
                let hidden = set_member(&mut map.hidden, layer, own && !on);
                let shown = set_member(&mut map.shown, layer, own && on);
                hidden || shown
            }
        }
    }

    /// A map with the settings that it draws as its own settings, and with
    /// no map style.
    pub fn resolved_map(&self, map: MapId) -> Option<MapRecord> {
        let atlas = &self.project.atlas;
        let record = atlas.map(map)?;
        let mut out = record.clone();
        out.style = None;
        out.shown.clear();
        out.styles = (self.project.layers().iter())
            .filter_map(|l| {
                let text = match atlas.style_text(map, l.id) {
                    Some(text) => text.clone(),
                    None => self.layer_style(l.id)?.to_json(),
                };
                Some((l.id, text))
            })
            .collect();
        out.legacy_overrides.clear();
        let Some(style) = atlas.style_of(map) else {
            return Some(out);
        };
        let switches = (record.hidden.iter())
            .chain(&record.shown)
            .chain(&style.hidden);
        out.hidden = switches
            .copied()
            .filter(|&l| !atlas.shows(map, l))
            .collect();
        out.labels = (record.labels.keys())
            .chain(style.labels.keys())
            .filter_map(|&l| Some((l, atlas.label_text(map, l)?.clone())))
            .collect();
        Some(out)
    }

    /// Moves the settings that a map draws into a new map style with a
    /// free name, and links the map to it. The map keeps no settings of its
    /// own.
    pub fn save_as_map_style(&mut self, map: MapId) -> Option<MapStyleId> {
        let resolved = self.resolved_map(map)?;
        let atlas = &mut self.project.atlas;
        let id = atlas.add_map_style(MapStyleRecord {
            id: MapStyleId(0),
            name: String::new(),
            hidden: resolved.hidden,
            styles: resolved.styles,
            labels: resolved.labels,
        });
        let record = atlas.map_mut(map)?;
        record.style = Some(id);
        record.hidden.clear();
        record.shown.clear();
        record.styles.clear();
        record.labels.clear();
        Some(id)
    }

    /// Links a map to a map style, and the map drops its own switches,
    /// styles, and label classes. `None` unlinks the map, and the map keeps
    /// the settings that it draws. Returns false if nothing changed.
    pub fn link_map_style(&mut self, map: MapId, style: Option<MapStyleId>) -> bool {
        let atlas = &self.project.atlas;
        let Some(record) = atlas.map(map) else {
            return false;
        };
        if record.style == style || style.is_some_and(|s| atlas.map_style(s).is_none()) {
            return false;
        }
        let new = match style {
            Some(s) => MapRecord {
                style: Some(s),
                hidden: BTreeSet::new(),
                shown: BTreeSet::new(),
                styles: BTreeMap::new(),
                legacy_overrides: BTreeMap::new(),
                labels: BTreeMap::new(),
                ..record.clone()
            },
            None => match self.resolved_map(map) {
                Some(resolved) => resolved,
                None => return false,
            },
        };
        if let Some(record) = self.project.atlas.map_mut(map) {
            *record = new;
        }
        true
    }

    /// Deletes a map style. Each map with the map style keeps the settings
    /// that it draws. Returns false if the project has no such map style.
    pub fn delete_map_style(&mut self, id: MapStyleId) -> bool {
        if self.project.atlas.map_style(id).is_none() {
            return false;
        }
        let linked: Vec<MapId> = (self.project.atlas.maps.iter())
            .filter(|m| m.style == Some(id))
            .map(|m| m.id)
            .collect();
        for map in linked {
            self.link_map_style(map, None);
        }
        self.project.atlas.remove_map_style(id);
        true
    }

    /// Renames a map style. The name has no spaces at its ends, and it is
    /// not empty and not the name of another map style. Returns false if
    /// nothing changed.
    pub fn rename_map_style(&mut self, id: MapStyleId, name: &str) -> bool {
        let name = name.trim();
        let atlas = &mut self.project.atlas;
        if name.is_empty() || atlas.map_styles.iter().any(|s| s.name == name) {
            return false;
        }
        let Some(style) = atlas.map_style_mut(id) else {
            return false;
        };
        style.name = name.to_string();
        true
    }
}

/// The settings of a map or a map style that a map can change.
type Parts<'a> = (
    &'a mut BTreeSet<LayerId>,
    &'a mut BTreeMap<LayerId, String>,
    &'a mut BTreeMap<LayerId, String>,
);

/// Puts `item` in `set` or takes it out. Returns false if nothing changed.
fn set_member(set: &mut BTreeSet<LayerId>, item: LayerId, member: bool) -> bool {
    if member {
        set.insert(item)
    } else {
        set.remove(&item)
    }
}

/// The map or the map style whose settings an edit changes. A read of a map
/// gives the settings that the map draws.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Map(MapId),
    Style(MapStyleId),
}

impl From<MapId> for Target {
    fn from(map: MapId) -> Target {
        Target::Map(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resolved_map_agrees_with_the_settings_that_the_map_draws() {
        let mut doc = Document::sample();
        let map = doc.project.atlas.maps[0].id;
        let layers: Vec<LayerId> = doc.frame.entries.iter().map(|e| e.layer).collect();
        let (a, b, c) = (layers[0], layers[1], layers[2]);
        let style = doc.save_as_map_style(map).unwrap();
        let s = Target::Style(style);
        assert!(doc.set_shown(s, a, false));
        assert!(doc.set_shown(s, b, false));
        assert!(doc.set_shown(map, b, true));
        assert!(doc.set_shown(map, c, false));
        let mut wide = doc.map_style(s, a).unwrap();
        wide.label.size = 31.0;
        assert!(doc.set_map_style(s, a, &wide));
        let mut wider = doc.map_style(map, b).unwrap();
        wider.label.size = 41.0;
        assert!(doc.set_map_style(map, b, &wider));
        let mut classes = doc.label_classes(Some(map), c);
        classes.clear();
        assert!(doc.set_label_classes(s, c, &classes));

        let resolved = doc.resolved_map(map).unwrap();
        assert_eq!(resolved.style, None);
        assert!(resolved.shown.is_empty());
        let before: Vec<_> = (layers.iter())
            .map(|&l| {
                let classes = doc.label_classes(Some(map), l);
                (doc.shown(map, l), doc.map_style(map, l), classes)
            })
            .collect();
        doc.project.atlas.maps[0] = resolved;
        let after: Vec<_> = (layers.iter())
            .map(|&l| {
                let classes = doc.label_classes(Some(map), l);
                (doc.shown(map, l), doc.map_style(map, l), classes)
            })
            .collect();
        assert_eq!(before, after);
        assert!(!after[0].0 && after[1].0 && !after[2].0);
        assert!(after[2].2.is_empty());
    }
}
