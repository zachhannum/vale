//! The attribute form of a point that the point tool placed.

use eframe::egui::{self, Order, Pos2, Rect, pos2, vec2};
use vale_store::{Feature, Geometry, Layer, LayerId, Uuid, Value};

use super::pad::geometry::{GAP, MARGIN, ROW, Rects};
use super::pad::theme;
use super::pad::widgets::{self, BODY_PAD, Controls, PANEL_ROW};
use super::{AppState, Workspace, save_session};
use crate::globe::PlacedPoint;
use crate::globe::math::{V3, dir_to_lonlat};

/// The field of a layer with no fields.
pub const NAME_FIELD: &str = "name";
/// The button of the desktop form that closes it.
pub const DONE: &str = "Done";
const WIDTH: f32 = 360.0;
const BOTTOM: f32 = 12.0;
const CARD: &str = "point-form";
/// The space between the new point and the edge of the free part of the
/// screen.
const POINT_GAP: f32 = 24.0;

/// The type of the values of a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Text,
    Number,
    Bool,
}

impl Kind {
    /// The type of the values that the features of the layer have in the
    /// field. A field with no values, or with values of more than one type,
    /// is text.
    fn of(layer: &Layer, name: &str) -> Kind {
        let mut kinds = layer
            .features
            .iter()
            .filter_map(|f| match f.attributes.get(name)? {
                Value::Null => None,
                Value::Bool(_) => Some(Kind::Bool),
                Value::Number(_) => Some(Kind::Number),
                Value::Text(_) => Some(Kind::Text),
            });
        let Some(first) = kinds.next() else {
            return Kind::Text;
        };
        if kinds.all(|k| k == first) {
            first
        } else {
            Kind::Text
        }
    }
}

/// One field of the form.
pub struct FormField {
    pub name: String,
    kind: Kind,
    pub text: String,
}

impl FormField {
    /// The value of the text. An empty text is `Null`. A text that is not
    /// a value of the type of the field stays text.
    fn value(&self) -> Value {
        let text = self.text.trim();
        if text.is_empty() {
            return Value::Null;
        }
        match self.kind {
            Kind::Number => text.parse().map(Value::Number).ok(),
            Kind::Bool => match text.to_lowercase().as_str() {
                "true" => Some(Value::Bool(true)),
                "false" => Some(Value::Bool(false)),
                _ => None,
            },
            Kind::Text => None,
        }
        .unwrap_or_else(|| Value::Text(text.to_owned()))
    }

    /// True if the text is a value of the type of the field.
    fn valid(&self) -> bool {
        self.kind == Kind::Text || !matches!(self.value(), Value::Text(_))
    }
}

/// The open form of a new point.
pub struct PointForm {
    pub layer: LayerId,
    pub feature: Uuid,
    pub fields: Vec<FormField>,
    /// The field that gets the keyboard focus in the next frame.
    focus: Option<usize>,
    /// The new point, until the form keeps it out from under the form and
    /// the keyboard.
    show: Option<V3>,
}

impl PointForm {
    /// The ID of the text edit of field `i`.
    fn field_id(&self, ui: &egui::Ui, i: usize) -> egui::Id {
        ui.id().with(&self.fields[i].name)
    }
}

impl AppState {
    /// Puts the last point of the point tool in the selected point layer
    /// and opens its form. The form of the point before closes.
    pub fn poll_point(&mut self) {
        let Some(point) = self.globe.take_point() else {
            return;
        };
        self.commit_point_form();
        let Some(id) = self.selected_vector() else {
            return;
        };
        let Some(layer) = self.doc.project.layer(id) else {
            return;
        };
        let named = layer.fields.is_empty();
        let names = match named {
            true => vec![NAME_FIELD.to_owned()],
            false => layer.fields.clone(),
        };
        let fields: Vec<FormField> = names
            .into_iter()
            .map(|name| FormField {
                kind: Kind::of(layer, &name),
                name,
                text: String::new(),
            })
            .collect();
        let PlacedPoint { dir, exact } = point;
        let lonlat = exact.unwrap_or_else(|| dir_to_lonlat(dir).into());
        let attributes = fields.iter().map(|f| (f.name.clone(), Value::Null));
        let feature = Feature::new(Geometry::Points(vec![lonlat]), attributes.collect());
        let uuid = feature.uuid();
        if !self.doc.project.add_feature(id, feature) {
            return;
        }
        // A layer that gets the name field labels its points with it.
        let entry = self.doc.frame.entries.iter_mut().find(|e| e.layer == id);
        let label = entry.map(|e| &mut e.style.label).filter(|_| named);
        if let Some(label) = label.filter(|l| l.field.is_none()) {
            label.field = Some(NAME_FIELD.to_owned());
            label.enabled = true;
        }
        self.pipeline.forget(id);
        self.touch();
        self.point_form = Some(PointForm {
            layer: id,
            feature: uuid,
            fields,
            focus: Some(0),
            show: Some(dir),
        });
    }

    /// Closes the form if the point tool cannot place a point in its layer.
    pub fn check_point_form(&mut self) {
        let open = self.point_form.as_ref().is_some_and(|form| {
            self.workspace == Workspace::Globe
                && self.globe.placing()
                && self.selected_vector() == Some(form.layer)
        });
        if !open {
            self.commit_point_form();
        }
    }

    /// Puts the values of the form in the point and closes the form. The
    /// point and its values are one undo step.
    pub fn commit_point_form(&mut self) {
        let Some(form) = self.point_form.take() else {
            return;
        };
        let values = form.fields.iter().map(|f| (f.name.clone(), f.value()));
        self.doc
            .project
            .set_attributes(form.layer, form.feature, values);
        self.pipeline.forget(form.layer);
        self.touch();
        self.line_edits += 1;
        // The next point must not join this undo step.
        let edits = self.edits();
        let Some(session) = self.session.as_mut() else {
            return;
        };
        self.saved_edits = edits;
        match save_session(session, &mut self.doc.project, &mut self.globe) {
            Ok(()) => self.saved_at = Some(std::time::Instant::now()),
            Err(e) => {
                self.save_failed = true;
                self.status = format!("The project did not save: {e}");
            }
        }
    }
}

/// The field that gets the focus in this frame. A press or a release of
/// the pointer in this frame takes the focus away, so the field waits for
/// a frame with neither.
fn take_focus(ctx: &egui::Context, form: &mut PointForm) -> Option<usize> {
    let pointer = ctx.input(|i| i.pointer.any_down() || i.pointer.any_released());
    if pointer { None } else { form.focus.take() }
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

/// The form in the iPad layout: a card near the top of the screen, above
/// the keyboard. The card takes no touch outside it, so the next tap of the
/// pen places the next point.
pub fn pad(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let Some(mut form) = state.point_form.take() else {
        return;
    };
    let title = state
        .doc
        .project
        .layer(form.layer)
        .map_or_else(String::new, |l| l.name.clone());
    let screen = rects.screen;
    let top = rects.workspace.bottom() + GAP;
    let room = keyboard_top(screen) - GAP - top;
    let fields = form.fields.len() as f32;
    let height = (ROW + fields * PANEL_ROW + BOTTOM).min(room.max(ROW + PANEL_ROW + BOTTOM));
    let width = WIDTH.min(screen.width() - 2.0 * MARGIN);
    let rect = Rect::from_min_size(
        pos2(screen.center().x - width / 2.0, top),
        vec2(width, height),
    );

    let mut done = false;
    let focus = take_focus(ctx, &mut form);
    let radius = theme::CARD_RADIUS.into();
    widgets::card(ctx, None, CARD, rect, Order::Foreground, radius, |ui| {
        ui.painter()
            .rect_filled(rect, radius, theme::card(false).to_opaque());
        ui.spacing_mut().item_spacing.y = 0.0;
        if widgets::title_row(ui, controls, &title, None, false).close {
            done = true;
        }
        let inner = ui.available_rect_before_wrap().shrink2(vec2(BODY_PAD, 0.0));
        let layout = egui::Layout::top_down(egui::Align::Min);
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
        egui::ScrollArea::vertical().show(&mut ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let count = form.fields.len();
            for i in 0..count {
                let field = &mut form.fields[i];
                let valid = field.valid();
                let (name, text) = (&field.name, &mut field.text);
                let resp = widgets::text_field(ui, controls, name, name, None, text, valid);
                if focus == Some(i) {
                    resp.request_focus();
                    resp.scroll_to_me(None);
                }
                // Return goes to the next field, and on the last field it
                // closes the form.
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if i + 1 < count {
                        form.focus = Some(i + 1);
                    } else {
                        done = true;
                    }
                }
            }
        });
    });
    if let Some(dir) = form.show.take() {
        keep_in_view(state, dir, rect, keyboard_top(screen));
    }
    state.point_form = Some(form);
    if done {
        state.commit_point_form();
    }
}

/// Turns the view so the new point is below the card and above the
/// keyboard.
fn keep_in_view(state: &mut AppState, dir: V3, card: Rect, keyboard: f32) {
    let globe = &mut state.globe;
    let Some(pos) = globe.project(dir) else {
        return;
    };
    let (top, bottom) = (card.bottom() + POINT_GAP, keyboard - POINT_GAP);
    if (top..=bottom).contains(&pos.y) {
        return;
    }
    let y = if top < bottom {
        (top + bottom) / 2.0
    } else {
        top
    };
    globe.move_place(pos, Pos2::new(pos.x, y));
}

/// The form in the desktop layout: a window at the top of the canvas.
pub fn desktop(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut form) = state.point_form.take() else {
        return;
    };
    let title = state
        .doc
        .project
        .layer(form.layer)
        .map_or_else(String::new, |l| l.name.clone());
    form.show = None;
    let focus = take_focus(ctx, &mut form);
    let mut done = false;
    egui::Window::new(title)
        .id(egui::Id::new(CARD))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_TOP, vec2(0.0, 48.0))
        .show(ctx, |ui| {
            egui::Grid::new("point-form-fields")
                .num_columns(2)
                .show(ui, |ui| {
                    let count = form.fields.len();
                    for i in 0..count {
                        let id = form.field_id(ui, i);
                        let field = &mut form.fields[i];
                        ui.label(&field.name);
                        let color = (!field.valid()).then(|| ui.visuals().warn_fg_color);
                        let edit = egui::TextEdit::singleline(&mut field.text)
                            .id(id)
                            .text_color_opt(color);
                        let resp = ui.add(edit);
                        if focus == Some(i) {
                            resp.request_focus();
                        }
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            if i + 1 < count {
                                form.focus = Some(i + 1);
                            } else {
                                done = true;
                            }
                        }
                        ui.end_row();
                    }
                });
            if ui.button(DONE).clicked() {
                done = true;
            }
        });
    state.point_form = Some(form);
    if done {
        state.commit_point_form();
    }
}
