//! A new feature of the point tool, the line tool, or the polygon tool,
//! while the inspector takes its values.

use eframe::egui::{self, Rect, pos2};
use vale_store::{Feature, Field, FieldType, Geometry, GeometryKind, Value};

use super::pad::Panel;
use super::pad::geometry::Rects;
use super::{AppState, Workspace};
use crate::globe::PlacedPoint;
use crate::globe::math::{V3, dir_to_lonlat};
use crate::globe::select::FeatureRef;

/// The space between the new point and the edge of the free part of the
/// screen.
const POINT_GAP: f32 = 24.0;

/// One field of the form.
pub struct FormField {
    pub name: String,
    kind: FieldType,
    pub text: String,
    /// The selected features have different values, and the user did not
    /// edit the field.
    pub mixed: bool,
}

impl FormField {
    /// A field of the form with this text.
    pub(super) fn new(field: &Field, text: String) -> FormField {
        FormField {
            name: field.name.clone(),
            kind: field.kind,
            text,
            mixed: false,
        }
    }

    /// The value of the text. An empty text, and a text that is not a value
    /// of the type of the field, give `Null`.
    pub(super) fn value(&self) -> Value {
        let text = self.text.trim();
        if text.is_empty() {
            return Value::Null;
        }
        self.kind.parse(text).unwrap_or(Value::Null)
    }

    /// True if the text is a value of the type of the field.
    pub(super) fn valid(&self) -> bool {
        let text = self.text.trim();
        text.is_empty() || self.kind.parse(text).is_some()
    }
}

/// A new feature that the inspector shows. The feature and the values
/// that the inspector gives it are one undo step.
pub struct NewFeature {
    pub feature: FeatureRef,
    /// The new point, until the globe turns it out from under the inspector
    /// and the keyboard.
    show: Option<V3>,
}

impl AppState {
    /// Puts the last point of the point tool in the selected point layer.
    /// The point gets the default value of each field, and it becomes the
    /// selection.
    pub fn poll_point(&mut self) {
        let Some(point) = self.globe.take_point() else {
            return;
        };
        self.commit_new_feature();
        let Some(id) = self.selected_vector() else {
            return;
        };
        let Some(layer) = self.doc.project.layer(id) else {
            return;
        };
        if layer.geometry() != Some(GeometryKind::Point) {
            return;
        }
        let PlacedPoint { dir, exact } = point;
        let lonlat = exact.unwrap_or_else(|| dir_to_lonlat(dir).into());
        let feature = Feature::new(Geometry::Points(vec![lonlat]), Default::default());
        let uuid = feature.uuid();
        if !self.doc.project.add_feature(id, feature) {
            return;
        }
        let new = FeatureRef { layer: id, uuid };
        self.set_feature_selection(vec![new]);
        if self.start_new_feature(new, Some(dir)) {
            self.pipeline.forget(id);
            self.touch();
        } else {
            self.end_vector_edit(id);
        }
    }

    /// Holds the undo step of a new feature open while the inspector takes
    /// its values. Returns false if the inspector is closed or the layer has
    /// no fields. `show`: the new point.
    pub(super) fn start_new_feature(&mut self, feature: FeatureRef, show: Option<V3>) -> bool {
        self.drop_focus = true;
        let fields = self
            .doc
            .layer(feature.layer)
            .is_some_and(|l| !l.fields.is_empty());
        if !self.inspector_shown || !fields {
            return false;
        }
        self.new_feature = Some(NewFeature { feature, show });
        true
    }

    /// Ends the undo step of the new feature if the inspector no longer
    /// shows it, or if the tool cannot draw in its layer.
    pub fn check_new_feature(&mut self) {
        let Some(new) = &self.new_feature else {
            return;
        };
        let open = self.workspace == Workspace::Globe
            && self.inspector_shown
            && (self.globe.placing() || self.globe.drawing())
            && self.selected_vector() == Some(new.feature.layer)
            && self.feature_selection == [new.feature];
        if !open {
            self.commit_new_feature();
        }
    }

    /// Puts the values of the inspector in the new feature and ends its
    /// undo step.
    pub fn commit_new_feature(&mut self) {
        let Some(layer) = self.new_feature.as_ref().map(|new| new.feature.layer) else {
            return;
        };
        self.commit_selection_form();
        self.new_feature = None;
        self.end_vector_edit(layer);
    }
}

/// True if a field can take the focus in this frame. A press or a release
/// of the pointer in this frame takes the focus away, so the field waits
/// for a frame with neither.
pub fn can_take_focus(ctx: &egui::Context) -> bool {
    !ctx.input(|i| i.pointer.any_down() || i.pointer.any_released())
}

/// The top of the on-screen keyboard of an iPad. The app does not know the
/// keyboard, so this is the top of a keyboard with its row of suggestions.
pub fn keyboard_top(screen: Rect) -> f32 {
    let part = if screen.width() > screen.height() {
        0.45
    } else {
        0.62
    };
    screen.top() + part * screen.height()
}

/// Turns the globe so that the new point is not under the inspector of the
/// iPad layout. If a field takes the focus, the point is also above the
/// keyboard.
pub fn keep_in_view(state: &mut AppState, rects: &Rects) {
    let Some(dir) = state.new_feature.as_mut().and_then(|new| new.show.take()) else {
        return;
    };
    let Some(pos) = state.globe.project(dir) else {
        return;
    };
    let screen = rects.screen;
    let keyboard = state
        .selection_form
        .as_ref()
        .is_some_and(|f| f.takes_focus());
    let bottom = if keyboard {
        keyboard_top(screen)
    } else {
        screen.bottom()
    };
    let top_left = pos2(screen.left(), rects.workspace.bottom());
    let mut free = Rect::from_min_max(top_left, pos2(screen.right(), bottom));
    let panels = &state.pad.panels;
    if let Some((_, panel)) = panels.iter().find(|(p, _)| *p == Panel::Attributes) {
        if rects.sheet.is_some() {
            free.max.y = free.max.y.min(panel.top());
        } else {
            free.max.x = free.max.x.min(panel.left());
        }
    }
    let free = free.shrink(POINT_GAP);
    if !free.is_positive() || free.contains(pos) {
        return;
    }
    state.globe.move_place(pos, free.clamp(pos));
}
