//! The iPad layout of the world workspace.

mod common;
mod images;
mod screenshot;

use eframe::egui::{self, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use vale_app::document::Document;
use vale_app::globe::select::FeatureRef;
use vale_app::globe::view::GlobeView;
use vale_app::globe::{StackItem, Tool, WorldView};
use vale_app::headless;
use vale_app::projects::Locations;
use vale_app::session::Session;
use vale_app::ui::gradient::{COLUMN, ColumnParts, GRADIENT, band_row, widest_gap_value};
use vale_app::ui::pad::eyedropper;
use vale_app::ui::pad::flyout::Flyout;
use vale_app::ui::pad::geometry::{BrushCard, TOOLS, WidthClass};
use vale_app::ui::pad::number::{LABEL_COLUMN, TRACK_INSET};
use vale_app::ui::pad::raster;
use vale_app::ui::pad::{
    ADD_LAYER, CLEAR_PATH, DESELECT, EXPORT_GEOTIFF, GRATICULE, IMPORT_HEIGHTMAP, LAYER_NAME, LINE,
    LayerRef, MEASURE, ModeGroup, NEW_GEOJSON, NEW_HEIGHTMAP, NEW_IMAGE, NEW_RASTER, OPACITY,
    PANELS, POLYGON, PROJECTION, Page, Panel, RECENTER, RESET, SCULPT, SMOOTHING, SNAP,
    SNAP_DISTANCE, SNAP_EDGES, SNAP_VERTICES, SNAPPING, TERRAIN, TERRAIN_BALANCE, TERRAIN_FIT,
    TERRAIN_FIT_SWITCH, TOOLBOX, VECTOR, VIEW, VIEWS, WORLD_PROPERTIES, WORLDS, WORLDS_ROW,
    brush_size_text, panels_menu, readout_line, readout_text, theme,
};
use vale_app::ui::raster_form::{self, PRESETS, RasterForm, face_size_name};
use vale_app::ui::select;
use vale_app::ui::table::SHOW_IN_TABLE;
use vale_app::ui::world_form::{CREATE, FormMode, NAME, NEW_WORLD, RADIUS_SLIDER, WorldForm};
use vale_app::ui::{Action, AppState, FontTarget, Layout, Workspace, draw, panels};
use vale_app::world::{radius_fraction, three_significant};
use vale_store::{GeometryKind, Hillshade, LayerKind, RasterDisplay, RasterId, Units, World};
use vale_terrain::{Mode, Rain, meters_to_level};

type Pad = Harness<'static, AppState>;

/// The boards of the mockup: landscape, portrait, half split view, and
/// narrow split view.
const BOARDS: [(f32, f32); 4] = [
    (1194.0, 834.0),
    (834.0, 1194.0),
    (597.0, 834.0),
    (375.0, 834.0),
];
/// The bands that the system uses at the top and at the bottom.
const BANDS: (f32, f32) = (24.0, 20.0);
const WORLD_VIEWS: [WorldView; 2] = [WorldView::Globe, WorldView::Flat];

/// Shows each raster layer, and a globe with no layers, with this display.
fn show_as(state: &mut AppState, display: RasterDisplay) {
    state.globe.preview.display = display;
    let ids: Vec<RasterId> = state.doc.project.rasters().iter().map(|r| r.id).collect();
    for id in ids {
        state.set_raster_display(id, display);
    }
}

fn state() -> AppState {
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    state.set_layout(Layout::Pad);
    state
}

/// The layout of a world with no raster layer.
fn empty_pad((width, height): (f32, f32)) -> Pad {
    let mut h = Harness::builder()
        .with_size(vec2(width, height))
        .with_pixels_per_point(1.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state());
    h.run_steps(3);
    h
}

/// The layout of a world with a height layer that the pen paints.
fn pad(size: (f32, f32)) -> Pad {
    let mut h = empty_pad(size);
    h.state_mut().add_height();
    h.run_steps(2);
    h
}

/// The layout of a world with a painted height layer.
fn painted_pad(size: (f32, f32)) -> Pad {
    let mut h = pad(size);
    let globe = &mut h.state_mut().globe;
    globe.map.set(0, 0, 0, meters_to_level(100.0));
    assert!(h.state().has_raster());
    h.run_steps(2);
    h
}

fn control(h: &Pad, name: &str) -> Option<Rect> {
    let controls = &h.state().pad.controls;
    controls.iter().find(|c| c.name == name).map(|c| c.rect)
}

fn panel(h: &Pad, panel: Panel) -> Option<Rect> {
    let panels = &h.state().pad.panels;
    panels.iter().find(|(p, _)| *p == panel).map(|(_, r)| *r)
}

/// The first touch is also the pointer. A touch with a force is a pen.
fn touch(h: &mut Pad, phase: egui::TouchPhase, pos: Pos2, force: Option<f32>) {
    h.event(egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(1),
        phase,
        pos,
        force,
    });
    h.event(egui::Event::PointerMoved(pos));
    if phase != egui::TouchPhase::Move {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: phase == egui::TouchPhase::Start,
            modifiers: egui::Modifiers::NONE,
        });
    }
    if phase == egui::TouchPhase::End {
        h.event(egui::Event::PointerGone);
    }
}

/// A touch that moves from one place to another. A force makes it a pen.
fn drag(h: &mut Pad, from: Pos2, to: Pos2, force: Option<f32>) {
    touch(h, egui::TouchPhase::Start, from, force);
    h.step();
    for i in 1..=6 {
        touch(
            h,
            egui::TouchPhase::Move,
            from.lerp(to, i as f32 / 6.0),
            force,
        );
        h.step();
    }
    touch(h, egui::TouchPhase::End, to, force);
    h.run_steps(3);
}

/// Pulls an open sheet that covers the tool strip down to its title row.
fn lower_sheet(h: &mut Pad) {
    let rects = h.state().pad.rects.unwrap();
    if let Some(sheet) = rects.sheet.filter(|_| rects.row && rects.tool_cells == 0) {
        let bar = pos2(sheet.center().x, sheet.top() + 26.0);
        drag(h, bar, bar + vec2(0.0, sheet.height()), None);
        assert!(h.state().pad.rects.unwrap().tool_cells > 0);
    }
}

/// Opens or closes the Toolbox panel.
fn toolbox(h: &mut Pad) {
    open_panel(h, "Toolbox");
}

/// Opens or closes a panel with its tile in the Panels menu.
fn open_panel(h: &mut Pad, name: &str) {
    tap(h, vale_app::ui::pad::PANELS);
    tap(h, name);
}

/// A tap of a finger on the middle of a control.
fn tap(h: &mut Pad, name: &str) {
    let rect = control(h, name).unwrap_or_else(|| panic!("no control `{name}`"));
    let pos = rect.center();
    touch(h, egui::TouchPhase::Start, pos, None);
    h.step();
    touch(h, egui::TouchPhase::End, pos, None);
    h.run_steps(3);
}

/// Types `text` in the value field of the slider or the stepper `name`
/// with its keypad, and closes the keypad.
#[allow(dead_code)]
fn type_value(h: &mut Pad, name: &str, text: &str) {
    let field = format!("{name} value");
    tap(h, &field);
    for c in text.chars() {
        tap(h, &format!("{field} {c}"));
    }
    tap(h, &format!("{field} Done"));
}

/// A tap on the plus button (`up`) or the minus button of the stepper
/// `name`. A `tap` lasts long enough for the button to repeat, and a
/// `quick_tap` does not.
#[allow(dead_code)]
fn step(h: &mut Pad, name: &str, up: bool) {
    let button = if up { "Increase" } else { "Decrease" };
    quick_tap(h, &format!("{button} {name}"));
}

/// The control `name`, or the value field of the stepper `name`.
fn row_of(h: &Pad, name: &str) -> Option<Rect> {
    control(h, name).or_else(|| control(h, &format!("{name} value")))
}

/// A drag on the slider `name`, along its track, that is longer than the
/// track. It moves the value to the highest end with `up`, or else to the
/// lowest end.
fn slide_to_end(h: &mut Pad, name: &str, up: bool) {
    let rect = control(h, name).unwrap_or_else(|| panic!("no control `{name}`"));
    let length = rect.width().max(rect.height()) + 40.0;
    let sign = if up { 1.0 } else { -1.0 };
    let along = if rect.height() > rect.width() {
        vec2(0.0, -sign * length)
    } else {
        vec2(sign * length, 0.0)
    };
    drag(h, rect.center(), rect.center() + along, None);
}

/// A tap of a finger on the middle of a control, with each half of the
/// touch in one frame. A `tap` lasts as long as a hold on the brush cell.
fn quick_tap(h: &mut Pad, name: &str) {
    let pos = control(h, name)
        .unwrap_or_else(|| panic!("no control `{name}`"))
        .center();
    for pressed in [true, false] {
        let phase = if pressed {
            egui::TouchPhase::Start
        } else {
            egui::TouchPhase::End
        };
        let events = &mut h.input_mut().events;
        events.push(egui::Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(1),
            phase,
            pos,
            force: None,
        });
        events.push(egui::Event::PointerMoved(pos));
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        if !pressed {
            events.push(egui::Event::PointerGone);
        }
        h.step();
    }
    h.run_steps(3);
}

/// The group of the brush mode with this name.
fn group_of(name: &str) -> ModeGroup {
    let has = |group: &ModeGroup| group.modes().iter().any(|(mode, ..)| *mode == name);
    ModeGroup::ALL.into_iter().find(has).unwrap()
}

/// The names of all brush modes.
fn all_modes() -> impl Iterator<Item = &'static str> {
    ModeGroup::ALL
        .into_iter()
        .flat_map(|group| group.modes().iter().map(|(name, ..)| *name))
}

/// Opens the flyout of a group with taps on its cell.
fn open_group(h: &mut Pad, group: ModeGroup) {
    if h.state().pad.flyouts.open != Some(Flyout::Brush(group)) {
        let globe = &h.state().globe;
        if !globe.brushing() || ModeGroup::of(globe.brush.mode) != group {
            quick_tap(h, group.name());
        }
        quick_tap(h, group.name());
    }
    assert_eq!(h.state().pad.flyouts.open, Some(Flyout::Brush(group)));
}

/// Opens the flyout of the group of the brush mode.
fn open_modes(h: &mut Pad) {
    let group = ModeGroup::of(h.state().globe.brush.mode);
    open_group(h, group);
}

/// Chooses a brush mode in the flyout of its group.
fn choose_mode(h: &mut Pad, name: &str) {
    open_group(h, group_of(name));
    tap(h, name);
}

/// The place of the draw cell: the third cell of the tool strip.
fn draw_cell(h: &Pad) -> Rect {
    let sculpt = control(h, SCULPT).unwrap();
    if h.state().pad.rects.unwrap().row {
        sculpt.translate(vec2(2.0 * sculpt.width(), 0.0))
    } else {
        sculpt.translate(vec2(0.0, 2.0 * sculpt.height()))
    }
}

/// The card with the controls of the tool: the row above the tool strip
/// in the row layout, and else the tool column.
fn context_card(h: &Pad) -> Rect {
    let rects = h.state().pad.rects.unwrap();
    rects.brush.unwrap_or(rects.tools)
}

/// The rectangles of all cards on the screen.
fn cards(h: &Pad) -> Vec<Rect> {
    let pad = &h.state().pad;
    let r = pad.rects.unwrap();
    let mut cards = vec![r.workspace, r.actions, r.tools];
    cards.extend(pad.view_rect);
    cards.extend(pad.view_menu_rect);
    cards.extend(pad.place_rect);
    cards.extend(r.brush);
    cards.extend(pad.panels.iter().map(|(_, rect)| *rect));
    cards.extend(pad.menu_rect);
    cards.extend(pad.flyouts.rect);
    cards.extend(pad.add_rect);
    cards
}

#[test]
fn the_canvas_fills_the_screen_and_the_panels_float_over_it() {
    for size in BOARDS {
        let mut h = pad(size);
        let screen = Rect::from_min_size(Pos2::ZERO, size.into());
        assert_eq!(h.state().globe.rect, screen, "{size:?}");
        open_panel(&mut h, "Layers");
        if h.state().pad.rects.unwrap().class != WidthClass::Compact {
            tap(&mut h, "Brush settings");
        }
        assert!(!h.state().pad.panels.is_empty());
        // The dock floats over the canvas, so the globe stays in place.
        assert_eq!(h.state().globe.rect, screen, "{size:?}");
        for card in cards(&h) {
            assert!(screen.contains_rect(card), "{size:?}: {card:?}");
        }
    }
}

/// Makes sure that a finger can hit each control of the frame.
fn check_controls(h: &Pad, what: &str) {
    let rects = h.state().pad.rects.unwrap();
    let screen = rects.screen;
    let controls = &h.state().pad.controls;
    // A sheet at half or full height covers the rows of the tool strip.
    let least = if rects.row && rects.sheet.is_some() && rects.tool_cells == 0 {
        3
    } else {
        6
    };
    assert!(
        controls.len() >= least,
        "{what}: {} controls",
        controls.len()
    );
    let in_menu = h.state().pad.menu_rect.is_some();
    let in_modes = h.state().pad.flyouts.rect.is_some();
    let in_add = h.state().pad.add_rect.is_some();
    let in_panels = h.state().pad.panels_menu_rect.is_some();
    let in_list = h.state().pad.projections_rect.is_some();
    let in_view = h.state().pad.view_menu_rect.is_some();
    let in_pane_menu = h.state().pad.pane_menu.is_some();
    let in_stop = h.state().pad.style.card.is_some() || h.state().pad.style.reset_card.is_some();
    let in_edit = controls
        .iter()
        .any(|c| c.name == vale_app::ui::pad::widgets::COPY);
    // The rows of the open menu of the values of a field.
    let choice_rows: Vec<String> = (h.state().selection_form.as_ref())
        .and_then(|form| {
            let open = form.open.as_ref()?;
            let i = form.fields.iter().position(|f| &f.name == open)?;
            let rows = form.choices[i].iter();
            Some(
                rows.map(|c| vale_app::ui::pad::widgets::choice_row_name(open, c))
                    .collect(),
            )
        })
        .unwrap_or_default();
    let popup = !choice_rows.is_empty()
        || in_edit
        || in_menu
        || in_modes
        || in_add
        || in_panels
        || in_list
        || in_view
        || in_pane_menu
        || in_stop;
    for (i, a) in controls.iter().enumerate() {
        let (name, rect) = (&a.name, a.rect);
        assert!(
            rect.width() >= 44.0 && rect.height() >= 44.0,
            "{what}: {name} is {rect:?}"
        );
        assert!(rect.top() >= BANDS.0, "{what}: {name} is in the top band");
        assert!(
            rect.bottom() <= screen.bottom() - BANDS.1,
            "{what}: {name} is in the bottom band"
        );
        assert!(
            rect.left() >= 0.0 && rect.right() <= screen.right(),
            "{what}: {name}"
        );
        for b in &controls[i + 1..] {
            assert_ne!(a.name, b.name, "{what}");
            // The open menu, the open list of projections, the open view
            // options, the open flyout of the brush modes, and the open menu
            // of the new layers cover the cards below them.
            let row = |c: &vale_app::ui::pad::widgets::Control| {
                let name = c.name.as_str();
                let kind = vale_sphere::ProjectionKind::ALL
                    .iter()
                    .any(|k| k.name() == name);
                let menu = ["World", "Atlas", WORLDS_ROW].contains(&name);
                let modes = in_modes
                    && (all_modes().any(|mode| mode == name)
                        || vale_app::ui::pad::POLYGON_MODES.iter().any(|m| m.0 == name)
                        || vale_app::ui::pad::VECTOR_TOOLS
                            .iter()
                            .any(|t| t.name == name));
                let rows = [
                    NEW_RASTER,
                    NEW_IMAGE,
                    NEW_GEOJSON,
                    "Lines",
                    "Points",
                    "Polygons",
                    NEW_HEIGHTMAP,
                ];
                let add = in_add && rows.contains(&name);
                let tiles = [
                    "Layers",
                    select::ATTRIBUTES,
                    "Toolbox",
                    "Brush",
                    panels_menu::CLOSE_ALL,
                    panels_menu::RESET_LAYOUT,
                ];
                let add = add || in_panels && tiles.contains(&name);
                let pane_menu = in_pane_menu
                    && (name.starts_with("Close ")
                        || name == vale_app::ui::pad::dock_ui::MOVE_TO_NEW_PANE
                        || name == vale_app::ui::pad::dock_ui::COLLAPSE_PANE
                        || name == vale_app::ui::pad::dock_ui::EXPAND_PANE);
                let add = add || pane_menu;
                let view = in_view && view_rows().contains(&name);
                let add = add || view;
                let stop = in_stop
                    && (name.starts_with(vale_app::ui::pad::raster::COLOR) || raster_stop(name));
                menu || kind || modes || add || stop || (in_menu && name == "Projection")
            };
            // The edit menu and the menu of the values of a field cover the
            // controls below them.
            let edit = |c: &vale_app::ui::pad::widgets::Control| {
                use vale_app::ui::pad::widgets::{COPY, CUT, PASTE};
                c.name == CUT || c.name == COPY || c.name == PASTE
            };
            // The value field of a vertical slider is at the top of the
            // slider, and it takes the touches there.
            let field = |a: &str, b: &str| b.strip_suffix(" value") == Some(a);
            let own = field(&a.name, &b.name) || field(&b.name, &a.name);
            let choice = |c: &vale_app::ui::pad::widgets::Control| choice_rows.contains(&c.name);
            let covered =
                own || popup && (row(a) != row(b) || edit(a) != edit(b) || choice(a) != choice(b));
            // The width handle is on the edge of the dock, over the left
            // ends of the rows of the panes, and it takes only a drag.
            let handle = |c: &vale_app::ui::pad::widgets::Control| {
                c.name == vale_app::ui::pad::dock_ui::DOCK_WIDTH
            };
            let covered = covered || handle(a) || handle(b);
            let free = !a.rect.shrink(0.5).intersects(b.rect.shrink(0.5));
            assert!(free || covered, "{what}: {name} and {} overlap", b.name);
        }
    }
}

/// The controls of the View popover.
fn view_rows() -> [&'static str; 6] {
    [VIEWS[0], VIEWS[1], GRATICULE, PROJECTION, RECENTER, RESET]
}

/// Opens the View popover of the readout and taps the control `name` in it.
fn tap_in_view(h: &mut Pad, name: &str) {
    if !h.state().pad.view_menu {
        tap(h, VIEW);
    }
    tap(h, name);
}

/// A control of the card of the selected stop.
fn raster_stop(name: &str) -> bool {
    use vale_app::ui::pad::raster;
    [
        raster::BAND_NAME,
        raster::STOP_VALUE,
        raster::FILL_COLOR,
        raster::LINE_COLOR,
        raster::LINE,
        raster::LINE_WEIGHT,
        raster::LINE_SOURCE,
        raster::DARKER,
        raster::CUSTOM,
        raster::DARKEN,
        raster::REMOVE_STOP,
        raster::RESET,
    ]
    .contains(&name)
}

#[test]
fn each_control_is_large_and_is_outside_the_system_bands() {
    for size in BOARDS {
        let mut h = painted_pad(size);
        check_controls(&h, &format!("{size:?}"));
        for name in [
            "Layers",
            "Open Height",
            select::ATTRIBUTES,
            "Brush settings",
        ] {
            let tile = name == "Layers" || name == select::ATTRIBUTES;
            if !tile && control(&h, name).is_none() {
                // The open sheets cover the Brush button.
                assert_eq!(name, "Brush settings");
                tap(&mut h, "Close Attributes");
                tap(&mut h, "Close Layers");
            }
            if tile {
                open_panel(&mut h, name);
            } else {
                tap(&mut h, name);
            }
            check_controls(&h, &format!("{size:?} after {name}"));
        }
        lower_sheet(&mut h);
        check_controls(&h, &format!("{size:?} with the sheet at its title row"));
        for group in ModeGroup::ALL {
            open_group(&mut h, group);
            let modes = h.state().pad.flyouts.rect.unwrap();
            for (name, ..) in group.modes() {
                let row = control(&h, name).unwrap_or_else(|| panic!("{size:?}: {name}"));
                assert!(modes.contains_rect(row), "{size:?}: {name}");
            }
            let what = format!("{size:?} with the {} modes", group.name());
            check_controls(&h, &what);
            quick_tap(&mut h, group.name());
            assert_eq!(h.state().pad.flyouts.open, None, "{size:?}");
        }
        if control(&h, "Workspace").is_some() {
            tap(&mut h, "Workspace");
            assert!(h.state().pad.menu);
            check_controls(&h, &format!("{size:?} with the menu"));
            h.state_mut().pad.menu = false;
        }
        tap(&mut h, PANELS);
        assert!(h.state().pad.panels_menu, "{size:?}");
        check_controls(&h, &format!("{size:?} with the Panels menu"));
        h.state_mut().pad.panels_menu = false;

        // The list of the layers, the menu of the new layers, and the panel
        // of a vector layer.
        let pad = &mut h.state_mut().pad;
        pad.close(Panel::Brush);
        pad.layer = None;
        pad.open(Panel::Layers);
        h.run_steps(2);
        for name in [
            "Layer Height",
            "Layer Land",
            "Hide Rivers",
            "Open Places",
            ADD_LAYER,
        ] {
            assert!(control(&h, name).is_some(), "{size:?}: {name}");
        }
        check_controls(&h, &format!("{size:?} with the layer list"));
        tap(&mut h, ADD_LAYER);
        let menu = h.state().pad.add_rect.unwrap();
        for name in ["Lines", "Points", "Polygons"] {
            let row = control(&h, name).unwrap();
            assert!(menu.contains_rect(row), "{size:?}: {name}");
        }
        check_controls(&h, &format!("{size:?} with the menu of the new layers"));
        tap(&mut h, "Lines");
        for name in [LAYER_NAME, "Back to Layers"] {
            assert!(control(&h, name).is_some(), "{size:?}: {name}");
        }
        check_controls(&h, &format!("{size:?} with the panel of a layer"));
    }
}

/// The name of a raster layer, from its layer record.
fn raster_name(h: &Pad, id: vale_store::RasterId) -> String {
    let layer = h.state().doc.project.raster_layer(id);
    layer.unwrap().name.clone()
}

/// The vector layer with a name.
fn vector(h: &Pad, name: &str) -> vale_store::LayerId {
    let layers = h.state().doc.project.layers();
    let layer = layers.iter().find(|l| l.name == name);
    layer.unwrap_or_else(|| panic!("no layer {name}")).id
}

/// The raster layers that the globe draws, from the bottom.
fn drawn(h: &Pad) -> Vec<vale_store::RasterId> {
    let stack = h.state().globe.stack().unwrap_or_default();
    let ids = stack.iter().filter_map(|item| match item {
        StackItem::Raster { id, .. } => Some(*id),
        StackItem::Vector(_) | StackItem::Image { .. } => None,
    });
    ids.collect()
}

/// The raster layer of the readout.
fn shown(h: &Pad) -> Option<vale_store::RasterId> {
    h.state().shown_raster().map(|raster| raster.id)
}

#[test]
fn the_layers_panel_adds_selects_and_hides_raster_layers() {
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = empty_pad(size);
        h.state_mut().form_face_size = Some(64);
        // With no raster layer, the brush cells are off and the tool is Move.
        for name in [SCULPT, TERRAIN, VECTOR] {
            assert!(control(&h, name).is_none(), "{what}: {name}");
        }
        assert!(control(&h, "Pan").is_some(), "{what}");
        open_panel(&mut h, "Layers");
        assert!(control(&h, "Layer Height").is_none(), "{what}");
        assert!(control(&h, IMPORT_HEIGHTMAP).is_none(), "{what}");

        // The last row of the section "New" opens the form of a raster
        // layer. The form adds a layer, selects it, and opens its panel.
        tap(&mut h, ADD_LAYER);
        let (raster, polygons) = (control(&h, NEW_RASTER), control(&h, "Polygons"));
        assert!(polygons.unwrap().bottom() <= raster.unwrap().top() + 0.5);
        check_controls(&h, &format!("{what} with the menu of the new layers"));
        tap(&mut h, NEW_RASTER);
        assert!(!h.state().pad.add && h.state().raster_form.is_some());
        tap(&mut h, "Height");
        tap(&mut h, raster_form::ADD);
        let rasters = h.state().doc.project.rasters();
        assert_eq!(rasters.len(), 1, "{what}");
        let first = rasters[0].id;
        let record = &rasters[0];
        assert_eq!(record.unit, "m");
        assert_eq!((record.value_min, record.value_max), (-6000.0, 6000.0));
        assert_eq!((record.face_size, record.opacity), (64, 1.0));
        assert_eq!(raster_name(&h, first), "Height");
        assert_eq!(h.state().pad.layer, Some(LayerRef::Raster(first)));
        assert_eq!(h.state().selected_layer, Some(LayerRef::Raster(first)));
        assert_eq!(h.state().globe.raster(), Some(first));
        check_controls(&h, &format!("{what} with the panel of a raster layer"));
        tap(&mut h, "Back to Layers");
        tap(&mut h, ADD_LAYER);
        tap(&mut h, NEW_RASTER);
        tap(&mut h, raster_form::ADD);
        tap(&mut h, "Back to Layers");
        let second = h.state().doc.project.rasters()[1].id;
        assert_eq!(raster_name(&h, second), "Raster");
        assert_eq!(h.state().selected_layer, Some(LayerRef::Raster(second)));

        // The list has one row for each layer of each kind, in the draw
        // order. A new raster layer is above the top raster layer.
        assert_eq!(
            layer_names(&h),
            ["Height", "Raster", "Land", "Rivers", "Places"],
            "{what}"
        );
        check_controls(&h, &format!("{what} with two raster layers"));
        let top = |name: &str| control(&h, name).unwrap().top();
        let names = layer_names(&h);
        let rows: Vec<f32> = names.iter().map(|n| top(&format!("Layer {n}"))).collect();
        assert!(rows.is_sorted() && rows[0] < rows[4], "{what}: {rows:?}");
        for name in &names {
            for control_name in [format!("Hide {name}"), format!("Open {name}")] {
                assert!(
                    control(&h, &control_name).is_some(),
                    "{what}: {control_name}"
                );
            }
        }

        // A tap selects a layer, and the globe shows it.
        tap(&mut h, "Layer Height");
        assert_eq!(h.state().selected_layer, Some(LayerRef::Raster(first)));
        assert_eq!(h.state().globe.raster(), Some(first));
        assert!(h.state().globe.paintable, "{what}");

        // The globe draws the two raster layers in the draw order, and the
        // selection does not change that.
        assert_eq!(drawn(&h), [first, second], "{what}");

        // The eye hides the layer. The brush does not paint a hidden layer,
        // and the globe draws the other layer. The readout has the top
        // visible raster layer.
        tap(&mut h, "Hide Height");
        let layer = h.state().doc.project.raster_layer(first).unwrap();
        assert!(!layer.visible, "{what}");
        assert_eq!(h.state().selected_layer, Some(LayerRef::Raster(first)));
        assert_eq!(h.state().globe.raster(), Some(first));
        assert_eq!((drawn(&h), shown(&h)), (vec![second], Some(second)));
        assert!(!h.state().globe.paintable, "{what}");
        tap(&mut h, "Hide Raster");
        assert_eq!((drawn(&h), shown(&h)), (vec![], None), "{what}");
        tap(&mut h, "Show Height");
        tap(&mut h, "Show Raster");
        assert_eq!(h.state().globe.raster(), Some(first));
        assert_eq!((drawn(&h), shown(&h)), (vec![first, second], Some(first)));
        assert!(h.state().globe.paintable, "{what}");

        // The brush does not paint a vector layer. The readout then has the
        // top raster layer of the draw order.
        tap(&mut h, "Layer Rivers");
        assert_eq!(shown(&h), Some(second));
        assert!(!h.state().globe.paintable, "{what}");
        let height = h.state().doc.project.raster_layer(first).unwrap().id;
        assert!(h.state_mut().move_layer(height, 4));
        h.run_steps(2);
        assert_eq!((drawn(&h), shown(&h)), (vec![second, first], Some(first)));

        // The app takes a raster layer away, with its row.
        assert!(h.state_mut().remove_raster(second));
        h.run_steps(2);
        assert!(control(&h, "Layer Raster").is_none(), "{what}");
        assert_eq!(layer_names(&h), ["Land", "Rivers", "Places", "Height"]);
        assert_eq!(h.state().globe.raster(), Some(first));
    }
}

#[test]
fn the_brush_cells_are_on_for_a_selected_raster_layer_only() {
    let mut h = pad(BOARDS[0]);
    let cells = [SCULPT, TERRAIN, "Size", "Flow"];
    let on = |h: &Pad| cells.map(|name| control(h, name).is_some());
    assert_eq!(on(&h), [true; 4]);
    assert_eq!(h.state().globe.tool, Tool::Brush);
    h.state_mut().select(None);
    h.run_steps(2);
    assert_eq!(on(&h), [false; 4]);
    // The cells stay in their places, and the Move cell is the tool.
    let tools = h.state().pad.rects.unwrap().tools;
    assert!(tools.contains_rect(control(&h, "Pan").unwrap()));
    assert!(!h.state().globe.brushing());
    let height = h.state().doc.project.rasters()[0].id;
    h.state_mut().select(Some(LayerRef::Raster(height)));
    h.run_steps(2);
    assert_eq!(on(&h), [true; 4]);
    assert!(h.state().globe.brushing());

    // A raster layer of another unit has no mode that needs heights.
    let rain = vale_app::ui::RasterSpec {
        name: "Rain".to_string(),
        unit: "mm".to_string(),
        min: 0.0,
        max: 4000.0,
        face_size: 256,
    };
    h.state_mut().globe.brush.mode = Mode::Carve;
    h.state_mut().add_raster(rain).unwrap();
    h.run_steps(2);
    assert_eq!(h.state().globe.brush.mode, Mode::Raise);
    assert!(!h.state().globe.is_height());
    // The Terrain cell is off, and the Sculpt cell stays on.
    assert!(control(&h, TERRAIN).is_none());
    assert!(disabled(&h, TERRAIN));
    assert!(!disabled(&h, SCULPT));
    let terrain = control(&h, SCULPT).unwrap().translate(vec2(0.0, 52.0));
    finger_tap(&mut h, terrain.center());
    assert_eq!(h.state().pad.flyouts.open, None);
    assert_eq!(h.state().globe.brush.mode, Mode::Raise);
    open_modes(&mut h);
    assert_eq!(
        h.state().pad.flyouts.open,
        Some(Flyout::Brush(ModeGroup::Sculpt))
    );
    for (name, ..) in ModeGroup::Sculpt.modes() {
        assert!(control(&h, name).is_some(), "{name}");
    }
    tap(&mut h, "Warp");
    assert_eq!(h.state().globe.brush.mode, Mode::Warp);
}

/// Opens the form of a new raster layer from the Layers panel.
fn open_raster_form(h: &mut Pad) {
    if control(h, "Back to Layers").is_some() {
        tap(h, "Back to Layers");
    }
    tap(h, ADD_LAYER);
    tap(h, NEW_RASTER);
    assert!(h.state().raster_form.is_some());
}

#[test]
fn a_new_raster_layer_has_the_picked_face_size() {
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = empty_pad(size);
        // The face size of `--face-size` is in the list, and it is on.
        h.state_mut().form_face_size = Some(64);
        open_panel(&mut h, "Layers");
        open_raster_form(&mut h);
        let form = h.state().raster_form.clone().unwrap();
        assert_eq!(
            (form.sizes.as_slice(), form.face_size),
            (&[64, 1024, 2048, 4096, 8192][..], 64)
        );
        assert_eq!((form.name.as_str(), form.height()), ("Raster", false));
        check_controls(&h, &format!("{what} with the form of a raster layer"));
        for size in [64, 1024, 2048, 4096, 8192] {
            assert!(
                control(&h, &face_size_name(size)).is_some(),
                "{what}: {size}"
            );
        }

        tap(&mut h, &face_size_name(1024));
        assert_eq!(h.state().raster_form.as_ref().unwrap().face_size, 1024);
        tap(&mut h, raster_form::ADD);
        assert!(h.state().raster_form.is_none(), "{what}");
        let state = h.state();
        let rasters = state.doc.project.rasters();
        assert_eq!(rasters.len(), 1, "{what}");
        let first = rasters[0].id;
        assert_eq!(rasters[0].face_size, 1024, "{what}");
        assert_eq!(state.globe.raster_map(first).unwrap().face_size(), 1024);
        assert_eq!(state.pad.layer, Some(LayerRef::Raster(first)));
        assert_eq!(state.selected_layer, Some(LayerRef::Raster(first)));
        assert_eq!(state.globe.raster(), Some(first));

        // The next layer has the size that is on at first.
        open_raster_form(&mut h);
        assert_eq!(h.state().raster_form.as_ref().unwrap().name, "Raster 2");
        tap(&mut h, raster_form::ADD);
        let second = h.state().doc.project.rasters()[1].clone();
        assert_eq!(raster_name(&h, second.id), "Raster 2");
        assert_eq!(second.face_size, 64);
        let state = h.state();
        assert_eq!(state.globe.raster_map(second.id).unwrap().face_size(), 64);
        assert_eq!(state.globe.raster_map(first).unwrap().face_size(), 1024);

        // The close button adds no layer.
        open_raster_form(&mut h);
        tap(&mut h, &format!("Close {}", raster_form::TITLE));
        assert!(h.state().raster_form.is_none());
        assert_eq!(h.state().doc.project.rasters().len(), 2, "{what}");
    }
}

#[test]
fn the_form_offers_four_face_sizes_and_the_preset_gives_the_default() {
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = empty_pad(size);
        open_panel(&mut h, "Layers");
        open_raster_form(&mut h);
        // The form opens on the custom preset.
        let form = h.state().raster_form.clone().unwrap();
        assert_eq!(form.sizes, [1024, 2048, 4096, 8192], "{what}");
        assert_eq!(PRESETS[form.preset].name, "Custom");
        assert_eq!(form.face_size, 2048, "{what}");
        check_controls(&h, &format!("{what} with the four face sizes"));
        let screen = h.state().pad.rects.unwrap().screen;
        for preset in PRESETS {
            let chip = control(&h, preset.name).unwrap();
            assert!(screen.contains_rect(chip), "{what}: {}", preset.name);
        }
        let top = |size: &usize| control(&h, &face_size_name(*size)).unwrap().top();
        let tops: Vec<f32> = form.sizes.iter().map(top).collect();
        assert!(tops.is_sorted(), "{what}: {tops:?}");

        // The face size follows the preset.
        for preset in PRESETS {
            tap(&mut h, preset.name);
            let form = h.state().raster_form.as_ref().unwrap();
            assert_eq!(form.face_size, preset.face_size, "{}", preset.name);
        }
        let sizes: Vec<usize> = PRESETS.iter().map(|preset| preset.face_size).collect();
        assert_eq!(sizes, [8192, 2048, 2048, 2048, 2048]);

        // A face size that the user picks is the size of the layer.
        tap(&mut h, "Precipitation");
        tap(&mut h, &face_size_name(1024));
        tap(&mut h, raster_form::ADD);
        let rain = h.state().doc.project.rasters()[0].clone();
        assert_eq!((rain.face_size, rain.unit.as_str()), (1024, "mm"), "{what}");
    }

    // With `--face-size`, that size is on for each preset.
    let mut state = state();
    state.form_face_size = Some(256);
    let mut form = RasterForm::open(&state);
    for index in 0..PRESETS.len() {
        form.set_preset(index, &state);
        assert_eq!(form.face_size, 256);
    }
    // A device with a small limit has no larger face size.
    let sizes = raster_form::face_sizes(None, 4096);
    assert_eq!(sizes, [1024, 2048, 4096]);
    assert_eq!(raster_form::first_face_size(&sizes, 8192), 4096);
}

#[test]
fn each_preset_adds_a_layer_with_its_unit_and_its_range() {
    let mut h = empty_pad(BOARDS[0]);
    h.state_mut().form_face_size = Some(64);
    open_panel(&mut h, "Layers");
    let wanted = [
        ("Height", "m", (-6000.0, 6000.0)),
        ("Precipitation", "mm", (0.0, 4000.0)),
        ("Temperature", "°C", (-60.0, 60.0)),
        ("Pressure", "hPa", (950.0, 1050.0)),
        ("Custom", "", (0.0, 100.0)),
    ];
    for (index, (preset, unit, range)) in wanted.into_iter().enumerate() {
        open_raster_form(&mut h);
        tap(&mut h, preset);
        // The name follows the preset. The unit and the range of each
        // preset but Height are fields.
        let name = if preset == "Custom" { "Raster" } else { preset };
        let form = h.state().raster_form.clone().unwrap();
        assert_eq!((form.name.as_str(), form.unit.as_str()), (name, unit));
        let fields = control(&h, raster_form::UNIT).is_some()
            && control(&h, raster_form::MINIMUM).is_some()
            && control(&h, raster_form::MAXIMUM).is_some();
        assert_eq!(fields, preset != "Height", "{preset}");
        check_controls(&h, &format!("the form of the preset {preset}"));
        tap(&mut h, raster_form::ADD);
        let state = h.state();
        let rasters = state.doc.project.rasters();
        assert_eq!(rasters.len(), index + 1, "{preset}");
        let raster = &rasters[index];
        assert_eq!(raster.unit, unit, "{preset}");
        assert_eq!((raster.value_min, raster.value_max), range, "{preset}");
        assert_eq!(raster.face_size, 64, "{preset}");
        assert_eq!(raster_name(&h, raster.id), name);
        // The bands of the layer have stops in the range.
        let bands = &state.globe.raster_map(raster.id).unwrap().bands;
        assert_eq!((bands.min(), bands.max()), range, "{preset}");
        let stops = bands.stops();
        let inner: Vec<f64> = stops[1..stops.len() - 1].iter().map(|s| s.value).collect();
        assert!(inner.len() >= 4, "{preset}: {inner:?}");
        let inside = |value: &f64| *value > range.0 && *value < range.1;
        assert!(inner.iter().all(inside), "{preset}: {inner:?}");
        // Only the height preset gives a height layer. It has a sea split,
        // and it starts as a sea. Each other layer has one ramp, and it
        // starts at the minimum of its range.
        let height = preset == "Height";
        assert_eq!(state.globe.is_height(), height, "{preset}");
        assert_eq!(bands.sea().is_some(), height, "{preset}");
        let map = state.globe.raster_map(raster.id).unwrap();
        let start = if height { -2500.0 } else { range.0 };
        assert_eq!(map.base(), bands.level(start), "{preset}");
        assert_eq!(map.allocated_tiles(), 0, "{preset}");
    }

    // A name that the user typed stays when the preset changes.
    open_raster_form(&mut h);
    type_text(&mut h, raster_form::NAME, "Rain");
    tap(&mut h, "Precipitation");
    let form = h.state().raster_form.clone().unwrap();
    assert_eq!((form.name.as_str(), form.unit.as_str()), ("Rain", "mm"));
    // A preset is a start only: the unit and the range are free.
    type_text(&mut h, raster_form::UNIT, "cm");
    type_text(&mut h, raster_form::MAXIMUM, "400");
    tap(&mut h, raster_form::ADD);
    let rain = h.state().doc.project.rasters()[5].clone();
    assert_eq!((rain.unit.as_str(), rain.value_max), ("cm", 400.0));
}

#[test]
fn the_form_shows_the_values_next_to_each_face_size() {
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = empty_pad(size);
        open_panel(&mut h, "Layers");
        open_raster_form(&mut h);
        let card = |h: &Pad, name: &str| control(h, name).unwrap();
        let shows = |h: &Pad, text: &str, row: Rect| {
            let label = h.get_by_label(text).rect();
            assert!(
                row.expand(1.0).contains_rect(label),
                "{what}: {text} at {label:?}"
            );
        };
        let (small, large) = (face_size_name(4096), face_size_name(8192));
        shows(&h, "2.4 km per pixel", card(&h, &small));
        shows(&h, "16,384 × 8,192 · 201 MB", card(&h, &small));
        shows(&h, "1.2 km per pixel", card(&h, &large));
        shows(&h, "32,768 × 16,384 · 805 MB", card(&h, &large));

        // The ground of a pixel follows the radius and the units of the
        // world.
        h.state_mut().doc.set_radius_km(6371.0 / 2.0);
        h.run_steps(2);
        shows(&h, "1.2 km per pixel", card(&h, &small));
        shows(&h, "610 m per pixel", card(&h, &large));
        shows(&h, "32,768 × 16,384 · 805 MB", card(&h, &large));
        h.state_mut().doc.set_radius_km(6371.0);
        h.state_mut().doc.project.world.units = Units::Imperial;
        h.run_steps(2);
        shows(&h, "1.5 mi per pixel", card(&h, &small));
    }
}

#[test]
fn the_form_adds_a_raster_layer_of_another_unit() {
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        h.state_mut().form_face_size = Some(64);
        open_panel(&mut h, "Layers");
        open_raster_form(&mut h);
        // The form opens on the custom preset, with a plain value.
        let form = h.state().raster_form.clone().unwrap();
        assert_eq!((form.name.as_str(), form.height()), ("Raster", false));
        assert_eq!(
            (form.unit.as_str(), form.min.as_str(), form.max.as_str()),
            ("", "0", "100")
        );
        // The height layer of the world has the name Height. A height has
        // a fixed unit and a fixed range.
        tap(&mut h, "Height");
        assert_eq!(h.state().raster_form.as_ref().unwrap().name, "Height 2");
        for name in [
            raster_form::UNIT,
            raster_form::MINIMUM,
            raster_form::MAXIMUM,
        ] {
            assert!(control(&h, name).is_none(), "{what}: {name}");
        }
        tap(&mut h, "Precipitation");
        let form = h.state().raster_form.clone().unwrap();
        assert_eq!(
            (form.name.as_str(), form.height()),
            ("Precipitation", false)
        );
        assert_eq!(
            (form.unit.as_str(), form.min.as_str(), form.max.as_str()),
            ("mm", "0", "4000")
        );
        check_controls(&h, &format!("{what} with the form of another raster layer"));
        let screen = h.state().pad.rects.unwrap().screen;
        for name in [raster_form::UNIT, raster_form::MAXIMUM, raster_form::ADD] {
            assert!(
                screen.contains_rect(control(&h, name).unwrap()),
                "{what}: {name}"
            );
        }

        // The form does not add a layer with an empty range.
        type_text(&mut h, raster_form::MAXIMUM, "0");
        assert!(control(&h, raster_form::ADD).is_none(), "{what}");
        assert!(
            h.query_by_label("Maximum must be greater than minimum.")
                .is_some()
        );
        type_text(&mut h, raster_form::MAXIMUM, "2500");
        // A plain value has no unit.
        type_text(&mut h, raster_form::UNIT, " ");
        assert!(control(&h, raster_form::ADD).is_some(), "{what}");
        assert_eq!(h.state().doc.project.rasters().len(), 1);

        type_text(&mut h, raster_form::UNIT, "°C");
        type_text(&mut h, raster_form::MINIMUM, "-50");
        type_text(&mut h, raster_form::MAXIMUM, "50");
        type_text(&mut h, raster_form::NAME, "Heat");
        tap(&mut h, raster_form::ADD);
        let heat = h.state().doc.project.rasters()[1].clone();
        assert_eq!(raster_name(&h, heat.id), "Heat");
        assert_eq!(heat.unit, "°C");
        assert_eq!(
            (heat.value_min, heat.value_max, heat.face_size),
            (-50.0, 50.0, 64)
        );
        let state = h.state();
        assert_eq!(state.selected_layer, Some(LayerRef::Raster(heat.id)));
        assert!(!state.globe.is_height());
        check_controls(
            &h,
            &format!("{what} with the panel of another raster layer"),
        );
    }
}

#[test]
fn the_values_of_a_raster_layer_have_the_unit_and_the_range_of_the_layer() {
    let mut h = pad(BOARDS[0]);
    let height = h.state().doc.project.rasters()[0].id;
    let rain = vale_app::ui::RasterSpec {
        name: "Rain".to_string(),
        unit: "mm".to_string(),
        min: 0.0,
        max: 4000.0,
        face_size: 64,
    };
    let rain = h.state_mut().add_raster(rain).unwrap();
    h.run_steps(2);

    // The readout, the limits, the flatten level, and the strength of the
    // brush are values of the layer that the globe shows.
    let state = h.state_mut();
    assert_eq!(state.globe.raster(), Some(rain));
    assert_eq!(state.shown_unit(), "mm");
    let value = state.value_text(1234.4);
    assert_eq!(
        readout_text(Some([24.6, 31.2]), Some(&value)),
        "24.6°E 31.2°N · 1234 mm"
    );
    let bands = state.globe.map.bands.clone();
    let brush = &mut state.globe.brush;
    assert_eq!(brush.value_range, (0.0, 4000.0));
    (brush.floor_on, brush.floor_m) = (true, 1000.0);
    (brush.ceiling_on, brush.ceiling_m) = (true, 3000.0);
    let limits = brush.limits();
    assert_eq!(
        (limits.floor, limits.ceiling),
        (bands.level(1000.0), bands.level(3000.0))
    );
    assert_eq!(limits.floor, 16384);
    brush.flatten_level = Some(brush.level(2000.0));
    assert_eq!(brush.flatten_level, Some(bands.level(2000.0)));
    assert!((brush.value(bands.level(2000.0)) - 2000.0).abs() < 0.1);
    brush.strength_m = 1500.0;
    assert_eq!(brush.strength_value(), 500.0);

    // The levels of the Brush panel go over the range of the layer, in 240
    // steps.
    tap(&mut h, "Brush settings");
    type_value(&mut h, "Brush floor level", "9000");
    assert_eq!(h.state().globe.brush.floor_m, 4000.0);
    type_value(&mut h, "Brush floor level", "0");
    assert_eq!(h.state().globe.brush.floor_m, 0.0);
    step(&mut h, "Brush floor level", true);
    let floor = h.state().globe.brush.floor_m;
    assert!((floor - 4000.0 / 240.0).abs() < 1e-9, "{floor}");

    // A height layer has meters, as before.
    h.state_mut().select(Some(LayerRef::Raster(height)));
    h.run_steps(2);
    let state = h.state_mut();
    assert_eq!(state.shown_unit(), "m");
    let value = state.value_text(-120.4);
    assert_eq!(
        readout_text(Some([-70.04, -8.0]), Some(&value)),
        "70.0°W 8.0°S · -120 m"
    );
    let brush = &mut state.globe.brush;
    assert_eq!(brush.value_range, (-6000.0, 6000.0));
    brush.floor_m = 0.0;
    assert_eq!(brush.limits().floor, meters_to_level(0.0));
    assert_eq!(brush.level(1500.0), meters_to_level(1500.0));
    assert_eq!(brush.strength_value(), 1500.0);
}

#[test]
fn the_panel_of_a_raster_layer_renames_the_layer_and_a_swipe_deletes_it() {
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let dir = tempfile::tempdir().unwrap();
        let mut h = pad(size);
        let state = h.state_mut();
        state.session = Some(Session::open(&dir.path().join("w.gpkg"), &mut state.doc).unwrap());
        let height = state.doc.project.rasters()[0].id;
        let level = meters_to_level(100.0);
        state.globe.map.set(0, 0, 0, level);
        state.save_waiting().unwrap();
        h.run_steps(2);

        open_panel(&mut h, "Layers");
        tap(&mut h, "Open Height");
        check_controls(&h, &format!("{what} with the panel of a raster layer"));
        type_text(&mut h, LAYER_NAME, "Ground");
        assert_eq!(raster_name(&h, height), "Ground");
        assert!(h.state().pad.is_open(Panel::Layers), "{what}");
        // A blank name changes nothing.
        type_text(&mut h, LAYER_NAME, "  ");
        assert_eq!(raster_name(&h, height), "Ground");

        tap(&mut h, "Back to Layers");
        swipe_and_delete(&mut h, "Ground");
        let state = h.state();
        assert!(state.doc.project.rasters().is_empty(), "{what}");
        assert!(state.globe.raster_map(height).is_none());
        assert_eq!((state.pad.layer, state.selected_layer), (None, None));
        assert!(control(&h, "Layer Ground").is_none(), "{what}");

        // One undo puts back the layer and its tiles.
        h.state_mut().undo();
        h.run_steps(2);
        let state = h.state();
        assert_eq!(state.globe.raster_map(height).unwrap().get(0, 0, 0), level);
        assert_eq!(raster_name(&h, height), "Ground");
        assert!(control(&h, "Layer Ground").is_some(), "{what}");
    }
}

fn layer_names(h: &Pad) -> Vec<String> {
    let layers = h.state().doc.project.layers();
    layers.iter().map(|l| l.name.clone()).collect()
}

#[test]
fn you_make_rename_hide_and_delete_a_layer() {
    for size in [BOARDS[0], BOARDS[3]] {
        let mut h = pad(size);
        open_panel(&mut h, "Layers");
        tap(&mut h, ADD_LAYER);
        assert!(h.state().pad.add);
        let (menu, layers) = (
            h.state().pad.add_rect.unwrap(),
            panel(&h, Panel::Layers).unwrap(),
        );
        if size == BOARDS[0] {
            // The menu is at the left of the panel, with its top at the top
            // of the panel.
            assert_eq!(menu.right_top(), layers.left_top() - vec2(8.0, 0.0));
        } else {
            assert!(layers.contains_rect(menu), "{menu:?}");
        }
        assert_eq!(menu.width(), 220.0);

        // A new layer goes on top, and its panel opens.
        tap(&mut h, "Polygons");
        assert!(!h.state().pad.add && h.state().pad.add_rect.is_none());
        assert_eq!(
            layer_names(&h),
            ["Height", "Land", "Rivers", "Places", "Polygons 1"],
            "{size:?}"
        );
        let id = vector(&h, "Polygons 1");
        assert_eq!(h.state().pad.layer, Some(LayerRef::Vector(id)));
        assert_eq!(
            h.state().doc.layer(id).unwrap().geometry(),
            Some(vale_store::GeometryKind::Polygon)
        );
        assert!(h.state().pad.is_open(Panel::Layers));

        type_text(&mut h, LAYER_NAME, "Coast");
        assert_eq!(h.state().doc.layer(id).unwrap().name, "Coast");
        assert!(h.state().pad.is_open(Panel::Layers));

        tap(&mut h, "Back to Layers");
        assert_eq!(h.state().pad.layer, None);
        tap(&mut h, "Hide Coast");
        assert!(!h.state().doc.visible(id));
        assert!(control(&h, "Hide Coast").is_none());
        tap(&mut h, "Show Coast");
        assert!(h.state().doc.visible(id));

        swipe(&mut h, "Coast", -120.0);
        tap(&mut h, "Delete Coast");
        assert_eq!(
            layer_names(&h),
            ["Height", "Land", "Rivers", "Places"],
            "{size:?}"
        );
        assert_eq!(h.state().pad.layer, None);
        assert!(control(&h, ADD_LAYER).is_some());
        assert!(control(&h, "Open Coast").is_none());
    }
}

#[test]
fn a_press_outside_closes_the_menu_of_the_new_layers() {
    let mut h = pad(BOARDS[0]);
    open_panel(&mut h, "Layers");
    tap(&mut h, ADD_LAYER);
    assert!(h.state().pad.add);
    tap(&mut h, ADD_LAYER);
    assert!(!h.state().pad.add);
    tap(&mut h, ADD_LAYER);
    let pos = pos2(400.0, 500.0);
    touch(&mut h, egui::TouchPhase::Start, pos, None);
    h.step();
    touch(&mut h, egui::TouchPhase::End, pos, None);
    h.run_steps(2);
    assert!(!h.state().pad.add);
    assert_eq!(layer_names(&h).len(), 4);
}

#[test]
fn the_arrow_of_a_row_opens_the_panel_of_the_layer() {
    for size in BOARDS {
        let mut h = painted_pad(size);
        open_panel(&mut h, "Layers");
        let list = panel(&h, Panel::Layers).unwrap();
        let rivers = vector(&h, "Rivers");
        tap(&mut h, "Open Rivers");
        assert_eq!(h.state().pad.layer, Some(LayerRef::Vector(rivers)));
        assert_eq!(h.state().selected_layer, Some(LayerRef::Vector(rivers)));
        let open = panel(&h, Panel::Layers).unwrap();
        assert_eq!(open.left_top(), list.left_top(), "{size:?}");
        assert_eq!(open.width(), list.width(), "{size:?}");
        for name in ["Back to Layers", LAYER_NAME] {
            assert!(control(&h, name).is_some(), "{size:?}: {name}");
        }
        assert!(control(&h, ADD_LAYER).is_none());
        tap(&mut h, "Back to Layers");
        tap(&mut h, "Open Height");
        let height = h.state().doc.project.rasters()[0].id;
        assert_eq!(h.state().pad.layer, Some(LayerRef::Raster(height)));
        assert_eq!(h.state().selected_layer, Some(LayerRef::Raster(height)));
        assert!(h.state().pad.is_open(Panel::Layers));

        // The panel of a layer closes when an undo takes the layer away.
        h.state_mut().pad.layer = Some(LayerRef::Vector(rivers));
        h.state_mut().doc.remove_layer(rivers);
        h.run_steps(2);
        assert_eq!(h.state().pad.layer, None);
    }
}

/// Holds a finger on a row of the layer list until the row lifts.
fn hold(h: &mut Pad, name: &str) -> Pos2 {
    let pos = control(h, name).unwrap().center();
    touch(h, egui::TouchPhase::Start, pos, None);
    for _ in 0..200 {
        h.step();
        if h.state().pad.reorder.is_some() {
            break;
        }
    }
    pos
}

#[test]
fn a_hold_and_a_drag_change_the_order() {
    for size in [BOARDS[0], BOARDS[3]] {
        let mut h = pad(size);
        open_panel(&mut h, "Layers");
        let land = vector(&h, "Land");
        let places = control(&h, "Layer Places").unwrap().center();
        let from = hold(&mut h, "Layer Land");
        assert_eq!(h.state().pad.reorder.map(|r| r.layer), Some(land));
        // The order changes when the finger goes up.
        for i in 1..=6 {
            let pos = from.lerp(places + vec2(0.0, 6.0), i as f32 / 6.0);
            touch(&mut h, egui::TouchPhase::Move, pos, None);
            h.step();
        }
        assert_eq!(layer_names(&h), ["Height", "Land", "Rivers", "Places"]);
        touch(&mut h, egui::TouchPhase::End, places, None);
        h.run_steps(3);
        assert_eq!(h.state().pad.reorder, None);
        assert_eq!(
            layer_names(&h),
            ["Height", "Rivers", "Places", "Land"],
            "{size:?}"
        );
        assert_eq!(
            control(&h, "Layer Land").unwrap().center().y,
            places.y,
            "{size:?}"
        );

        // The row goes to the first place, below the raster layer.
        let rivers = control(&h, "Layer Rivers").unwrap().center();
        let from = hold(&mut h, "Layer Land");
        touch(&mut h, egui::TouchPhase::Move, from.lerp(rivers, 0.5), None);
        h.step();
        touch(&mut h, egui::TouchPhase::Move, pos2(from.x, 0.0), None);
        h.step();
        touch(&mut h, egui::TouchPhase::End, pos2(from.x, 0.0), None);
        h.run_steps(3);
        assert_eq!(
            layer_names(&h),
            ["Land", "Height", "Rivers", "Places"],
            "{size:?}"
        );
    }
}

/// Runs one frame and the save of the app, as the window does.
fn step_and_save(h: &mut Pad, hold: bool) {
    h.step();
    h.state_mut().save_tick(hold, std::time::Instant::now());
}

/// The layout of a world with a height layer and a project file. The file
/// holds each change.
fn saved_pad(size: (f32, f32), dir: &std::path::Path) -> Pad {
    let mut h = pad(size);
    let state = h.state_mut();
    state.session = Some(Session::open(&dir.join("w.gpkg"), &mut state.doc).unwrap());
    state.save_waiting().unwrap();
    h.run_steps(2);
    h
}

/// The number of the saves of the project file.
fn saves(h: &Pad) -> u64 {
    h.state().session.as_ref().unwrap().saves()
}

#[test]
fn a_drag_moves_a_raster_row_past_a_vector_row_and_undo_puts_it_back() {
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let dir = tempfile::tempdir().unwrap();
        let mut h = saved_pad(size, dir.path());
        open_panel(&mut h, "Layers");
        let before = ["Height", "Land", "Rivers", "Places"];
        assert_eq!(layer_names(&h), before, "{what}");
        let height = h.state().doc.project.rasters()[0].id;
        let layer = h.state().doc.project.raster_layer(height).unwrap().id;
        let saved = saves(&h);

        // The finger holds the raster row and moves it past two vector rows.
        let rivers = control(&h, "Layer Rivers").unwrap().center();
        let from = hold(&mut h, "Layer Height");
        assert_eq!(h.state().pad.reorder.map(|r| r.layer), Some(layer));
        for i in 1..=6 {
            let pos = from.lerp(rivers + vec2(0.0, 6.0), i as f32 / 6.0);
            touch(&mut h, egui::TouchPhase::Move, pos, None);
            step_and_save(&mut h, true);
        }
        assert_eq!(layer_names(&h), before, "{what}");
        touch(&mut h, egui::TouchPhase::End, rivers, None);
        for _ in 0..3 {
            step_and_save(&mut h, false);
        }
        let after = ["Land", "Rivers", "Height", "Places"];
        assert_eq!(layer_names(&h), after, "{what}");
        assert_eq!(
            control(&h, "Layer Height").unwrap().center().y,
            rivers.y,
            "{what}"
        );
        // The map has the vector layers only, in their order.
        let entries = &h.state().doc.frame.entries;
        let on_map: Vec<_> = entries.iter().map(|e| e.layer).collect();
        assert_eq!(on_map, ["Land", "Rivers", "Places"].map(|n| vector(&h, n)));
        // The move is one undo step.
        assert_eq!(saves(&h), saved + 1, "{what}");

        tap(&mut h, "Undo");
        assert_eq!(layer_names(&h), before, "{what}");
        assert!(h.state().globe.raster_map(height).is_some(), "{what}");
        tap(&mut h, "Redo");
        assert_eq!(layer_names(&h), after, "{what}");

        // A vector row moves past the raster row.
        let land = control(&h, "Layer Land").unwrap().center();
        let from = hold(&mut h, "Layer Places");
        for i in 1..=6 {
            let pos = from.lerp(land, i as f32 / 6.0);
            touch(&mut h, egui::TouchPhase::Move, pos, None);
            step_and_save(&mut h, true);
        }
        touch(&mut h, egui::TouchPhase::End, land, None);
        for _ in 0..3 {
            step_and_save(&mut h, false);
        }
        assert_eq!(
            layer_names(&h),
            ["Places", "Land", "Rivers", "Height"],
            "{what}"
        );
        tap(&mut h, "Undo");
        assert_eq!(layer_names(&h), after, "{what}");
    }
}

#[test]
fn the_blend_page_sets_the_blend_mode_and_is_one_undo_step() {
    use vale_store::BlendMode;
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let dir = tempfile::tempdir().unwrap();
        let mut h = saved_pad(size, dir.path());
        let height = h.state().doc.project.rasters()[0].id;
        let blend = |h: &Pad| h.state().doc.project.raster(height).unwrap().blend;
        assert_eq!(blend(&h), BlendMode::Normal, "{what}");
        open_panel(&mut h, "Layers");
        tap(&mut h, "Open Height");
        scroll_to(&mut h, raster::BLEND);
        tap(&mut h, raster::BLEND);
        check_controls(&h, &format!("{what} with the blend page"));
        tap(&mut h, "Blend mode Multiply");
        let card = panel(&h, Panel::Layers).unwrap();
        for mode in BlendMode::ALL {
            let name = format!(
                "{} {}",
                raster::BLEND,
                vale_app::ui::elevation::blend_label(mode)
            );
            scroll_to(&mut h, &name);
            let rect = control(&h, &name).unwrap_or_else(|| panic!("{what}: no {name}"));
            assert!(card.contains_rect(rect), "{what}: {name} is {rect:?}");
        }
        for _ in 0..3 {
            step_and_save(&mut h, false);
        }
        assert_eq!(blend(&h), BlendMode::Multiply, "{what}");
        let style = h.state().doc.project.raster_layer(height).unwrap();
        let style = vale_style::LayerStyle::from_json(style.style.as_deref().unwrap()).unwrap();
        assert_eq!(
            style.raster_symbolizer().unwrap().blend,
            BlendMode::Multiply
        );
        tap(&mut h, "Undo");
        assert_eq!(blend(&h), BlendMode::Normal, "{what}");
    }
}

#[test]
fn the_opacity_slider_changes_the_layer_and_is_one_undo_step() {
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let dir = tempfile::tempdir().unwrap();
        let mut h = saved_pad(size, dir.path());
        let height = h.state().doc.project.rasters()[0].id;
        let opacity = |h: &Pad| h.state().doc.project.raster(height).unwrap().opacity;
        assert_eq!(opacity(&h), 1.0);
        open_panel(&mut h, "Layers");
        tap(&mut h, "Open Height");
        check_controls(&h, &format!("{what} with the opacity slider"));
        let slider = control(&h, OPACITY).unwrap();
        let card = panel(&h, Panel::Layers).unwrap();
        assert!(card.contains_rect(slider), "{what}: {slider:?}");
        // The row is below the rows of the values and of the face size.
        let name = control(&h, LAYER_NAME).unwrap();
        assert_eq!(slider.top(), name.bottom() + 96.0, "{what}");
        let saved = saves(&h);

        // One drag has many values and one save.
        let y = slider.center().y;
        let (from, to) = (pos2(slider.right() - 70.0, y), pos2(slider.left(), y));
        touch(&mut h, egui::TouchPhase::Start, from, None);
        step_and_save(&mut h, true);
        let mut values = vec![opacity(&h)];
        for i in 1..=6 {
            touch(
                &mut h,
                egui::TouchPhase::Move,
                from.lerp(to, i as f32 / 6.0),
                None,
            );
            step_and_save(&mut h, true);
            values.push(opacity(&h));
        }
        assert_eq!(saves(&h), saved, "{what}");
        touch(&mut h, egui::TouchPhase::End, to, None);
        for _ in 0..3 {
            step_and_save(&mut h, false);
        }
        values.dedup();
        assert!(values.len() >= 4, "{what}: {values:?}");
        assert!(values.is_sorted_by(|a, b| a > b), "{what}: {values:?}");
        assert_eq!(opacity(&h), 0.0, "{what}");
        assert_eq!(saves(&h), saved + 1, "{what}");

        // A tap on the track changes nothing, and a typed value sets a whole
        // percent.
        let middle = pos2(slider.center().x + 3.3, y);
        touch(&mut h, egui::TouchPhase::Start, middle, None);
        step_and_save(&mut h, true);
        touch(&mut h, egui::TouchPhase::End, middle, None);
        for _ in 0..3 {
            step_and_save(&mut h, false);
        }
        assert_eq!(opacity(&h), 0.0, "{what}");
        type_value(&mut h, OPACITY, "42.4");
        step_and_save(&mut h, false);
        // The change saves after a short wait.
        let later = std::time::Instant::now() + std::time::Duration::from_secs(10);
        h.state_mut().save_tick(false, later);
        assert_eq!(opacity(&h), 0.42, "{what}");
        assert_eq!(saves(&h), saved + 2, "{what}");

        // One undo puts back the opacity from before the typed value, and the
        // next one puts back the opacity from before the drag.
        tap(&mut h, "Undo");
        assert_eq!(opacity(&h), 0.0, "{what}");
        tap(&mut h, "Undo");
        assert_eq!(opacity(&h), 1.0, "{what}");
        tap(&mut h, "Redo");
        assert_eq!(opacity(&h), 0.0, "{what}");
    }
}

#[test]
fn a_quick_drag_does_not_lift_a_row() {
    let mut h = pad(BOARDS[0]);
    open_panel(&mut h, "Layers");
    let from = control(&h, "Layer Land").unwrap().center();
    let to = control(&h, "Layer Places").unwrap().center();
    touch(&mut h, egui::TouchPhase::Start, from, None);
    touch(&mut h, egui::TouchPhase::Move, from.lerp(to, 0.5), None);
    for _ in 0..8 {
        h.step();
        assert_eq!(h.state().pad.reorder, None);
    }
    touch(&mut h, egui::TouchPhase::End, to, None);
    h.run_steps(3);
    assert_eq!(layer_names(&h), ["Height", "Land", "Rivers", "Places"]);
    // A tap makes the layer the selected layer.
    tap(&mut h, "Layer Rivers");
    let rivers = vector(&h, "Rivers");
    assert_eq!(h.state().selected_layer, Some(LayerRef::Vector(rivers)));
}

/// A sideways drag of a finger on the row of a layer, from near the right
/// end of its body.
fn swipe(h: &mut Pad, name: &str, dx: f32) {
    let body = control(h, &format!("Layer {name}")).unwrap_or_else(|| panic!("no row {name}"));
    let from = body.right_center() - vec2(10.0, 0.0);
    drag(h, from, from + vec2(dx, 0.0), None);
}

/// A swipe that opens a row, and a tap on its Delete button.
fn swipe_and_delete(h: &mut Pad, name: &str) {
    swipe(h, name, -120.0);
    tap(h, &format!("Delete {name}"));
}

#[test]
fn a_swipe_shows_delete_a_tap_deletes_and_undo_brings_it_back() {
    for size in BOARDS {
        let what = format!("{size:?}");
        let dir = tempfile::tempdir().unwrap();
        let mut h = saved_pad(size, dir.path());
        open_panel(&mut h, "Layers");
        assert!(control(&h, "Delete Rivers").is_none(), "{what}");
        swipe(&mut h, "Rivers", -120.0);
        let rivers = vector(&h, "Rivers");
        let open = h.state().pad.swipe.unwrap();
        assert_eq!((open.row, open.moving), (rivers, false), "{what}");
        let delete = control(&h, "Delete Rivers").unwrap_or_else(|| panic!("{what}"));
        assert!(delete.width() >= 88.0 - 0.5, "{what}: {delete:?}");
        // The eye and the arrow of the open row take no touch.
        assert!(control(&h, "Hide Rivers").is_none(), "{what}");
        assert!(control(&h, "Open Rivers").is_none(), "{what}");
        assert!(control(&h, "Hide Land").is_some(), "{what}");
        check_controls(&h, &format!("{what} with an open row"));
        tap(&mut h, "Delete Rivers");
        assert_eq!(layer_names(&h), ["Height", "Land", "Places"], "{what}");
        assert_eq!(h.state().pad.swipe, None, "{what}");
        step_and_save(&mut h, false);
        h.state_mut().undo();
        h.run_steps(2);
        assert_eq!(layer_names(&h), ["Height", "Land", "Rivers", "Places"]);
        assert!(control(&h, "Hide Rivers").is_some(), "{what}");

        // A long swipe only opens the row, and it deletes nothing.
        let height = h.state().doc.project.rasters()[0].id;
        let width = panel(&h, Panel::Layers).unwrap().width();
        swipe(&mut h, "Height", -0.9 * width);
        assert_eq!(layer_names(&h), ["Height", "Land", "Rivers", "Places"]);
        let open = h.state().pad.swipe.unwrap();
        assert_eq!(
            open.offset,
            -vale_app::ui::pad::widgets::RENAME_WIDTH,
            "{what}"
        );
        tap(&mut h, "Delete Height");
        assert!(h.state().doc.project.rasters().is_empty(), "{what}");
        assert_eq!(layer_names(&h), ["Land", "Rivers", "Places"], "{what}");
        step_and_save(&mut h, false);
        h.state_mut().undo();
        h.run_steps(2);
        assert_eq!(raster_name(&h, height), "Height", "{what}");
        assert_eq!(layer_names(&h), ["Height", "Land", "Rivers", "Places"]);
    }
}

#[test]
fn a_short_swipe_snaps_shut() {
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        open_panel(&mut h, "Layers");
        swipe(&mut h, "Land", -30.0);
        assert_eq!(h.state().pad.swipe, None, "{what}");
        assert!(control(&h, "Delete Land").is_none(), "{what}");
        assert!(control(&h, "Hide Land").is_some(), "{what}");
        assert_eq!(layer_names(&h).len(), 4, "{what}");

        // A tap on the body of an open row shuts it, and a press elsewhere
        // shuts it.
        swipe(&mut h, "Land", -120.0);
        assert!(h.state().pad.swipe.is_some(), "{what}");
        tap(&mut h, "Layer Land");
        assert_eq!(h.state().pad.swipe, None, "{what}");
        swipe(&mut h, "Land", -120.0);
        tap(&mut h, "Layer Places");
        assert_eq!(h.state().pad.swipe, None, "{what}");
        assert!(control(&h, "Hide Land").is_some(), "{what}");
        // Only one row is open.
        swipe(&mut h, "Land", -120.0);
        swipe(&mut h, "Places", -120.0);
        let open = h.state().pad.swipe.map(|s| s.row);
        assert_eq!(open, Some(vector(&h, "Places")), "{what}");
        assert!(control(&h, "Delete Land").is_none(), "{what}");
        assert_eq!(layer_names(&h).len(), 4, "{what}");
    }
}

#[test]
fn a_vertical_drag_on_the_list_scrolls_and_opens_no_row() {
    let mut h = pad(BOARDS[3]);
    for _ in 0..12 {
        h.state_mut().doc.add_layer(vale_store::GeometryKind::Line);
    }
    h.run_steps(2);
    open_panel(&mut h, "Layers");
    let top = control(&h, "Layer Height").unwrap().center().y;
    let from = control(&h, "Layer Places").unwrap().center();
    drag(&mut h, from, from - vec2(10.0, 150.0), None);
    assert_eq!(h.state().pad.swipe, None);
    assert_eq!(h.state().pad.reorder, None);
    assert!(
        control(&h, "Layer Height").is_none_or(|r| r.center().y < top - 50.0),
        "the list did not scroll"
    );
    assert_eq!(layer_names(&h).len(), 16);
    assert!(control(&h, "Delete Places").is_none());
}

/// Writes a 16-bit greyscale PNG with a slope from west to east.
fn heightmap_file(name: &str, width: u32, height: u32) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/app/test-import");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let level = |x: u32, _| image::Luma([(x * 65535 / width) as u16]);
    let image = image::ImageBuffer::<image::Luma<u16>, _>::from_fn(width, height, level);
    image.save(&path).unwrap();
    path
}

#[test]
fn the_height_panel_asks_for_a_heightmap_file() {
    for size in BOARDS {
        let mut h = painted_pad(size);
        // iOS has no file dialog of the desktop.
        h.state_mut().file_buttons = false;
        open_panel(&mut h, "Layers");
        tap(&mut h, "Open Height");
        check_controls(&h, &format!("{size:?}"));
        assert!(h.state().actions.is_empty());
        tap(&mut h, IMPORT_HEIGHTMAP);
        let actions = &h.state().actions;
        assert_eq!(actions, &[Action::ImportHeightmapDialog], "{size:?}");
    }
}

#[test]
fn the_height_panel_shows_the_warning_of_a_wrong_aspect_ratio() {
    let path = heightmap_file("pad-ratio.png", 300, 200);
    for size in BOARDS {
        // A world with no height layer has the import row in the menu of
        // the new layers. The import makes the Height layer and opens its
        // panel.
        let mut h = empty_pad(size);
        open_panel(&mut h, "Layers");
        assert!(control(&h, "Open Height").is_none());
        tap(&mut h, ADD_LAYER);
        check_controls(&h, &format!("{size:?} with the import row"));
        tap(&mut h, NEW_HEIGHTMAP);
        assert!(!h.state().pad.add, "{size:?}");
        assert_eq!(
            h.state_mut().actions.pop(),
            Some(Action::ImportHeightmapDialog)
        );
        assert!(h.query_by_label_contains("not 2:1").is_none());
        h.state_mut()
            .run_action(Action::ImportHeightmap(path.clone()));
        h.run_steps(3);
        assert!(h.state().import_note.as_ref().unwrap().warning);
        assert!(h.query_by_label_contains("not 2:1").is_some(), "{size:?}");
        check_controls(&h, &format!("{size:?} with the warning"));
    }
}

#[test]
fn the_import_row_is_off_while_an_import_runs() {
    let path = heightmap_file("pad-busy.png", 512, 256);
    let mut h = painted_pad(BOARDS[0]);
    open_panel(&mut h, "Layers");
    tap(&mut h, "Open Height");
    assert!(control(&h, IMPORT_HEIGHTMAP).is_some());
    // The worker gives one face in each frame.
    h.state_mut().globe.start_import(path);
    h.run_steps(2);
    assert!(h.state().globe.importing());
    assert!(control(&h, IMPORT_HEIGHTMAP).is_none());
    assert!(
        h.query_by_label_contains("Importing pad-busy.png")
            .is_some()
    );
    let start = std::time::Instant::now();
    while h.state().globe.busy() {
        assert!(start.elapsed().as_secs() < 30, "the import did not end");
        h.step();
    }
    h.run_steps(2);
    assert!(control(&h, IMPORT_HEIGHTMAP).is_some());
    assert!(h.query_by_label_contains("Imported pad-busy.png").is_some());
    assert!(!h.state().import_note.as_ref().unwrap().warning);
}

#[test]
fn the_note_of_an_import_shows_only_in_the_panel_of_its_layer_and_goes_away() {
    use vale_app::ui::pad::NOTE_TIME;
    let path = heightmap_file("pad-note.png", 512, 256);
    let mut h = painted_pad(BOARDS[0]);
    let rain = vale_app::ui::RasterSpec {
        name: "Rain".to_string(),
        unit: "mm".to_string(),
        min: 0.0,
        max: 4000.0,
        face_size: 64,
    };
    h.state_mut().add_raster(rain).unwrap();
    h.run_steps(2);
    open_panel(&mut h, "Layers");
    tap(&mut h, "Open Height");
    h.state_mut()
        .run_action(Action::ImportHeightmap(path.clone()));
    let start = std::time::Instant::now();
    while h.state().globe.busy() {
        assert!(start.elapsed().as_secs() < 30, "the import did not end");
        h.step();
    }
    h.run_steps(2);
    let note = |h: &Pad| h.query_by_label_contains("Imported pad-note.png").is_some();
    assert!(note(&h));
    // The panel of another raster layer does not show the note.
    tap(&mut h, "Back to Layers");
    tap(&mut h, "Open Rain");
    assert!(!note(&h));
    // The note went away when its panel closed.
    assert_eq!(h.state().import_note, None);
    tap(&mut h, "Back to Layers");
    tap(&mut h, "Open Height");
    assert!(!note(&h));

    // A note with no warning goes away after a few seconds, and a warning
    // stays while its panel is open.
    for warning in [false, true] {
        let text = format!("note {warning}");
        let height = h.state().doc.project.rasters()[0].id;
        let mut made = vale_app::ui::Note::new(text.clone(), warning, Some(height));
        h.state_mut().import_note = Some(made.clone());
        h.run_steps(2);
        assert!(h.query_by_label(&text).is_some(), "{warning}");
        let (time, pass) = h.state().import_note.as_ref().unwrap().shown.unwrap();
        made.shown = Some((time - NOTE_TIME, pass));
        made.last_pass = h.state().import_note.as_ref().unwrap().last_pass;
        h.state_mut().import_note = Some(made);
        h.run_steps(2);
        assert_eq!(h.query_by_label(&text).is_some(), warning);
    }
}

/// The layout with the open panel of the Height layer of a small heightmap.
fn height_pad(size: (f32, f32)) -> Pad {
    let mut h = empty_pad(size);
    h.state_mut().globe.set_face_size(64);
    h.state_mut().add_height();
    h.state_mut().globe.map.set(0, 0, 0, meters_to_level(100.0));
    h.run_steps(2);
    open_panel(&mut h, "Layers");
    tap(&mut h, "Open Height");
    h
}

#[test]
fn the_height_panel_asks_for_the_file_of_an_export() {
    for size in BOARDS {
        let mut h = height_pad(size);
        check_controls(&h, &format!("{size:?}"));
        // The row is below the import row, and the two rows are in the panel.
        let import = control(&h, IMPORT_HEIGHTMAP).unwrap();
        let export = control(&h, EXPORT_GEOTIFF).unwrap();
        assert!(export.top() >= import.bottom(), "{size:?}");
        let card = panel(&h, Panel::Layers).unwrap();
        assert!(card.contains_rect(export), "{size:?}: {export:?}");
        assert!(h.state().actions.is_empty());
        tap(&mut h, EXPORT_GEOTIFF);
        let actions = &h.state().actions;
        assert_eq!(actions, &[Action::ExportHeightmapDialog], "{size:?}");
    }
}

#[test]
fn a_world_with_no_raster_layer_has_no_export_row() {
    for size in BOARDS {
        let mut h = empty_pad(size);
        open_panel(&mut h, "Layers");
        assert!(control(&h, IMPORT_HEIGHTMAP).is_none(), "{size:?}");
        assert!(control(&h, EXPORT_GEOTIFF).is_none(), "{size:?}");
        assert!(h.query_by_label(EXPORT_GEOTIFF).is_none(), "{size:?}");
    }
}

#[test]
fn the_height_panel_shows_the_note_of_an_export_below_the_row() {
    let dir = tempfile::tempdir().unwrap();
    for size in BOARDS {
        let mut h = height_pad(size);
        let path = dir.path().join("Terra.tif");
        h.state_mut().run_action(Action::ExportHeightmap(path));
        h.run_steps(3);
        assert!(!h.state().export_note.as_ref().unwrap().warning);
        let note = h.get_by_label("Exported Terra.tif (256 × 128)").rect();
        let row = control(&h, EXPORT_GEOTIFF).unwrap();
        assert!(note.top() >= row.bottom() - 0.5, "{size:?}: {note:?}");
        assert!(note.left() >= row.left() && note.right() <= row.right() + 0.5);
        check_controls(&h, &format!("{size:?} with the note"));

        // A folder is in the place of the file.
        let path = dir.path().to_path_buf();
        h.state_mut().run_action(Action::ExportHeightmap(path));
        h.run_steps(3);
        assert!(h.state().export_note.as_ref().unwrap().warning);
        assert!(h.query_by_label_contains("Exported").is_none());
        assert!(h.query_by_label_contains("cannot write").is_some());
        // The panel grows with the text, or it scrolls.
        let card = panel(&h, Panel::Layers).unwrap();
        let screen = h.state().pad.rects.unwrap().screen;
        assert!(screen.contains_rect(card), "{size:?}: {card:?}");
    }
}

/// A named pipe holds the worker until the test opens the pipe.
#[cfg(unix)]
#[test]
fn the_export_row_is_off_and_shows_the_progress_while_an_export_runs() {
    let dir = tempfile::tempdir().unwrap();
    let pipe = dir.path().join("Terra.tif");
    let made = std::process::Command::new("mkfifo").arg(&pipe).status();
    assert!(made.unwrap().success());

    let mut h = height_pad(BOARDS[0]);
    h.state_mut().headless = false;
    h.state_mut()
        .run_action(Action::ExportHeightmap(pipe.clone()));
    h.run_steps(2);
    assert!(h.state().globe.exporting());
    assert!(control(&h, EXPORT_GEOTIFF).is_none());
    // The brush and the import stay on.
    assert!(control(&h, IMPORT_HEIGHTMAP).is_some());
    let progress = h.get_by_label("Exporting Terra.tif: 0%").rect();
    let import = control(&h, IMPORT_HEIGHTMAP).unwrap();
    assert!(progress.top() >= import.bottom() + 40.0, "{progress:?}");

    // The worker cannot write a GeoTIFF to a pipe, so the export fails.
    drop(std::fs::File::open(&pipe).unwrap());
    let start = std::time::Instant::now();
    while h.state().globe.exporting() {
        assert!(start.elapsed().as_secs() < 30, "the export did not end");
        h.step();
    }
    h.run_steps(2);
    assert!(control(&h, EXPORT_GEOTIFF).is_some());
    assert!(h.query_by_label_contains("Exporting").is_none());
    assert!(h.state().export_note.is_some());
}

#[test]
fn a_finger_hits_each_control_at_the_first_try() {
    let mut h = pad(BOARDS[0]);
    for (name, mode) in [
        ("Lower", Mode::Lower),
        ("Smooth", Mode::Smooth),
        ("Flatten", Mode::Flatten),
        ("Raise", Mode::Raise),
    ] {
        choose_mode(&mut h, name);
        assert_eq!(h.state().globe.tool, Tool::Brush);
        assert_eq!(h.state().globe.brush.mode, mode);
    }
    choose_mode(&mut h, "Carve");
    assert_eq!(h.state().globe.brush.mode, Mode::Carve);

    for (name, which) in [
        ("Layers", Panel::Layers),
        (select::ATTRIBUTES, Panel::Attributes),
        ("Brush settings", Panel::Brush),
    ] {
        if which == Panel::Brush {
            tap(&mut h, name);
        } else {
            open_panel(&mut h, name);
        }
        assert!(h.state().pad.is_open(which), "{name}");
    }
    // Each panel stays.
    assert!(h.state().pad.is_open(Panel::Layers));

    // A touch at the top of a slider sets its largest value.
    h.state_mut().globe.brush.flow = 0.2;
    for (name, at) in [("Flow", 0.0), ("Brush hardness", 1.0)] {
        let rect = control(&h, name).unwrap();
        let (from, to) = if at == 0.0 {
            (rect.center(), rect.center_top())
        } else {
            (rect.center(), rect.right_center())
        };
        drag(&mut h, from, to, None);
    }
    assert_eq!(h.state().globe.brush.flow, 1.0);
    assert_eq!(h.state().globe.brush.hardness, 1.0);
    let size = h.state().globe.brush.size_points;
    let rect = control(&h, "Size").unwrap();
    drag(&mut h, rect.center(), rect.center_bottom(), None);
    assert!(h.state().globe.brush.size_points < size);

    // The Move tool has no context strip, and the Brush panel stays.
    quick_tap(&mut h, "Pan");
    assert_eq!(h.state().globe.tool, Tool::Navigate);
    assert!(control(&h, "Brush settings").is_none());
    assert!(h.state().pad.is_open(Panel::Brush));

    tap(&mut h, "Atlas");
    assert_eq!(h.state().workspace, Workspace::Atlas);
}

#[test]
fn the_brush_size_uses_the_radius_and_the_units_of_the_world() {
    let mut h = pad(BOARDS[0]);
    let km = |h: &Pad| {
        let globe = &h.state().globe;
        let world_km = h.state().doc.project.world.radius_km;
        globe.brush.radius_km(globe.radius(), world_km)
    };
    let earth = km(&h);
    assert_eq!(
        brush_size_text(h.state()),
        format!("{} km", three_significant(earth))
    );

    // The brush covers the same angle on a small world, and thus less ground.
    h.state_mut().doc.set_radius_km(200.0);
    h.run_steps(2);
    let small = km(&h);
    assert!((small - earth * 200.0 / 6371.0).abs() < 1e-9);
    assert_eq!(h.state().globe.world_km, 200.0);
    assert_eq!(
        brush_size_text(h.state()),
        format!("{} km", three_significant(small))
    );

    // The slider reaches the same angles on each world.
    let globe = &h.state().globe;
    let range = globe.brush.radius_km_range(globe.radius(), 200.0);
    let earth_range = globe.brush.radius_km_range(globe.radius(), 6371.0);
    assert!((range.end() / earth_range.end() - 200.0 / 6371.0).abs() < 1e-12);

    h.state_mut().doc.project.world.units = Units::Imperial;
    h.run_steps(2);
    let miles = small / 1.609_344;
    assert_eq!(
        brush_size_text(h.state()),
        format!("{} mi", three_significant(miles))
    );
}

/// The names of the tool cells of the tool strip that are on, from the
/// top. The tool column also holds the controls of the tool.
fn strip(h: &Pad) -> Vec<String> {
    use vale_app::ui::pad::atlas::{FRAME_TOOL, MOVE, VERTEX};
    let names = [
        SCULPT,
        TERRAIN,
        VECTOR,
        select::TOOL,
        "Pan",
        MEASURE,
        FRAME_TOOL,
        VERTEX,
        MOVE,
    ];
    let tools = h.state().pad.rects.unwrap().tools;
    let controls = h.state().pad.controls.iter();
    let cells =
        controls.filter(|c| tools.contains_rect(c.rect) && names.contains(&c.name.as_str()));
    cells.map(|c| c.name.clone()).collect()
}

#[test]
fn the_tool_strip_has_six_cells_on_each_screen() {
    for (size, safe) in measure_screens() {
        let what = format!("{size:?}");
        let mut h = safe_pad(size, safe);
        let rects = h.state().pad.rects.unwrap();
        // The landscape iPhone is too short for the column, so the column
        // scrolls.
        let full = size.1 > 400.0;
        assert_eq!(rects.tool_cells, TOOLS, "{what}");
        if rects.row {
            // On a compact screen, the strip is a row.
            assert_eq!(rects.tools.width(), 6.0 * 52.0 + 8.0, "{what}");
        } else {
            // The tool column has the six tools at its top, and below them
            // the controls of the tool.
            let top = rects.tools.top() + 6.0;
            assert_eq!(control(&h, SCULPT).unwrap().top(), top, "{what}");
            assert!(rects.column_content > 12.0 + 6.0 * 44.0, "{what}");
            let scrolls = rects.column_content > rects.tools.height() + 0.5;
            assert_eq!(scrolls, !full, "{what}");
        }
        let screen = Rect::from_min_size(Pos2::ZERO, size.into());
        assert!(screen.contains_rect(rects.tools), "{what}");
        // The cells that are on show in this order.
        let shows = |h: &Pad, cells: &[&str]| {
            let strip = strip(h);
            if full {
                assert_eq!(strip, cells, "{what}");
            } else {
                assert!(
                    cells.windows(strip.len()).any(|w| w == strip),
                    "{what}: {strip:?}"
                );
            }
        };
        // With a raster layer, the Vector cell is off.
        shows(&h, &[SCULPT, TERRAIN, select::TOOL, "Pan", MEASURE]);
        assert!(disabled(&h, VECTOR), "{what}");
        check_controls(&h, &what);
        check_safe(&h, size, safe, &what);
        // With a vector layer, the brush cells are off.
        let rivers = vector(&h, "Rivers");
        h.state_mut().select(Some(LayerRef::Vector(rivers)));
        h.run_steps(2);
        shows(&h, &[VECTOR, select::TOOL, "Pan", MEASURE]);
        assert!(disabled(&h, SCULPT) && disabled(&h, TERRAIN), "{what}");
        check_controls(&h, &format!("{what} with a line layer"));
        check_safe(&h, size, safe, &what);
        for card in cards(&h) {
            assert!(screen.contains_rect(card), "{what}: {card:?}");
        }
        scroll_strip_to(&mut h, MEASURE);
        assert!(strip(&h).contains(&MEASURE.to_string()), "{what}");
    }
}

#[test]
fn the_small_ipads_show_the_sliders_in_the_tool_column() {
    // The small iPads in landscape have less room for the sliders.
    for (size, track) in [
        ((1133.0, 744.0), 101.5),
        ((744.0, 1133.0), 120.0),
        ((1024.0, 768.0), 113.5),
        ((768.0, 1024.0), 120.0),
    ] {
        let what = format!("{size:?}");
        let h = pad(size);
        let rects = h.state().pad.rects.unwrap();
        assert_eq!(rects.brush_card, BrushCard::Full(track), "{what}");
        assert_eq!(rects.tools.height(), rects.column_content, "{what}");
        for name in ["Size", "Flow", "Brush settings"] {
            let rect = control(&h, name).unwrap_or_else(|| panic!("{what}: {name}"));
            assert!(rects.tools.contains_rect(rect), "{what}: {name}");
        }
        // The value is above the track, and the name is below it.
        assert_eq!(
            control(&h, "Size").unwrap().height(),
            track + 44.0,
            "{what}"
        );
        check_controls(&h, &what);
    }
}

#[test]
fn a_tap_on_the_brush_cell_chooses_the_brush_in_its_mode() {
    let mut h = pad(BOARDS[0]);
    h.state_mut().globe.brush.mode = Mode::Erode;
    quick_tap(&mut h, "Pan");
    assert_eq!(h.state().globe.tool, Tool::Navigate);
    assert_eq!(toggled(&h, TERRAIN), Some(false));
    assert_eq!(face(&h, ModeGroup::Terrain), "Erode");
    assert_eq!(face(&h, ModeGroup::Sculpt), "Raise");
    quick_tap(&mut h, TERRAIN);
    let s = h.state();
    assert_eq!(
        (s.globe.tool, s.globe.brush.mode),
        (Tool::Brush, Mode::Erode)
    );
    assert_eq!(s.pad.flyouts.open, None);
    assert_eq!(toggled(&h, TERRAIN), Some(true));
    assert_eq!(toggled(&h, SCULPT), Some(false));
}

/// The label of the cell of a group.
fn face(h: &Pad, group: ModeGroup) -> &'static str {
    let i = ModeGroup::ALL.iter().position(|g| *g == group).unwrap();
    h.state().pad.brush_faces[i].unwrap().0
}

#[test]
fn each_brush_cell_remembers_the_last_mode_of_its_group() {
    let mut h = pad(BOARDS[0]);
    choose_mode(&mut h, "Push");
    choose_mode(&mut h, "Erode");
    assert_eq!(face(&h, ModeGroup::Sculpt), "Push");
    assert_eq!(face(&h, ModeGroup::Terrain), "Erode");
    assert_eq!(toggled(&h, TERRAIN), Some(true));
    assert_eq!(toggled(&h, SCULPT), Some(false));
    quick_tap(&mut h, SCULPT);
    let s = h.state();
    assert_eq!(
        (s.globe.tool, s.globe.brush.mode),
        (Tool::Brush, Mode::Push)
    );
    assert_eq!(s.pad.flyouts.open, None);
    assert_eq!(toggled(&h, SCULPT), Some(true));
    assert_eq!(toggled(&h, TERRAIN), Some(false));
    quick_tap(&mut h, TERRAIN);
    assert_eq!(h.state().globe.brush.mode, Mode::Erode);
    assert_eq!(face(&h, ModeGroup::Sculpt), "Push");
    // After the Move tool, each cell keeps its mode.
    quick_tap(&mut h, "Pan");
    quick_tap(&mut h, SCULPT);
    assert_eq!(h.state().globe.brush.mode, Mode::Push);
}

#[test]
fn each_flyout_lists_the_modes_of_its_group_and_fits_on_each_screen() {
    for (size, safe) in measure_screens() {
        let mut h = safe_pad(size, safe);
        let screen = Rect::from_min_size(Pos2::ZERO, size.into());
        for group in ModeGroup::ALL {
            let what = format!("{size:?} {}", group.name());
            open_group(&mut h, group);
            let flyout = h.state().pad.flyouts.rect.unwrap();
            assert!(screen.contains_rect(flyout), "{what}: {flyout:?}");
            let cell = control(&h, group.name()).unwrap();
            let rects = h.state().pad.rects.unwrap();
            let tools = rects.tools;
            if rects.row {
                // In the row layout, the flyout is above the strip.
                assert_eq!(flyout.bottom(), tools.top() - 12.0, "{what}");
                assert!(flyout.top() >= rects.column_top, "{what}");
            } else {
                assert_eq!(flyout.left(), tools.right() + 12.0, "{what}");
                assert!(flyout.top() <= cell.top(), "{what}");
            }
            for name in all_modes() {
                let mine = group.modes().iter().any(|(mode, ..)| *mode == name);
                assert!(mine || control(&h, name).is_none(), "{what}: {name}");
            }
            check_controls(&h, &what);
            check_safe(&h, size, safe, &what);
            // A flyout that is shorter than its rows scrolls to each row.
            for (name, ..) in group.modes() {
                for _ in 0..4 {
                    if control(&h, name).is_some() {
                        break;
                    }
                    let from = flyout.center();
                    drag(&mut h, from, from - vec2(0.0, 0.4 * flyout.height()), None);
                }
                let row = control(&h, name).unwrap_or_else(|| panic!("{what}: {name}"));
                assert!(flyout.contains_rect(row), "{what}: {name}");
            }
            quick_tap(&mut h, group.name());
            assert_eq!(h.state().pad.flyouts.open, None, "{what}");
        }
    }
}

/// Holds a finger on a control until a flyout opens, and lifts it.
fn hold_cell(h: &mut Pad, name: &str) {
    let pos = control(h, name).unwrap().center();
    touch(h, egui::TouchPhase::Start, pos, None);
    for _ in 0..200 {
        h.step();
        if h.state().pad.flyouts.open.is_some() {
            break;
        }
    }
    touch(h, egui::TouchPhase::End, pos, None);
    h.run_steps(3);
}

#[test]
fn a_hold_or_a_second_tap_on_the_brush_cell_opens_the_flyout() {
    let mut h = pad(BOARDS[0]);
    let tools = h.state().pad.rects.unwrap().tools;
    quick_tap(&mut h, "Pan");
    // A hold opens the flyout, and the end of the hold is not a tap.
    for group in ModeGroup::ALL {
        let name = group.name();
        hold_cell(&mut h, name);
        assert_eq!(
            h.state().pad.flyouts.open,
            Some(Flyout::Brush(group)),
            "{name}"
        );
        assert_eq!(h.state().globe.tool, Tool::Navigate, "{name}");
        let flyout = h.state().pad.flyouts.rect.unwrap();
        let cell = control(&h, name).unwrap();
        assert_eq!(flyout.left_top(), pos2(tools.right() + 12.0, cell.top()));
        for (mode, ..) in group.modes() {
            let row = control(&h, mode).unwrap();
            assert!(flyout.contains_rect(row), "{mode}");
        }
        assert_eq!(strip(&h), [SCULPT, TERRAIN, select::TOOL, "Pan", MEASURE]);
        check_controls(&h, &format!("the flyout of {name}"));

        // A press outside closes the flyout and changes nothing.
        let outside = flyout.right_bottom() + vec2(40.0, 40.0);
        finger_tap(&mut h, outside);
        let s = h.state();
        assert!(s.pad.flyouts.open.is_none() && s.pad.flyouts.rect.is_none());
        assert!(control(&h, "Weather").is_none());
        assert_eq!(s.globe.tool, Tool::Navigate);
    }

    // The first tap chooses the brush, the second tap opens the flyout,
    // and a third tap closes it.
    quick_tap(&mut h, SCULPT);
    assert!(h.state().globe.brushing() && h.state().pad.flyouts.open.is_none());
    quick_tap(&mut h, SCULPT);
    assert_eq!(
        h.state().pad.flyouts.open,
        Some(Flyout::Brush(ModeGroup::Sculpt))
    );
    quick_tap(&mut h, SCULPT);
    assert_eq!(h.state().pad.flyouts.open, None);

    // A tap on the other cell chooses its mode and closes the flyout.
    quick_tap(&mut h, SCULPT);
    quick_tap(&mut h, TERRAIN);
    assert_eq!(h.state().globe.brush.mode, Mode::Carve);
    assert_eq!(h.state().pad.flyouts.open, None);

    // A hold while the brush is the tool opens the flyout too.
    hold_cell(&mut h, TERRAIN);
    assert_eq!(
        h.state().pad.flyouts.open,
        Some(Flyout::Brush(ModeGroup::Terrain))
    );
    // A tap on another control closes it.
    open_panel(&mut h, "Layers");
    assert!(h.state().pad.flyouts.open.is_none() && h.state().pad.is_open(Panel::Layers));

    // The flyout opens while the Brush panel is open.
    tap(&mut h, "Brush settings");
    open_modes(&mut h);
    assert!(h.state().pad.flyouts.rect.is_some());
    assert!(h.state().pad.is_open(Panel::Brush));
}

#[test]
fn a_mode_of_the_flyout_sets_the_mode_and_the_face_of_the_cell() {
    let mut h = pad(BOARDS[0]);
    for group in ModeGroup::ALL {
        for &(name, icon, mode) in group.modes() {
            // A row of the flyout also chooses the brush.
            quick_tap(&mut h, "Pan");
            choose_mode(&mut h, name);
            let s = h.state();
            assert_eq!((s.globe.tool, s.globe.brush.mode), (Tool::Brush, mode));
            assert!(
                s.pad.flyouts.open.is_none() && s.pad.flyouts.rect.is_none(),
                "{name}"
            );
            let i = ModeGroup::ALL.iter().position(|g| *g == group).unwrap();
            assert_eq!(s.pad.brush_faces[i], Some((name, icon)), "{name}");
            assert_eq!(toggled(&h, group.name()), Some(true), "{name}");
        }
    }

    // The Brush panel has the angle in the Weather mode only.
    tap(&mut h, "Brush settings");
    assert!(control(&h, "Brush angle value").is_none());
    choose_mode(&mut h, "Weather");
    assert_eq!(h.state().globe.brush.mode, Mode::Weather);
    assert!(h.state().pad.is_open(Panel::Brush));
    type_value(&mut h, "Brush angle", "30");
    assert!((h.state().globe.brush.talus_deg - 20.0).abs() < 1e-9);
    step(&mut h, "Brush angle", false);
    assert!((h.state().globe.brush.talus_deg - 10.0).abs() < 1e-9);
}

#[test]
fn the_terrace_mode_is_in_the_terrain_cell_and_has_a_step() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Brush settings");
    assert!(control(&h, "Brush step").is_none());
    choose_mode(&mut h, "Terrace");
    assert_eq!(h.state().globe.brush.mode, Mode::Terrace);
    assert!(control(&h, "Brush step").is_some());
    assert!(control(&h, "Brush strength").is_none());
}

#[test]
fn the_shape_mode_is_in_the_terrain_cell_and_has_a_shape() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Brush settings");
    assert!(control(&h, "Brush shape").is_none());
    open_group(&mut h, group_of("Shape"));
    screenshot::save(&mut h, "pad-terrain-modes");
    tap(&mut h, "Shape");
    assert_eq!(h.state().globe.brush.mode, Mode::Shape);
    assert!(control(&h, "Brush shape").is_some());
    assert!(control(&h, "Brush strength").is_none());
    check_controls(&h, "the Shape mode");
    screenshot::save(&mut h, "pad-shape");
}

#[test]
fn the_brush_panel_of_the_push_mode_has_no_strength() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Brush settings");
    assert!(control(&h, "Brush strength").is_some());
    choose_mode(&mut h, "Push");
    assert_eq!(h.state().globe.brush.mode, Mode::Push);
    assert!(control(&h, "Brush strength").is_none());
    assert!(control(&h, "Brush flow").is_some());
    assert!(control(&h, "Brush hardness").is_some());
    check_controls(&h, "the Push mode");
}

#[test]
fn the_brush_panel_of_the_noise_mode_has_its_settings() {
    // The portrait iPad has the height for all the rows of the mode.
    let mut h = pad(BOARDS[1]);
    tap(&mut h, "Brush settings");
    assert!(control(&h, "Brush ridged").is_none());
    choose_mode(&mut h, "Noise");
    assert_eq!(
        (h.state().globe.tool, h.state().globe.brush.mode),
        (Tool::Brush, Mode::Noise)
    );
    assert_eq!(toggled(&h, SCULPT), Some(true));
    // The Raise and Lower sliders take the place of the Strength slider.
    assert!(control(&h, "Brush strength").is_none());
    check_controls(&h, "the Noise mode");
    screenshot::save(&mut h, "pad-noise");
    slide_to_end(&mut h, "Brush scale", false);
    assert!((h.state().globe.brush.scale_km - 5.0).abs() < 1e-9);
    // The Invert switch shows with the ridged noise only.
    assert!(!switch_is_on(&h, "Brush ridged"));
    assert!(control(&h, "Brush invert").is_none());
    tap(&mut h, "Brush ridged");
    assert!(h.state().globe.brush.noise_ridged);
    assert!(switch_is_on(&h, "Brush ridged"));
    tap(&mut h, "Brush invert");
    assert!(h.state().globe.brush.noise_inverted);
    assert!(switch_is_on(&h, "Brush invert"));
    check_controls(&h, "the inverted ridged noise");
    assert_eq!(h.state().globe.brush.noise_relief, 0.5);
    slide_to_end(&mut h, "Brush relief", true);
    assert_eq!(h.state().globe.brush.noise_relief, 1.0);
    // The Grain slider goes from no grain to features 10 times longer along
    // the stroke.
    assert_eq!(h.state().globe.brush.noise_grain, 0.0);
    slide_to_end(&mut h, "Brush grain", true);
    assert_eq!(h.state().globe.brush.noise_grain, 9.0);
    slide_to_end(&mut h, "Brush grain", false);
    assert_eq!(h.state().globe.brush.noise_grain, 0.0);
    // Each of the two sliders goes to 0 apart from the other.
    slide_to_end(&mut h, "Brush lower", false);
    let brush = &h.state().globe.brush;
    assert_eq!((brush.noise_raise_m, brush.noise_lower_m), (1500.0, 0.0));
    slide_to_end(&mut h, "Brush raise", false);
    assert_eq!(h.state().globe.brush.noise_raise_m, 0.0);
    slide_to_end(&mut h, "Brush raise", true);
    assert_eq!(h.state().globe.brush.noise_raise_m, 6000.0);
    type_value(&mut h, "Brush seed", "999");
    assert_eq!(h.state().doc.project.world.seed, 999);
    step(&mut h, "Brush seed", false);
    assert_eq!(h.state().doc.project.world.seed, 998);
}

#[test]
fn the_vector_cell_is_off_with_no_vector_layer_and_follows_the_kind() {
    use vale_store::GeometryKind;
    let mut h = pad(BOARDS[0]);
    let draw = draw_cell(&h);
    h.state_mut().select(None);
    h.run_steps(2);
    assert!(disabled(&h, VECTOR));
    assert!(control(&h, VECTOR).is_none());
    assert_eq!(h.state().pad.vector_face, None);
    for (kind, name, tool) in [
        (GeometryKind::Line, LINE, Tool::Line),
        (GeometryKind::Polygon, POLYGON, Tool::Polygon),
        (GeometryKind::Point, vale_app::ui::pad::POINT, Tool::Point),
    ] {
        let id = h.state_mut().doc.add_layer(kind);
        h.state_mut().select(Some(LayerRef::Vector(id)));
        h.run_steps(2);
        assert_eq!(control(&h, VECTOR), Some(draw), "{name}");
        assert_eq!(h.state().pad.vector_face, Some(name));
        quick_tap(&mut h, VECTOR);
        assert_eq!(h.state().globe.tool, tool, "{name}");
        assert_eq!(toggled(&h, VECTOR), Some(true), "{name}");
    }
    h.state_mut().select(None);
    h.run_steps(2);
    assert!(disabled(&h, VECTOR));
}

#[test]
fn the_brush_panel_has_the_two_sliders_of_the_uplift_mode() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Brush settings");
    assert!(control(&h, "Brush erosion").is_none());
    choose_mode(&mut h, "Uplift");
    let s = h.state();
    assert_eq!(
        (s.globe.tool, s.globe.brush.mode),
        (Tool::Brush, Mode::Uplift)
    );
    assert_eq!(toggled(&h, TERRAIN), Some(true));
    // The strength slider sets the rate of the uplift.
    slide_to_end(&mut h, "Brush strength", true);
    assert!((h.state().globe.brush.strength_m - 6000.0).abs() < 1e-6);
    slide_to_end(&mut h, "Brush erosion", false);
    assert_eq!(h.state().globe.brush.erosion, 0.0);
    slide_to_end(&mut h, "Brush erosion", true);
    assert_eq!(h.state().globe.brush.erosion, 1.0);
    assert!(control(&h, "Brush angle value").is_some());
    check_controls(&h, "the Uplift mode");
}

#[test]
fn the_brush_panel_has_the_wind_and_the_rain_shadow_of_the_erode_and_uplift_modes() {
    const ROWS: [&str; 2] = ["Brush wind", "Brush rain shadow"];
    for mode in [None, Some("Erode"), Some("Uplift"), Some("Warp")] {
        let mut h = pad(BOARDS[0]);
        tap(&mut h, "Brush settings");
        if let Some(mode) = mode {
            choose_mode(&mut h, mode);
        }
        let shown = matches!(mode, Some("Erode" | "Uplift"));
        for name in ROWS {
            assert_eq!(control(&h, name).is_some(), shown, "{mode:?} {name}");
        }
        if !shown {
            continue;
        }
        // The rows set the wind and the rain shadow of the world.
        let world = &h.state().doc.project.world;
        assert_eq!((world.wind_deg, world.rain_shadow), (270.0, 0.0));
        let r = control(&h, ROWS[0]).unwrap();
        finger_tap(&mut h, pos2(r.right() - 4.0, r.center().y));
        slide_to_end(&mut h, ROWS[1], true);
        let world = &h.state().doc.project.world;
        assert_eq!((world.wind_deg, world.rain_shadow), (90.0, 1.0), "{mode:?}");
        let rain = Rain {
            wind: 90.0,
            shadow: 1.0,
        };
        assert_eq!(h.state().globe.world_rain, rain, "{mode:?}");
        check_controls(&h, "the wind and the rain shadow");
    }
}

/// Whether the switch with this name is on.
fn switch_is_on(h: &Pad, name: &str) -> bool {
    let switch = h.get_by_label(name);
    switch.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

#[test]
fn the_brush_panel_has_the_floor_and_the_ceiling() {
    const SWITCHES: [&str; 2] = ["Brush floor", "Brush ceiling"];
    const LEVELS: [&str; 2] = ["Brush floor level", "Brush ceiling level"];
    const FIELDS: [&str; 2] = ["Brush floor level value", "Brush ceiling level value"];
    for mode in [
        None,
        Some("Erode"),
        Some("Uplift"),
        Some("Warp"),
        Some("Push"),
    ] {
        // The portrait iPad has the height for all the rows of each mode.
        let mut h = pad(BOARDS[1]);
        tap(&mut h, "Brush settings");
        if let Some(mode) = mode {
            choose_mode(&mut h, mode);
        }
        let what = format!("{mode:?}");
        // Both switches are off at the start, and no level shows.
        let brush = h.state().globe.brush;
        assert!(!brush.floor_on && !brush.ceiling_on, "{what}");
        assert_eq!((brush.floor_m, brush.ceiling_m), (0.0, 1000.0), "{what}");
        for (switch, level) in SWITCHES.iter().zip(FIELDS) {
            assert!(!switch_is_on(&h, switch), "{what}: {switch}");
            assert!(control(&h, level).is_none(), "{what}: {level}");
        }
        check_controls(&h, &format!("{what} with the limits off"));

        // The floor goes on alone, and its level shows.
        tap(&mut h, SWITCHES[0]);
        let brush = h.state().globe.brush;
        assert!(brush.floor_on && !brush.ceiling_on, "{what}");
        assert!(switch_is_on(&h, SWITCHES[0]) && !switch_is_on(&h, SWITCHES[1]));
        assert!(control(&h, FIELDS[0]).is_some() && control(&h, FIELDS[1]).is_none());
        // The switch and its level are two rows of the panel, and the
        // stepper of the level ends at the right of the row.
        let switch = control(&h, SWITCHES[0]).unwrap();
        let plus = control(&h, &format!("Increase {}", LEVELS[0])).unwrap();
        let size = control(&h, "Brush size").unwrap();
        let below = plus.center().y - switch.center().y;
        assert_eq!(below, switch.height(), "{what}");
        assert_eq!(switch.left(), size.left(), "{what}");
        assert_eq!(plus.right(), switch.right(), "{what}");

        tap(&mut h, SWITCHES[1]);
        let brush = h.state().globe.brush;
        assert!(brush.floor_on && brush.ceiling_on, "{what}");
        check_controls(&h, &format!("{what} with the limits on"));

        // A level has the meters of the layer, in steps of 50 m.
        type_value(&mut h, LEVELS[0], "-9000");
        assert_eq!(h.state().globe.brush.floor_m, -6000.0, "{what}");
        type_value(&mut h, LEVELS[0], "-1234");
        let floor = h.state().globe.brush.floor_m;
        assert_eq!(floor, -1250.0, "{what}");
        step(&mut h, LEVELS[0], true);
        assert_eq!(h.state().globe.brush.floor_m, -1200.0, "{what}");
        step(&mut h, LEVELS[0], false);
        assert_eq!(h.state().globe.brush.floor_m, floor, "{what}");
        type_value(&mut h, LEVELS[1], "6000");
        assert_eq!(h.state().globe.brush.ceiling_m, 6000.0, "{what}");
        // The floor can be above the ceiling.
        type_value(&mut h, LEVELS[1], "-6000");
        let brush = h.state().globe.brush;
        assert_eq!(brush.ceiling_m, -6000.0, "{what}");
        assert!(brush.floor_m > brush.ceiling_m, "{what}");
        let limits = brush.limits();
        assert_eq!(limits.floor, meters_to_level(floor), "{what}");
        assert_eq!(limits.ceiling, 0, "{what}");

        // A tap turns a limit off, and its level keeps its meters.
        tap(&mut h, SWITCHES[0]);
        let brush = h.state().globe.brush;
        assert!(!brush.floor_on && brush.ceiling_on, "{what}");
        assert_eq!(brush.floor_m, floor, "{what}");
        assert!(control(&h, FIELDS[0]).is_none() && control(&h, FIELDS[1]).is_some());
        assert_eq!(brush.limits().floor, 0, "{what}");
    }

    // The mode with the most rows, with both levels, on each board.
    for size in BOARDS {
        let mut h = pad(size);
        let brush = &mut h.state_mut().globe.brush;
        (brush.mode, brush.floor_on, brush.ceiling_on) = (Mode::Uplift, true, true);
        tap(&mut h, "Brush settings");
        h.run_steps(2);
        check_controls(&h, &format!("{size:?} with the limits on"));
        let shown = |h: &Pad| {
            let names = SWITCHES.iter().chain(&FIELDS);
            names.filter(|name| control(h, name).is_some()).count()
        };
        let sheet = h.state().pad.rects.unwrap().sheet.is_some();
        if sheet {
            assert_eq!(shown(&h), 0, "{size:?}");
        }
        // The card is too short for the rows. A finger that moves up on a
        // slider scrolls the rows and does not move the slider.
        const SLIDERS: [&str; 3] = ["Brush rain shadow", "Brush erosion", "Brush strength"];
        for _ in 0..3 {
            if shown(&h) == 4 {
                break;
            }
            let brush = h.state().globe.brush;
            let slider = SLIDERS.iter().find_map(|name| control(&h, name));
            let from = slider.unwrap().center();
            drag(&mut h, from, from - vec2(4.0, 260.0), None);
            assert_eq!(h.state().globe.brush, brush, "{size:?}");
            check_controls(&h, &format!("{size:?} after the scroll"));
        }
        assert_eq!(shown(&h), 4, "{size:?}");
        tap(&mut h, SWITCHES[1]);
        assert!(!h.state().globe.brush.ceiling_on, "{size:?}");
    }
}

/// The brush size of the brush that follows the zoom, in points, after a
/// touch on `path`, at each place of the path.
fn sizes_along(h: &mut Pad, path: &[Pos2]) -> Vec<f32> {
    let mut sizes = Vec::new();
    for (i, at) in path.iter().enumerate() {
        let phase = if i == 0 {
            egui::TouchPhase::Start
        } else {
            egui::TouchPhase::Move
        };
        touch(h, phase, *at, None);
        h.step();
        sizes.push(h.state().globe.brush.size_points);
    }
    touch(h, egui::TouchPhase::End, *path.last().unwrap(), None);
    h.run_steps(3);
    sizes
}

/// The distance on the track that moves the size of the Brush panel from
/// `from` to `to`, in points.
fn size_distance(h: &Pad, from: f32, to: f32) -> f32 {
    // The track has a log scale. It starts LABEL_COLUMN points from the left
    // of the row and ends 8 points from the value field, and the handle
    // stays TRACK_INSET from each end.
    let globe = &h.state().globe;
    let world_km = h.state().doc.project.world.radius_km;
    let range = globe.brush.radius_km_range(globe.radius(), world_km);
    let span = (range.end() / range.start()).ln() as f32;
    let row = control(h, "Brush size").unwrap();
    let length = row.width() - LABEL_COLUMN - 8.0 - 2.0 * TRACK_INSET;
    (to / from).ln() / span * length
}

#[test]
fn a_drag_on_the_size_moves_it_by_the_distance_of_the_drag() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Brush settings");
    h.state_mut().globe.brush.size_points = 20.0;
    h.run_steps(2);
    let c = control(&h, "Brush size").unwrap().center();

    // A touch with no drag changes nothing.
    let sizes = sizes_along(&mut h, &[c]);
    assert_eq!(sizes, [20.0]);
    assert_eq!(h.state().globe.brush.size_points, 20.0);

    // A drag of 30 points moves the thumb by 30 points, from where it was.
    let to = c + vec2(30.0, 0.0);
    let path: Vec<_> = (0..=6).map(|i| c.lerp(to, i as f32 / 6.0)).collect();
    sizes_along(&mut h, &path);
    let size = h.state().globe.brush.size_points;
    let moved = size_distance(&h, 20.0, size);
    assert!((moved - 30.0).abs() < 1.0, "{size} {moved}");
}

#[test]
fn a_drag_far_from_the_track_moves_the_size_8_times_less() {
    // The change of a 24 point move, on the track or 200 points below it.
    let moved = |below: f32| {
        let mut h = pad(BOARDS[0]);
        tap(&mut h, "Brush settings");
        // Far from the default size, where the thumb stops.
        h.state_mut().globe.brush.size_points = 8.0;
        h.run_steps(2);
        let c = control(&h, "Brush size").unwrap().center();
        let start = c + vec2(10.0, 0.0);
        let down = start + vec2(0.0, below);
        let path = [
            c,
            start,
            down,
            down + vec2(12.0, 0.0),
            down + vec2(24.0, 0.0),
        ];
        let sizes = sizes_along(&mut h, &path);
        size_distance(&h, sizes[2], sizes[4])
    };
    let (near, far) = (moved(0.0), moved(200.0));
    assert!((near - 24.0).abs() < 1.0, "{near}");
    assert!((far - 3.0).abs() < 0.5, "{far}");
}

#[test]
fn a_tap_on_the_size_value_opens_the_keypad_and_a_typed_size_sets_it() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Brush settings");
    let km = |h: &Pad| {
        let globe = &h.state().globe;
        let world_km = h.state().doc.project.world.radius_km;
        globe.brush.radius_km(globe.radius(), world_km)
    };
    let range = {
        let globe = &h.state().globe;
        let world_km = h.state().doc.project.world.radius_km;
        globe.brush.radius_km_range(globe.radius(), world_km)
    };
    assert!(range.contains(&1000.0), "{range:?}");
    assert!(control(&h, "Brush size value 1").is_none());
    tap(&mut h, "Brush size value");
    assert!(control(&h, "Brush size value 1").is_some());
    for c in ["1", "0", "0", "0", "Done"] {
        tap(&mut h, &format!("Brush size value {c}"));
    }
    assert!(control(&h, "Brush size value 1").is_none());
    assert!((km(&h) - 1000.0).abs() < 0.01, "{}", km(&h));
}

#[test]
fn a_tap_outside_the_keypad_closes_it_and_keeps_the_typed_value() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Brush settings");
    tap(&mut h, "Brush ceiling");
    let name = "Brush ceiling level value";
    tap(&mut h, name);
    for c in ["1", "2", "0", "0"] {
        tap(&mut h, &format!("{name} {c}"));
    }
    // A tap on the keypad keeps it open, and a drag on the globe does too.
    assert!(control(&h, &format!("{name} 1")).is_some());
    let at = h.state().pad.rects.unwrap().screen.center() + vec2(250.0, 150.0);
    drag(&mut h, at, at + vec2(60.0, 0.0), None);
    assert!(control(&h, &format!("{name} 1")).is_some());
    // A tap on the globe closes it.
    let at = h.state().pad.rects.unwrap().screen.center() + vec2(250.0, 150.0);
    touch(&mut h, egui::TouchPhase::Start, at, None);
    h.step();
    touch(&mut h, egui::TouchPhase::End, at, None);
    h.run_steps(3);
    assert!(control(&h, &format!("{name} 1")).is_none());
    assert_eq!(h.state().globe.brush.ceiling_m, 1200.0);
}

#[test]
fn the_ceiling_level_is_a_stepper_of_50_m_and_takes_a_typed_level() {
    let mut h = pad(BOARDS[1]);
    tap(&mut h, "Brush settings");
    tap(&mut h, "Brush ceiling");
    assert_eq!(h.state().globe.brush.ceiling_m, 1000.0);
    step(&mut h, "Brush ceiling level", true);
    assert_eq!(h.state().globe.brush.ceiling_m, 1050.0);
    step(&mut h, "Brush ceiling level", false);
    step(&mut h, "Brush ceiling level", false);
    assert_eq!(h.state().globe.brush.ceiling_m, 950.0);
    type_value(&mut h, "Brush ceiling level", "1200");
    assert_eq!(h.state().globe.brush.ceiling_m, 1200.0);
}

#[test]
fn the_brush_panel_locks_the_size_and_sets_the_flatten_level() {
    use vale_app::ui::pad::{FLATTEN_LEVEL, LOCK_SIZE, PICK_LEVEL, STROKE_START};
    let mut h = pad(BOARDS[1]);
    tap(&mut h, "Brush settings");
    assert!(h.state().globe.brush.lock.is_none());
    tap(&mut h, LOCK_SIZE);
    assert!(h.state().globe.brush.lock.is_some());
    assert!(control(&h, PICK_LEVEL).is_none());

    choose_mode(&mut h, "Flatten");
    assert_eq!(h.state().globe.brush.flatten_level, None);
    assert!(h.query_by_label("Level at stroke start").is_some());
    tap(&mut h, PICK_LEVEL);
    assert!(h.state().globe.pick_level);
    let brush = &mut h.state_mut().globe.brush;
    brush.flatten_level = Some(brush.level(1000.0));
    h.run_steps(2);
    type_value(&mut h, FLATTEN_LEVEL, "1200");
    let brush = h.state().globe.brush;
    let level = brush.value(brush.flatten_level.unwrap());
    assert!((level - 1200.0).abs() < 0.2, "{level}");
    tap(&mut h, STROKE_START);
    assert_eq!(h.state().globe.brush.flatten_level, None);
    check_controls(&h, "the Flatten mode");
}

#[test]
fn the_brush_panel_has_the_two_sliders_of_the_warp_mode() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Brush settings");
    assert!(control(&h, "Brush scale").is_none());
    choose_mode(&mut h, "Warp");
    let s = h.state();
    assert_eq!(
        (s.globe.tool, s.globe.brush.mode),
        (Tool::Brush, Mode::Warp)
    );
    assert_eq!(toggled(&h, SCULPT), Some(true));
    assert_eq!(
        (s.globe.brush.warp_km, s.globe.brush.scale_km),
        (50.0, 200.0)
    );
    // The strength slider sets the offset in kilometers.
    let meters = s.globe.brush.strength_m;
    slide_to_end(&mut h, "Brush strength", true);
    assert!((h.state().globe.brush.warp_km - 500.0).abs() < 1e-6);
    assert_eq!(h.state().globe.brush.strength_m, meters);
    slide_to_end(&mut h, "Brush scale", false);
    assert!((h.state().globe.brush.scale_km - 5.0).abs() < 1e-9);
    slide_to_end(&mut h, "Brush scale", true);
    assert!((h.state().globe.brush.scale_km - 2000.0).abs() < 1e-6);
    // The seed stepper sets the seed of the world.
    assert_eq!(h.state().doc.project.world.seed, 0);
    // The minus button is off at the lowest seed.
    assert!(control(&h, "Decrease Brush seed").is_none());
    step(&mut h, "Brush seed", true);
    assert_eq!(h.state().doc.project.world.seed, 1);
    type_value(&mut h, "Brush seed", "999");
    assert_eq!(h.state().doc.project.world.seed, 999);
    assert_eq!(h.state().globe.world_seed, 999);
    assert!(control(&h, "Brush angle value").is_none());
    check_controls(&h, "the Warp mode");
}

/// The rows of the ocean ridge mode in the brush panel.
const RIDGE_ROWS: [&str; 5] = [
    "Brush crest",
    "Brush spreading",
    "Brush magma",
    "Brush segment",
    "Brush offset",
];

/// The values of the ocean ridge sliders, in the order of `RIDGE_ROWS`.
fn ridge_values(h: &Pad) -> [f64; 5] {
    let b = h.state().globe.brush;
    [
        b.ridge_crest_m,
        b.ridge_spread_mm,
        b.ridge_magma,
        b.ridge_segment_km,
        b.ridge_offset_km,
    ]
}

/// The line with the age of the crust at the edge of the brush.
fn edge_age_line(h: &Pad) -> String {
    let s = h.state();
    let km = s
        .globe
        .brush
        .radius_km(s.globe.radius(), s.doc.project.world.radius_km);
    let age = km / (0.5 * s.globe.brush.ridge_spread_mm);
    assert!(age >= 10.0, "{age}");
    format!("Edge age {age:.0} Myr")
}

#[test]
fn the_brush_panel_has_the_five_sliders_of_the_ocean_ridge_mode() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Brush settings");
    let field = |h: &Pad, row: &str| control(h, &format!("{row} value"));
    for row in RIDGE_ROWS {
        assert!(field(&h, row).is_none(), "{row}");
    }
    assert!(control(&h, "Brush hardness").is_some());
    choose_mode(&mut h, "Ocean ridge");
    let s = h.state();
    assert_eq!(
        (s.globe.tool, s.globe.brush.mode),
        (Tool::Brush, Mode::OceanRidge)
    );
    assert_eq!(ridge_values(&h), [-700.0, 25.0, 0.0, 400.0, 100.0]);
    for row in RIDGE_ROWS {
        assert!(field(&h, row).is_some(), "{row}");
    }
    assert!(h.query_by_label(&edge_age_line(&h)).is_some());
    // The size and the flow stay. The mode has no hardness and no strength.
    for row in ["Brush size", "Brush flow"] {
        assert!(control(&h, row).is_some(), "{row}");
    }
    for row in ["Brush hardness", "Brush strength", "Brush seed"] {
        assert!(field(&h, row).is_none(), "{row}");
    }
    // Each row takes the values from one end of its range to the other, and
    // a typed value past an end gives the end.
    let ends = [
        (("-9000", -6000.0), ("9000", 6000.0)),
        (("0", 10.0), ("900", 150.0)),
        (("-90", -0.25), ("90", 0.15)),
        (("0", 100.0), ("9000", 1500.0)),
        (("-5", 0.0), ("900", 500.0)),
    ];
    for (i, (row, ends)) in RIDGE_ROWS.into_iter().zip(ends).enumerate() {
        for (text, value) in [ends.0, ends.1] {
            type_value(&mut h, row, text);
            assert!((ridge_values(&h)[i] - value).abs() < 1e-6, "{row} {text}");
        }
    }
    // The edge age follows the spreading, in steps of 1 mm/yr.
    type_value(&mut h, "Brush spreading", "60");
    step(&mut h, "Brush spreading", false);
    assert_eq!(h.state().globe.brush.ridge_spread_mm, 59.0);
    assert!(h.query_by_label(&edge_age_line(&h)).is_some());
    // The crest moves in steps of 50 m.
    step(&mut h, "Brush crest", false);
    assert_eq!(h.state().globe.brush.ridge_crest_m, 5950.0);
    type_value(&mut h, "Brush crest", "1234");
    assert_eq!(h.state().globe.brush.ridge_crest_m, 1250.0);
    check_controls(&h, "the Ocean ridge mode");
}

#[test]
fn the_ocean_ridge_mode_fits_on_the_smallest_ipad() {
    let mut h = pad((1133.0, 744.0));
    open_group(&mut h, ModeGroup::Terrain);
    let menu = h.state().pad.flyouts.rect.unwrap();
    let screen = h.state().pad.rects.unwrap().screen;
    assert!(screen.contains_rect(menu), "{menu:?}");
    // The flyout scrolls to its last row.
    let from = menu.center();
    drag(&mut h, from, from - vec2(0.0, 0.4 * menu.height()), None);
    let row = control(&h, "Ocean ridge").unwrap();
    assert!(menu.contains_rect(row), "{row:?}");
    check_controls(&h, "744 high with the brush modes");
    tap(&mut h, "Ocean ridge");
    tap(&mut h, "Brush settings");
    // Each row of the panel and the edge age line show.
    let (_, panel) = h.state().pad.panels[0];
    assert!(screen.contains_rect(panel), "{panel:?}");
    let rows = ["Brush size", "Brush flow"].into_iter().chain(RIDGE_ROWS);
    let fields = rows.map(|row| format!("{row} value"));
    for row in fields.chain(["Brush floor", "Brush ceiling"].map(String::from)) {
        let rect = control(&h, &row).unwrap_or_else(|| panic!("no control `{row}`"));
        assert!(panel.contains_rect(rect), "{row}: {rect:?} in {panel:?}");
    }
    let line = h.get_by_label(&edge_age_line(&h)).rect();
    assert!(panel.contains_rect(line), "{line:?} in {panel:?}");
    check_controls(&h, "744 high with the Ocean ridge panel");
    // With the two limits on, the panel stays on the screen, and the rows
    // scroll to the level of the ceiling.
    tap(&mut h, "Brush floor");
    tap(&mut h, "Brush ceiling");
    let brush = h.state().globe.brush;
    assert!(brush.floor_on && brush.ceiling_on);
    let (_, panel) = h.state().pad.panels[0];
    assert!(screen.contains_rect(panel), "{panel:?}");
    let from = control(&h, "Brush magma").unwrap().center();
    drag(&mut h, from, from - vec2(4.0, 200.0), None);
    assert_eq!(h.state().globe.brush, brush);
    for row in ["Brush floor level value", "Brush ceiling level value"] {
        let rect = control(&h, row).unwrap_or_else(|| panic!("no control `{row}`"));
        assert!(panel.contains_rect(rect), "{row}: {rect:?} in {panel:?}");
    }
    check_controls(&h, "744 high with the Ocean ridge limits");
}

#[test]
fn the_tool_strip_has_the_range_mode() {
    use vale_app::ui::pad::icons::Icon;
    let range = (panels::RANGE, Icon::Range, Mode::Range);
    assert!(ModeGroup::Terrain.modes().contains(&range));
    for size in [BOARDS[0], (1133.0, 744.0)] {
        let mut h = pad(size);
        open_group(&mut h, ModeGroup::Terrain);
        let flyout = h.state().pad.flyouts.rect.unwrap();
        // The flyout scrolls to its last row.
        let from = flyout.center();
        drag(&mut h, from, from - vec2(0.0, 0.4 * flyout.height()), None);
        let row = control(&h, "Range").unwrap();
        assert!(flyout.contains_rect(row), "{size:?}: {row:?}");
        tap(&mut h, "Range");
        let s = h.state();
        assert_eq!(
            (s.globe.tool, s.globe.brush.mode),
            (Tool::Brush, Mode::Range)
        );
        assert_eq!(s.pad.brush_faces[1], Some(("Range", Icon::Range)));
        assert_eq!(toggled(&h, TERRAIN), Some(true));
        check_controls(&h, &format!("{size:?} with the Range mode"));
    }
}

/// The rows of the range mode in the brush panel.
const RANGE_ROWS: [&str; 6] = [
    "Brush strength",
    "Brush spacing",
    "Brush seed",
    "Brush erosion",
    "Brush wind",
    "Brush rain shadow",
];

#[test]
fn the_brush_panel_has_the_sliders_of_the_range_mode() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Brush settings");
    for row in ["Brush spacing", "Brush seed", "Brush erosion"] {
        assert!(row_of(&h, row).is_none(), "{row}");
    }
    choose_mode(&mut h, "Range");
    assert_eq!(h.state().globe.brush.mode, Mode::Range);
    for row in ["Brush size", "Brush flow"].into_iter().chain(RANGE_ROWS) {
        assert!(row_of(&h, row).is_some(), "{row}");
    }
    for row in [
        "Brush hardness",
        "Brush angle",
        "Brush crest",
        "Brush scale",
    ] {
        assert!(row_of(&h, row).is_none(), "{row}");
    }
    // The spacing goes from one end of its range to the other.
    assert_eq!(h.state().globe.brush.range_spacing_km, 30.0);
    slide_to_end(&mut h, "Brush spacing", false);
    assert!((h.state().globe.brush.range_spacing_km - 5.0).abs() < 1e-9);
    slide_to_end(&mut h, "Brush spacing", true);
    assert!((h.state().globe.brush.range_spacing_km - 300.0).abs() < 1e-6);
    // The seed stepper sets the seed of the world, as in the Warp mode.
    type_value(&mut h, "Brush seed", "999");
    assert_eq!(h.state().doc.project.world.seed, 999);
    assert_eq!(h.state().globe.world_seed, 999);
    // The erosion is the erosion of the Uplift mode.
    slide_to_end(&mut h, "Brush erosion", false);
    assert_eq!(h.state().globe.brush.erosion, 0.0);
    choose_mode(&mut h, "Uplift");
    assert_eq!(h.state().globe.brush.erosion, 0.0);
    check_controls(&h, "the Range mode");
}

#[test]
fn the_tool_strip_has_the_basin_mode() {
    use vale_app::ui::pad::icons::Icon;
    let basin = (panels::BASIN, Icon::Basin, Mode::Basin);
    assert!(ModeGroup::Terrain.modes().contains(&basin));
    assert_eq!(ModeGroup::of(Mode::Basin), ModeGroup::Terrain);
    for size in [BOARDS[0], (1133.0, 744.0)] {
        let mut h = pad(size);
        open_group(&mut h, ModeGroup::Terrain);
        let flyout = h.state().pad.flyouts.rect.unwrap();
        // The flyout scrolls to its last row.
        let from = flyout.center();
        drag(&mut h, from, from - vec2(0.0, 0.4 * flyout.height()), None);
        let row = control(&h, "Basin").unwrap();
        assert!(flyout.contains_rect(row), "{size:?}: {row:?}");
        tap(&mut h, "Basin");
        let s = h.state();
        assert_eq!(
            (s.globe.tool, s.globe.brush.mode),
            (Tool::Brush, Mode::Basin)
        );
        assert_eq!(s.pad.brush_faces[1], Some(("Basin", Icon::Basin)));
        assert_eq!(toggled(&h, TERRAIN), Some(true));
        check_controls(&h, &format!("{size:?} with the Basin mode"));
    }
}

/// The rows of the basin mode in the brush panel.
const BASIN_ROWS: [&str; 6] = [
    "Brush strength",
    "Brush density",
    "Brush erosion",
    "Brush seed",
    "Brush wind",
    "Brush rain shadow",
];

#[test]
fn the_brush_panel_has_the_sliders_of_the_basin_mode() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Brush settings");
    assert!(control(&h, "Brush density").is_none());
    choose_mode(&mut h, "Basin");
    assert_eq!(h.state().globe.brush.mode, Mode::Basin);
    for row in BASIN_ROWS {
        assert!(row_of(&h, row).is_some(), "{row}");
    }
    // The outline sets the size of a basin.
    for row in [
        "Brush size",
        "Brush hardness",
        "Brush flow",
        "Brush spacing",
        "Brush channel width",
        "Brush floodplain",
        "Brush angle",
    ] {
        assert!(row_of(&h, row).is_none(), "{row}");
    }
    // The density goes from one end of its range to the other.
    assert_eq!(h.state().globe.brush.basin_density_km, 40.0);
    slide_to_end(&mut h, "Brush density", false);
    assert!((h.state().globe.brush.basin_density_km - 10.0).abs() < 1e-9);
    slide_to_end(&mut h, "Brush density", true);
    assert!((h.state().globe.brush.basin_density_km - 500.0).abs() < 1e-6);
    // The seed stepper sets the seed of the world.
    type_value(&mut h, "Brush seed", "999");
    assert_eq!(h.state().globe.world_seed, 999);
    check_controls(&h, "the Basin mode");
}

#[test]
fn the_ocean_ridge_panel_has_no_buttons_before_a_stroke() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Brush settings");
    choose_mode(&mut h, "Ocean ridge");
    // The hint and the buttons show only while a ridge is held.
    assert!(h.query_by_label(panels::HELD_HINT).is_none());
    assert!(control(&h, panels::APPLY).is_none());
    assert!(control(&h, panels::DISCARD).is_none());
    check_controls(&h, "the Ocean ridge mode");
}

#[test]
fn the_basin_panel_shows_the_step_of_the_outline() {
    use vale_app::globe::math::lonlat_to_dir;
    for size in [BOARDS[0], BOARDS[1]] {
        let mut h = pad(size);
        tap(&mut h, "Brush settings");
        choose_mode(&mut h, "Basin");
        assert!(h.query_by_label(panels::BASIN_OUTLINE_HINT).is_some());
        assert!(control(&h, panels::CLEAR_OUTLINE).is_none());
        // The outline sets the size, so the size slider of the brush card is off.
        if let Some(rect) = control(&h, "Size") {
            let points = h.state().globe.brush.size_points;
            drag(&mut h, rect.center(), rect.center_top(), None);
            assert_eq!(h.state().globe.brush.size_points, points, "{size:?}");
        }
        // An outline waits for its main river.
        let outline = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        let outline = outline.map(|(lon, lat)| lonlat_to_dir(lon, lat)).to_vec();
        h.state_mut().globe.set_basin_outline(Some(outline));
        h.run_steps(2);
        assert!(h.query_by_label(panels::BASIN_RIVER_HINT).is_some());
        let button = control(&h, panels::CLEAR_OUTLINE).unwrap();
        assert!(button.height() >= 44.0, "{size:?}: {button:?}");
        check_controls(&h, &format!("{size:?} with an outline"));
        screenshot::save(&mut h, &format!("pad-basin-outline-{}", size.0));
        tap(&mut h, panels::CLEAR_OUTLINE);
        assert!(h.state().globe.basin_outline().is_none());
        assert!(h.query_by_label(panels::BASIN_OUTLINE_HINT).is_some());
        assert!(control(&h, panels::CLEAR_OUTLINE).is_none());
    }
}

#[test]
fn the_range_mode_fits_on_the_smallest_ipad() {
    let mut h = pad((1133.0, 744.0));
    let screen = h.state().pad.rects.unwrap().screen;
    open_group(&mut h, ModeGroup::Terrain);
    let menu = h.state().pad.flyouts.rect.unwrap();
    let from = menu.center();
    drag(&mut h, from, from - vec2(0.0, 0.4 * menu.height()), None);
    tap(&mut h, "Range");
    tap(&mut h, "Brush settings");
    // Each row of the panel shows, and the rows below the first ones scroll
    // up. A scroll changes no value.
    let brush = h.state().globe.brush;
    let (_, panel) = h.state().pad.panels[0];
    assert!(screen.contains_rect(panel), "{panel:?}");
    let rows = ["Brush size", "Brush flow"].into_iter().chain(RANGE_ROWS);
    for row in rows.chain(["Brush floor", "Brush ceiling"]) {
        for _ in 0..4 {
            if row_of(&h, row).is_some_and(|rect| panel.contains_rect(rect)) {
                break;
            }
            let from = control(&h, "Brush flow")
                .or_else(|| control(&h, "Brush rain shadow"))
                .unwrap()
                .center();
            drag(&mut h, from, from - vec2(4.0, 120.0), None);
        }
        let rect = row_of(&h, row).unwrap_or_else(|| panic!("no control `{row}`"));
        assert!(panel.contains_rect(rect), "{row}: {rect:?} in {panel:?}");
        check_controls(&h, &format!("744 high with the Range panel at {row}"));
    }
    assert_eq!(h.state().globe.brush, brush);
}

#[test]
fn the_brush_panel_has_the_channel_rows_of_the_carve_and_erode_modes() {
    let (width, plain) = ("Brush channel width", "Brush floodplain");
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Brush settings");
    for (mode, rows) in [
        ("Lower", [false, false]),
        ("Carve", [true, true]),
        ("Erode", [true, false]),
        ("Range", [false, false]),
        ("Basin", [false, false]),
        ("Noise", [false, false]),
        ("Push", [false, false]),
    ] {
        choose_mode(&mut h, mode);
        let shown = [row_of(&h, width).is_some(), row_of(&h, plain).is_some()];
        assert_eq!(shown, rows, "{mode}");
    }
    choose_mode(&mut h, "Carve");
    assert_eq!(h.state().globe.brush.mode, Mode::Carve);
    check_controls(&h, "the Carve mode");
    // The channel width goes from one end of its range to the other, and
    // its buttons go through 1, 2, 5, 10.
    let brush = h.state().globe.brush;
    assert_eq!((brush.channel_km, brush.floodplain_km), (0.6, 10.0));
    step(&mut h, width, true);
    assert!((h.state().globe.brush.channel_km - 1.0).abs() < 1e-9);
    type_value(&mut h, width, "0");
    assert!((h.state().globe.brush.channel_km - 0.05).abs() < 1e-9);
    type_value(&mut h, width, "99");
    assert!((h.state().globe.brush.channel_km - 10.0).abs() < 1e-6);
    // The floodplain has steps of 1 km.
    type_value(&mut h, plain, "999");
    assert_eq!(h.state().globe.brush.floodplain_km, 100.0);
    step(&mut h, plain, false);
    assert_eq!(h.state().globe.brush.floodplain_km, 99.0);
    type_value(&mut h, plain, "12.4");
    assert_eq!(h.state().globe.brush.floodplain_km, 12.0);
    // The Erode mode has the same channel width.
    choose_mode(&mut h, "Erode");
    assert!((h.state().globe.brush.channel_km - 10.0).abs() < 1e-6);
    check_controls(&h, "the Erode mode");
}

/// Writes an image of the brush panel in the ocean ridge mode, for a person
/// to look at.
#[test]
#[ignore]
fn screenshot_of_the_ocean_ridge_panel() {
    let mut state = state();
    state.add_height();
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let mut h = vale_app::headless::ui_harness(state, (1194.0, 834.0), 2.0, setup);
    h.run_steps(3);
    open_group(&mut h, ModeGroup::Terrain);
    h.render()
        .unwrap()
        .save("../../target/app/ocean-ridge-more.png")
        .unwrap();
    tap(&mut h, "Ocean ridge");
    tap(&mut h, "Brush settings");
    h.render()
        .unwrap()
        .save("../../target/app/ocean-ridge-panel.png")
        .unwrap();
}

#[test]
fn the_globe_does_not_move_when_you_touch_a_card() {
    for size in BOARDS {
        let mut h = pad(size);
        open_panel(&mut h, "Layers");
        let before = h.state().globe.view;
        let screen = h.state().globe.rect;
        if control(&h, "Workspace").is_some() {
            tap(&mut h, "Workspace");
            let menu = h.state().pad.menu_rect.unwrap();
            drag(
                &mut h,
                menu.left_top() + vec2(3.0, 3.0),
                screen.center(),
                None,
            );
            assert_eq!(h.state().globe.view, before, "{size:?}: the menu");
            // A touch outside the menu closes it.
            assert!(h.state().pad.menu);
            tap(&mut h, "Close Layers");
            assert!(!h.state().pad.menu);
            open_panel(&mut h, "Layers");
        }
        // The Move tool has no context strip.
        h.state_mut().globe.tool = Tool::Navigate;
        h.run_steps(2);
        for card in cards(&h) {
            for from in [card.center(), card.left_top() + vec2(3.0, 3.0)] {
                let to = screen.center();
                // A finger, and then a pen in the Move tool. Each one turns the globe on the canvas.
                h.state_mut().globe.tool = Tool::Navigate;
                drag(&mut h, from, to, None);
                drag(&mut h, from, to, Some(0.5));
                assert_eq!(
                    h.state().globe.view,
                    before,
                    "{size:?}: {card:?} from {from:?}"
                );
            }
        }
        // The same touch on the canvas turns the globe.
        let from = pos2(screen.center().x + 20.0, 250.0);
        drag(&mut h, from, from + vec2(40.0, 30.0), None);
        assert_ne!(h.state().globe.view, before, "{size:?}");
    }
}

#[test]
fn one_finger_turns_the_globe_and_two_fingers_zoom_it() {
    let mut h = pad(BOARDS[0]);
    assert_eq!(h.state().globe.tool, Tool::Brush);
    let c = h.state().globe.rect.center();
    let before = h.state().globe.view;
    drag(&mut h, c, c + vec2(60.0, 0.0), None);
    let turned = h.state().globe.view;
    assert_ne!(turned.rot, before.rot);
    assert_eq!(turned.zoom, before.zoom);

    let finger = |h: &mut Pad, id: u64, phase: egui::TouchPhase, pos: Pos2| {
        h.event(egui::Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(id),
            phase,
            pos,
            force: None,
        });
    };
    let (a, b) = (c - vec2(50.0, 0.0), c + vec2(50.0, 0.0));
    finger(&mut h, 1, egui::TouchPhase::Start, a);
    finger(&mut h, 2, egui::TouchPhase::Start, b);
    h.step();
    finger(&mut h, 1, egui::TouchPhase::Move, a - vec2(50.0, 0.0));
    finger(&mut h, 2, egui::TouchPhase::Move, b + vec2(50.0, 0.0));
    h.step();
    finger(&mut h, 1, egui::TouchPhase::End, a - vec2(50.0, 0.0));
    finger(&mut h, 2, egui::TouchPhase::End, b + vec2(50.0, 0.0));
    h.step();
    assert!(h.state().globe.view.zoom > turned.zoom * 1.5);
}

/// One test at a time makes a wgpu device.
fn one_gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The layout on the wgpu renderer, with a height layer that the pen paints.
fn gpu_pad(size: (f32, f32)) -> Pad {
    let mut state = state();
    state.globe.set_face_size(256);
    state.add_height();
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let size = (f64::from(size.0), f64::from(size.1));
    let mut h = headless::ui_harness(state, size, 1.0, setup);
    h.set_render_every_step(true);
    h.run_steps(3);
    h
}

fn settle(h: &mut Pad) {
    for _ in 0..400 {
        h.step();
        if !h.state().globe.busy() {
            return;
        }
    }
    panic!("the stroke did not end");
}

/// A harness with the Brush and Layers panels open, in greyscale, and with a
/// globe that covers the screen. `level` gives the level at a longitude in
/// degrees.
fn blur_pad(level: impl Fn(f64) -> u16) -> Pad {
    blur_pad_on(BOARDS[0], level)
}

/// `blur_pad` on a screen of `size`.
fn blur_pad_on(size: (f32, f32), level: impl Fn(f64) -> u16) -> Pad {
    let mut h = gpu_pad(size);
    let state = h.state_mut();
    // The vector layers of the sample world are hidden.
    let layers: Vec<_> = state.doc.project.layers().iter().map(|l| l.id).collect();
    for id in layers {
        let raster = matches!(
            state.doc.project.layer(id).unwrap().kind,
            LayerKind::Raster(_)
        );
        state.set_layer_visible(id, raster);
    }
    state.pad.docks.world.open_new(Panel::Brush);
    state.pad.docks.world.open_new(Panel::Layers);
    show_as(state, RasterDisplay::Grey);
    let globe = &mut state.globe;
    globe.preview.graticule = false;
    globe.view = GlobeView::centered(0.0, 0.0);
    globe.view.zoom = 3.0;
    let n = globe.map.face_size();
    for face in 0..6 {
        for (x, y) in (0..n).flat_map(|y| (0..n).map(move |x| (x, y))) {
            let d = globe.map.texel_dir(face, x, y);
            let level = level(d[1].atan2(d[0]).to_degrees());
            globe.map.set(face, x, y, level);
        }
    }
    h.run_steps(3);
    h
}

/// The red values of a row of pixels near the bottom of a card, inside its
/// round corners. No control is there.
fn card_row(image: &image::RgbaImage, card: Rect) -> Vec<i32> {
    let y = card.bottom() as u32 - 3;
    let (left, right) = (card.left() as u32 + 16, card.right() as u32 - 16);
    let red = |x| i32::from(image.get_pixel(x, y).0[0]);
    (left..right).map(red).collect()
}

fn difference(a: &[i32], b: &[i32]) -> i32 {
    a.iter().zip(b).map(|(a, b)| (a - b).abs()).sum()
}

#[test]
#[ignore = "gpu"]
fn each_card_shows_the_blurred_canvas_and_the_blur_follows_the_globe() {
    let _gpu = one_gpu_test();
    // Light and dark stripes along the meridians, each 6 degrees wide, so
    // that the edge of a stripe is below each card.
    let mut h = blur_pad(|lon| match (lon / 6.0).floor().rem_euclid(2.0) {
        0.0 => u16::MAX,
        _ => 0,
    });
    let cards = cards(&h);
    assert!(cards.len() >= 6);
    let blurred = h.render().unwrap();
    std::fs::create_dir_all("../../target/app").unwrap();
    blurred.save("../../target/app/test-pad-blur.png").unwrap();
    h.state_mut().globe.blur = false;
    h.run_steps(2);
    let plain = h.render().unwrap();
    h.state_mut().globe.blur = true;
    h.state_mut().globe.view = GlobeView::centered(3.0, 0.0);
    h.state_mut().globe.view.zoom = 3.0;
    h.run_steps(2);
    let turned = h.render().unwrap();

    for card in cards {
        let row = card_row(&blurred, card);
        let width = row.len() as i32;
        // The edge of a stripe is soft below a card.
        let step = row.windows(2).map(|w| (w[0] - w[1]).abs()).max().unwrap();
        assert!(step <= 4, "{card:?}: a step of {step}");
        assert!(
            difference(&row, &card_row(&plain, card)) > width * 2,
            "{card:?}"
        );
        // A turn of half a stripe moves the stripes below the card.
        assert!(
            difference(&row, &card_row(&turned, card)) > width * 2,
            "{card:?}"
        );
    }
}

#[test]
#[ignore = "gpu"]
fn the_blur_behind_a_card_shows_the_visible_vector_layers_in_the_draw_order() {
    use vale_store::{Feature, Geometry};
    let _gpu = one_gpu_test();
    let mut h = blur_pad(|_| 0);
    // A red polygon covers the side of the globe that faces the view.
    let state = h.state_mut();
    let polygon = state.doc.add_layer(GeometryKind::Polygon);
    let ring = vec![[-80.0, -70.0], [80.0, -70.0], [80.0, 70.0], [-80.0, 70.0]];
    let feature = Feature::new(Geometry::Polygons(vec![vec![ring]]), Default::default());
    assert!(state.doc.project.add_feature(polygon, feature));
    let style = &mut state.doc.entry_mut(polygon).unwrap().style;
    style.set_fill(Some(vale_style::Color::rgb(255, 0, 0)));
    state.touch();
    h.run_steps(2);
    let cards = cards(&h);
    // The red values below the cards, with the polygon shown and hidden.
    let reds = |h: &mut Pad, shown: bool| {
        h.state_mut().set_layer_visible(polygon, shown);
        h.run_steps(2);
        let image = h.render().unwrap();
        let rows: Vec<_> = cards.iter().map(|card| card_row(&image, *card)).collect();
        rows.concat()
    };
    let (red, plain) = (reds(&mut h, true), reds(&mut h, false));
    let width = red.len() as i32;
    assert!(difference(&red, &plain) > width * 20);
    assert!(red.iter().zip(&plain).all(|(r, p)| r >= p));

    // The flat view has the polygon in the blur too. A close view puts
    // the polygon below the cards.
    h.state_mut().globe.world_view = WorldView::Flat;
    h.state_mut().globe.flat.zoom = 3.0;
    let (red, plain) = (reds(&mut h, true), reds(&mut h, false));
    assert!(difference(&red, &plain) > width * 2);
    h.state_mut().globe.flat.zoom = 1.0;
    h.state_mut().globe.world_view = WorldView::Globe;

    // A raster mask cuts the polygon in the blur too. The whole world is
    // below sea level, so a mask of the sea keeps the polygon, and a mask
    // of the land leaves nothing.
    let raster = (h.state().doc.project.layers().iter())
        .find(|l| matches!(l.kind, LayerKind::Raster(_)))
        .unwrap()
        .id;
    let mask = |h: &mut Pad, min, max| {
        let state = h.state_mut();
        let style = &mut state.doc.entry_mut(polygon).unwrap().style.style;
        style.mask = Some(vale_style::Mask::Raster {
            layer: raster,
            min,
            max,
            offset: 0.0,
        });
        state.sync_globe();
    };
    let (red, plain) = (reds(&mut h, true), reds(&mut h, false));
    mask(&mut h, None, Some(0.0));
    let sea = reds(&mut h, true);
    assert!(difference(&sea, &plain) > width * 20);
    assert!(difference(&sea, &red) <= width * 2);
    mask(&mut h, Some(0.0), None);
    assert!(difference(&reds(&mut h, true), &plain) <= width);
    let state = h.state_mut();
    state.doc.entry_mut(polygon).unwrap().style.style.mask = None;
    state.sync_globe();

    // A raster layer above the polygon hides it.
    let top = h.state().doc.project.layers().len() - 1;
    assert!(h.state_mut().move_layer(raster, top));
    let (red, plain) = (reds(&mut h, true), reds(&mut h, false));
    assert!(difference(&red, &plain) <= width);
}

#[test]
fn the_keypad_of_the_brush_size_stays_clear_of_the_dock() {
    let mut h = docked(BOARDS[1], &[&[Panel::Brush], &[Panel::Layers]]);
    for width in [340.0, 520.0] {
        h.state_mut().pad.docks.world.width = width;
        h.run_steps(2);
        tap(&mut h, "Brush size value");
        let keypad = h
            .ctx
            .memory(|m| m.area_rect(egui::Id::new("Brush size keypad")))
            .unwrap();
        let dock = h.state().pad.dock_rect.unwrap();
        assert!(!keypad.intersects(dock), "{width}: {keypad:?} {dock:?}");
        tap(&mut h, "Brush size value Done");
    }
}

/// The contrast ratio of two colors, as WCAG defines it.
fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let luminance = |c: [u8; 3]| {
        let linear = c.map(|v| {
            let v = f64::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        });
        0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2]
    };
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[test]
#[ignore = "gpu"]
fn text_on_a_card_is_easy_to_read_over_a_white_canvas() {
    let _gpu = one_gpu_test();
    let mut h = blur_pad(|_| u16::MAX);
    let image = h.render().unwrap();
    let c = h.state().globe.rect.center();
    let canvas = image.get_pixel(c.x as u32, c.y as u32).0;
    assert!(canvas[..3].iter().all(|v| *v > 240), "{canvas:?}");
    for card in cards(&h) {
        let pixel = image.get_pixel(card.left() as u32 + 16, card.bottom() as u32 - 3);
        let fill = [pixel.0[0], pixel.0[1], pixel.0[2]];
        // The canvas shows through the card.
        assert!(fill[0] > 55, "{card:?}: {fill:?}");
        for text in [theme::TEXT, theme::MUTE, theme::WARN] {
            let ratio = contrast([text.r(), text.g(), text.b()], fill);
            assert!(ratio >= 4.5, "{card:?}: {ratio:.2} for {text:?}");
        }
    }
}

#[test]
#[ignore = "gpu"]
fn the_pen_paints_on_the_canvas_and_a_panel_stays_open() {
    let _gpu = one_gpu_test();
    let mut h = gpu_pad(BOARDS[0]);
    open_panel(&mut h, "Layers");
    tap(&mut h, "Brush settings");
    let c = h.state().globe.rect.center();

    // A pen that starts on a card paints nothing.
    for card in cards(&h) {
        drag(&mut h, card.center(), c, Some(0.5));
        settle(&mut h);
        assert!(!h.state().globe.can_undo(), "{card:?}");
    }
    assert!(control(&h, "Undo").is_none());

    let before = h.state().globe.view;
    drag(&mut h, c - vec2(40.0, 0.0), c + vec2(40.0, 0.0), Some(0.5));
    settle(&mut h);
    assert!(h.state().globe.can_undo());
    assert_eq!(h.state().globe.view, before);
    assert!(h.state().pad.is_open(Panel::Layers));
    assert!(h.state().pad.is_open(Panel::Brush));

    tap(&mut h, "Undo");
    settle(&mut h);
    assert!(!h.state().globe.can_undo());

    // A panel closes with the menu of its tab and with the button that
    // opened it.
    close_panel(&mut h, "Layers");
    assert!(!h.state().pad.is_open(Panel::Layers));
    tap(&mut h, "Brush settings");
    assert!(!h.state().pad.is_open(Panel::Brush));
    toolbox(&mut h);
    toolbox(&mut h);
    assert!(h.state().pad.dock().panes.is_empty());
}

/// A tap of `count` fingers near the middle of the canvas. The first finger
/// is also the pointer. All fingers come down in one frame and go up in the
/// next frame, so the tap is short.
fn tap_fingers(h: &mut Pad, count: u64) {
    let c = h.state().globe.rect.center();
    for phase in [egui::TouchPhase::Start, egui::TouchPhase::End] {
        let events = &mut h.input_mut().events;
        for id in 1..=count {
            let pos = c + vec2(60.0 * id as f32, 0.0);
            events.push(egui::Event::Touch {
                device_id: egui::TouchDeviceId(0),
                id: egui::TouchId(id),
                phase,
                pos,
                force: None,
            });
            if id == 1 {
                events.push(egui::Event::PointerMoved(pos));
                events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: phase == egui::TouchPhase::Start,
                    modifiers: egui::Modifiers::NONE,
                });
            }
        }
        if phase == egui::TouchPhase::End {
            events.push(egui::Event::PointerGone);
        }
        h.step();
    }
    h.run_steps(3);
}

/// Paints three pen strokes over the middle of the canvas. Returns the level
/// of the middle before the first stroke and after each stroke.
fn three_strokes(h: &mut Pad) -> [u16; 4] {
    let c = h.state().globe.rect.center();
    let mut levels = [middle_level(h); 4];
    for i in 1..4 {
        drag(h, c - vec2(40.0, 0.0), c + vec2(40.0, 0.0), Some(0.5));
        settle(h);
        levels[i] = middle_level(h);
        assert!(levels[i] > levels[i - 1], "{levels:?}");
    }
    levels
}

fn middle_level(h: &Pad) -> u16 {
    let globe = &h.state().globe;
    // The first raster layer of the world, or the empty heightmap of a
    // world with no raster layer.
    let first = h.state().doc.project.rasters().first();
    let map = first.and_then(|raster| globe.raster_map(raster.id));
    map.unwrap_or(&globe.map)
        .sample(globe.unproject(globe.rect.center()).unwrap())
}

#[test]
#[ignore = "gpu"]
fn a_new_world_has_an_empty_heightmap_on_the_gpu() {
    let _gpu = one_gpu_test();
    let mut h = gpu_pad(BOARDS[0]);
    let levels = three_strokes(&mut h);
    assert!(h.state().has_raster() && h.state().globe.can_undo());

    h.state_mut().run_action(Action::NewWorld(World::default()));
    h.run_steps(3);
    assert!(!h.state().has_raster() && !h.state().globe.can_undo());
    assert_eq!(middle_level(&h), levels[0]);
    // A stroke on a height layer of the new world starts from the base
    // level and not from the old ground.
    h.state_mut().add_height();
    h.run_steps(2);
    let c = h.state().globe.rect.center();
    drag(&mut h, c - vec2(40.0, 0.0), c + vec2(40.0, 0.0), Some(0.5));
    settle(&mut h);
    let level = middle_level(&h);
    assert!(
        levels[0] < level && level < levels[2],
        "{level} of {levels:?}"
    );
}

#[test]
#[ignore = "gpu"]
fn a_tap_of_two_fingers_is_undo_and_a_tap_of_three_fingers_is_redo() {
    let _gpu = one_gpu_test();
    let mut h = gpu_pad(BOARDS[0]);
    let levels = three_strokes(&mut h);
    let view = h.state().globe.view;
    for level in levels[..3].iter().rev() {
        tap_fingers(&mut h, 2);
        settle(&mut h);
        assert_eq!(middle_level(&h), *level, "undo");
    }
    assert!(!h.state().globe.can_undo());
    // One more tap does nothing.
    tap_fingers(&mut h, 2);
    assert_eq!(middle_level(&h), levels[0]);
    for level in &levels[1..] {
        tap_fingers(&mut h, 3);
        settle(&mut h);
        assert_eq!(middle_level(&h), *level, "redo");
    }
    assert!(!h.state().globe.can_redo());
    // A tap of one finger does nothing, and no tap moves the globe.
    tap_fingers(&mut h, 1);
    assert_eq!(middle_level(&h), levels[3]);
    assert_eq!(h.state().globe.view, view);
}

#[test]
#[ignore = "gpu"]
fn the_undo_button_and_the_redo_button_work() {
    let _gpu = one_gpu_test();
    let mut h = gpu_pad(BOARDS[0]);
    assert!(control(&h, "Undo").is_none() && control(&h, "Redo").is_none());
    let levels = three_strokes(&mut h);
    assert!(control(&h, "Redo").is_none());
    for level in levels[..3].iter().rev() {
        tap(&mut h, "Undo");
        settle(&mut h);
        assert_eq!(middle_level(&h), *level, "undo");
    }
    assert!(control(&h, "Undo").is_none());
    for level in &levels[1..] {
        tap(&mut h, "Redo");
        settle(&mut h);
        assert_eq!(middle_level(&h), *level, "redo");
    }
    assert!(control(&h, "Redo").is_none());
    // A new stroke removes the redo steps.
    tap(&mut h, "Undo");
    settle(&mut h);
    assert!(control(&h, "Redo").is_some());
    let c = h.state().globe.rect.center();
    drag(&mut h, c - vec2(40.0, 0.0), c + vec2(40.0, 0.0), Some(0.5));
    settle(&mut h);
    assert!(control(&h, "Redo").is_none());
}

/// A start of the app with a project file. The heightmap comes from the
/// file, if the file has one.
fn file_pad(path: &std::path::Path) -> Pad {
    let mut state = state();
    let session = Session::open(path, &mut state.doc).unwrap();
    state.globe.set_face_size(256);
    state.open_rasters(session.load_maps().unwrap());
    state.session = Some(session);
    // The pen paints the height layer.
    state.select_height();
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let size = (f64::from(BOARDS[0].0), f64::from(BOARDS[0].1));
    let mut h = headless::ui_harness(state, size, 1.0, setup);
    h.set_render_every_step(true);
    h.run_steps(3);
    h.state_mut().save_tick(false, std::time::Instant::now());
    h
}

/// Runs frames with the save of the app until the brush is idle.
fn settle_and_save(h: &mut Pad) {
    for _ in 0..400 {
        h.step();
        h.state_mut().save_tick(false, std::time::Instant::now());
        if !h.state().globe.busy() {
            return;
        }
    }
    panic!("the brush did not become idle");
}

/// The iPad test of the undo log: paint, stop the app with no exit call,
/// start it, and undo.
#[test]
#[ignore = "gpu"]
fn the_undo_button_works_after_the_app_stops_and_starts() {
    let _gpu = one_gpu_test();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut h = file_pad(&path);
    assert!(control(&h, "Undo").is_none());
    let c = h.state().globe.rect.center();
    let mut levels = [middle_level(&h); 3];
    for i in 1..3 {
        drag(&mut h, c - vec2(40.0, 0.0), c + vec2(40.0, 0.0), Some(0.5));
        settle_and_save(&mut h);
        levels[i] = middle_level(&h);
        assert!(levels[i] > levels[i - 1], "{levels:?}");
    }
    // The file has the steps, and the heightmap has none in memory.
    assert!(!h.state().globe.can_undo() && h.state().can_undo());
    drop(h);

    let mut h = file_pad(&path);
    assert_eq!(middle_level(&h), levels[2]);
    assert!(control(&h, "Redo").is_none());
    // Each stroke is one step.
    for level in levels[..2].iter().rev() {
        tap(&mut h, "Undo");
        settle_and_save(&mut h);
        assert_eq!(middle_level(&h), *level, "undo");
    }
    assert!(control(&h, "Undo").is_none());
    tap(&mut h, "Redo");
    settle_and_save(&mut h);
    assert_eq!(middle_level(&h), levels[1]);
    drop(h);

    let mut h = file_pad(&path);
    assert_eq!(middle_level(&h), levels[1]);
    tap(&mut h, "Redo");
    settle_and_save(&mut h);
    assert_eq!(middle_level(&h), levels[2]);
    assert!(control(&h, "Redo").is_none());
}

#[test]
fn the_view_popover_switches_the_view_and_the_tool_and_the_panels_stay() {
    let mut h = pad(BOARDS[0]);
    choose_mode(&mut h, "Lower");
    open_panel(&mut h, "Layers");
    tap(&mut h, "Brush settings");
    let panels = h.state().pad.panels.clone();
    assert_eq!(panels.len(), 2);
    for (name, view) in [("Flat", WorldView::Flat), ("Globe", WorldView::Globe)] {
        tap_in_view(&mut h, name);
        let s = h.state();
        assert_eq!(s.globe.world_view, view);
        assert_eq!(toggled(&h, name), Some(true));
        assert_eq!(s.workspace, Workspace::Globe);
        assert_eq!(
            (s.globe.tool, s.globe.brush.mode),
            (Tool::Brush, Mode::Lower)
        );
        assert_eq!(s.pad.panels, panels);
        // The popover stays open.
        assert!(s.pad.view_menu);
    }
}

#[test]
fn the_view_popover_opens_the_list_of_the_projections_of_the_flat_view() {
    use vale_sphere::ProjectionKind;
    let mut h = pad(BOARDS[0]);
    assert!(control(&h, PROJECTION).is_none());
    choose_mode(&mut h, "Lower");
    tap(&mut h, VIEW);
    assert!(control(&h, PROJECTION).is_none());
    tap(&mut h, "Flat");
    // The popover is above the card of the readout, at its left edge.
    let menu = h.state().pad.view_menu_rect.unwrap();
    let card = h.state().pad.view_rect.unwrap();
    assert_eq!(menu.left(), card.left());
    assert!(menu.bottom() < card.top());
    assert!(menu.contains_rect(control(&h, PROJECTION).unwrap()));

    tap(&mut h, PROJECTION);
    // On a wide screen, the list is at the right of the popover.
    let list = h.state().pad.projections_rect.unwrap();
    assert!(list.left() > menu.right());
    assert_eq!(list.bottom(), menu.bottom());
    for kind in ProjectionKind::ALL {
        assert!(list.contains_rect(control(&h, kind.name()).unwrap()));
    }
    check_controls(&h, "with the projections");
    tap(&mut h, "Equal Earth");
    let s = h.state();
    assert_eq!(s.globe.flat.spec().kind, ProjectionKind::EqualEarth);
    assert!(!s.pad.projections && s.pad.projections_rect.is_none());
    assert!(s.pad.view_menu);
    assert_eq!(
        (s.globe.tool, s.globe.brush.mode),
        (Tool::Brush, Mode::Lower)
    );

    // A touch off the list closes it, and the popover stays. The globe
    // has no list.
    tap(&mut h, PROJECTION);
    tap(&mut h, GRATICULE);
    assert!(!h.state().pad.projections && h.state().pad.view_menu);
    tap(&mut h, PROJECTION);
    tap(&mut h, "Globe");
    assert!(!h.state().pad.projections && control(&h, PROJECTION).is_none());
}

#[test]
fn the_view_popover_has_the_views_and_the_graticule_switch() {
    use vale_app::ui::pad::raster::{self, RIVERS};
    for size in BOARDS {
        let mut h = painted_pad(size);
        for view in WORLD_VIEWS {
            let what = format!("{size:?} {view:?}");
            h.state_mut().globe.world_view = view;
            h.run_steps(2);
            let rects = h.state().pad.rects.unwrap();
            let card = h.state().pad.view_rect.unwrap();
            assert!(rects.readout.unwrap().contains_rect(card), "{what}");
            // At rest, the card has the View button and no rows.
            assert!(card.contains_rect(control(&h, VIEW).unwrap()), "{what}");
            for name in view_rows() {
                assert!(control(&h, name).is_none(), "{what}: {name}");
            }
            tap(&mut h, VIEW);
            assert_eq!(toggled(&h, VIEW), Some(true), "{what}");
            let menu = h.state().pad.view_menu_rect.unwrap();
            assert!(rects.screen.contains_rect(menu), "{what}");
            // The rows of the flat view show in the flat view only.
            let flat = view == WorldView::Flat;
            for name in view_rows() {
                let shows = !matches!(name, PROJECTION | RECENTER | RESET) || flat;
                if let Some(row) = control(&h, name) {
                    assert!(menu.contains_rect(row), "{what}: {name}");
                } else if name != RECENTER && name != RESET {
                    // Recenter and Reset view are off until the view moves.
                    assert!(!shows, "{what}: no {name}");
                }
                if !shows {
                    assert!(control(&h, name).is_none(), "{what}: {name}");
                }
            }
            check_controls(&h, &format!("{what} with the View popover"));
            let before = h.state().globe.preview.graticule;
            tap(&mut h, GRATICULE);
            assert_eq!(h.state().globe.preview.graticule, !before, "{what}");
            assert_eq!(toggled(&h, GRATICULE), Some(!before), "{what}");
            tap(&mut h, GRATICULE);
            assert_eq!(h.state().globe.preview.graticule, before, "{what}");
            // The segments change the view.
            let other = if flat { "Globe" } else { "Flat" };
            tap(&mut h, other);
            assert_ne!(h.state().globe.world_view, view, "{what}");
            assert_eq!(toggled(&h, other), Some(true), "{what}");
            // A second tap on the View button closes the popover.
            tap(&mut h, VIEW);
            assert!(!h.state().pad.view_menu, "{what}");
            assert!(h.state().pad.view_menu_rect.is_none(), "{what}");
            h.state_mut().globe.world_view = view;
            h.run_steps(2);
        }

        // The panel of the height layer has the rivers and no graticule.
        open_height(&mut h);
        let mut rivers = control(&h, RIVERS).is_some();
        for _ in 0..8 {
            if rivers {
                break;
            }
            let card = panel(&h, Panel::Layers).unwrap();
            // A drag up on a row or on the gradient scrolls.
            let inside = |p: &Pos2| p.y > card.top() + 60.0 && p.y < card.bottom() - 10.0;
            let rows = [IMPORT_HEIGHTMAP, EXPORT_GEOTIFF, raster::COLOR_STOPS];
            let points: Vec<Pos2> = rows
                .into_iter()
                .filter_map(|name| control(&h, name))
                .map(|rect| rect.center())
                .collect();
            let from = points
                .into_iter()
                .filter(inside)
                .max_by(|a, b| a.y.total_cmp(&b.y))
                // The margin at the left of the rows scrolls too.
                .unwrap_or(pos2(card.left() + 6.0, card.bottom() - 60.0));
            let to = pos2(from.x, (from.y - 400.0).max(40.0));
            drag(&mut h, from, to, None);
            rivers = control(&h, RIVERS).is_some();
        }
        assert!(rivers, "{size:?}: the panel has no rivers");
    }
}

#[test]
fn the_flat_view_has_rows_that_move_the_center_of_the_projection_and_reset_it() {
    for size in BOARDS {
        let mut h = pad(size);
        h.state_mut().globe.world_view = WorldView::Flat;
        h.run_steps(2);
        let rects = h.state().pad.rects.unwrap();
        // The card of the readout is at the lower left of the canvas.
        let card = h.state().pad.view_rect.unwrap();
        assert_eq!(
            card.left_bottom(),
            rects.readout.unwrap().left_bottom(),
            "{size:?}"
        );
        assert!(!card.intersects(rects.tools), "{size:?}");
        // The middle of the view is the center of the projection, and the
        // view shows the whole map, so the two actions are off.
        tap(&mut h, VIEW);
        assert!(control(&h, PROJECTION).is_some(), "{size:?}");
        assert!(control(&h, RECENTER).is_none() && control(&h, RESET).is_none());

        // A drag moves the map at the first zoom, with no zoom before it.
        tap(&mut h, VIEW);
        let canvas = h.state().globe.rect;
        let from = canvas.center();
        drag(&mut h, from, from + vec2(-60.0, 0.0), None);
        tap(&mut h, VIEW);
        let menu = h.state().pad.view_menu_rect.unwrap();
        let (recenter, reset) = (control(&h, RECENTER).unwrap(), control(&h, RESET).unwrap());
        assert!(menu.contains_rect(recenter) && menu.contains_rect(reset));
        assert_eq!(recenter.top(), control(&h, PROJECTION).unwrap().bottom());
        assert_eq!(reset.top(), recenter.bottom());
        check_controls(&h, &format!("{size:?} with the center rows"));
        tap(&mut h, RECENTER);
        let flat = &h.state().globe.flat;
        assert!(flat.spec().lon0 > 1.0, "{size:?}: {:?}", flat.spec());
        assert!(control(&h, RECENTER).is_none());

        tap(&mut h, RESET);
        let flat = &h.state().globe.flat;
        assert_eq!((flat.spec().lon0, flat.zoom), (0.0, 1.0));
        assert!(control(&h, RESET).is_none());

        // The globe has no such rows.
        h.state_mut().globe.world_view = WorldView::Globe;
        h.run_steps(2);
        assert!(h.state().pad.view_menu, "{size:?}");
        assert!(control(&h, RECENTER).is_none() && control(&h, RESET).is_none());
        assert!(control(&h, PROJECTION).is_none());
    }
}

#[test]
fn the_readout_at_rest_has_the_view_button_and_the_scale_bar_and_no_place() {
    for size in BOARDS {
        for view in WORLD_VIEWS {
            let what = format!("{size:?} {view:?}");
            let mut h = pad(size);
            h.state_mut().globe.world_view = view;
            h.run_steps(2);
            let band = h.state().pad.rects.unwrap().readout.unwrap();
            let card = h.state().pad.view_rect.unwrap();
            // One row at the bottom of the band.
            assert_eq!(card.height(), 52.0, "{what}");
            assert_eq!(card.left_bottom(), band.left_bottom(), "{what}");
            let button = control(&h, VIEW).unwrap();
            assert_eq!(button.size(), vec2(44.0, 44.0), "{what}");
            assert!(card.contains_rect(button), "{what}");
            let scale = h.state().pad.readout_scale.clone();
            assert!(scale.is_some_and(|s| s.ends_with("km")), "{what}");
            assert_eq!(h.state().pad.place_rect, None, "{what}");

            // While a finger is down on the canvas, the place shows above
            // the card. A test has no clock, so the place goes when the
            // finger lifts.
            let at = h.state().globe.rect.center() + vec2(30.0, 0.0);
            touch(&mut h, egui::TouchPhase::Start, at, None);
            h.run_steps(2);
            let place = h.state().pad.place_rect.unwrap();
            assert_eq!(place.height(), 34.0, "{what}");
            assert_eq!(place.left(), card.left(), "{what}");
            assert!(
                place.bottom() < card.top() && place.top() >= band.top(),
                "{what}"
            );
            assert!(place.width() < band.width(), "{what}");
            touch(&mut h, egui::TouchPhase::End, at, None);
            h.run_steps(2);
            assert_eq!(h.state().pad.place_rect, None, "{what}");
        }
    }
}

#[test]
fn the_place_shows_while_the_measure_tool_has_a_result() {
    let mut h = pad(BOARDS[0]);
    h.state_mut().globe.tool = Tool::Measure;
    h.run_steps(2);
    assert_eq!(h.state().pad.place_rect, None);
    let measure = &mut h.state_mut().measure;
    measure.add([0.0, 0.0], true);
    measure.add([10.0, 0.0], true);
    h.run_steps(2);
    assert!(h.state().measure_readout().is_some());
    assert!(h.state().pad.place_rect.is_some());
}

#[test]
fn a_tap_outside_the_view_popover_closes_it() {
    for size in [BOARDS[0], BOARDS[3]] {
        let mut h = pad(size);
        tap(&mut h, VIEW);
        assert!(h.state().pad.view_menu, "{size:?}");
        // A tap on the canvas closes it.
        let at = h.state().globe.rect.center() + vec2(0.0, -200.0);
        finger_tap_at(&mut h, at);
        assert!(!h.state().pad.view_menu, "{size:?}");
        assert!(control(&h, GRATICULE).is_none(), "{size:?}");
        // A tap on another card closes it.
        tap(&mut h, VIEW);
        tap(&mut h, PANELS);
        assert!(!h.state().pad.view_menu, "{size:?}");
        h.state_mut().pad.panels_menu = false;
        h.run_steps(2);
        // The Atlas workspace has no popover.
        tap(&mut h, VIEW);
        h.state_mut().workspace = Workspace::Atlas;
        h.run_steps(3);
        assert!(!h.state().pad.view_menu, "{size:?}");
        assert!(h.state().pad.view_menu_rect.is_none(), "{size:?}");
        h.state_mut().workspace = Workspace::Globe;
        h.run_steps(3);
        assert!(control(&h, GRATICULE).is_none(), "{size:?}");
    }
}

/// There is no card at the top center: the World view switch and the page
/// card of the Atlas are gone from the top row.
#[test]
fn the_top_row_has_no_center_card() {
    for size in BOARDS {
        for atlas in [false, true] {
            let mut h = pad(size);
            if atlas {
                h.state_mut().workspace = Workspace::Atlas;
                h.run_steps(3);
            }
            let r = h.state().pad.rects.unwrap();
            let what = format!("{size:?} atlas {atlas}");
            let top = Rect::from_min_max(
                pos2(r.workspace.right() + 1.0, r.workspace.top()),
                pos2(r.actions.left() - 1.0, r.workspace.bottom()),
            );
            for c in &h.state().pad.controls {
                assert!(!top.intersects(c.rect), "{what}: {}", c.name);
            }
            use vale_app::ui::pad::atlas::{NEXT_PAGE, PREVIOUS_PAGE};
            for name in [VIEW, PREVIOUS_PAGE, NEXT_PAGE] {
                if let Some(rect) = control(&h, name) {
                    assert!(rect.top() > r.column_top, "{what}: {name}");
                }
            }
        }
    }
}

#[test]
fn the_top_row_and_the_panels_match_the_landscape_board() {
    let mut h = pad(BOARDS[0]);
    let r = h.state().pad.rects.unwrap();
    assert_eq!(r.class, WidthClass::Wide);
    assert_eq!(r.workspace.min, pos2(0.0, 28.0));
    assert_eq!(r.workspace.height(), 52.0);
    // The Worlds button and a line are at the left of the workspaces.
    assert_eq!(control(&h, WORLDS).unwrap().min, pos2(4.0, 32.0));
    assert_eq!(control(&h, "World").unwrap().min, pos2(57.0, 32.0));
    assert_eq!(control(&h, "Atlas").unwrap().min, pos2(101.0, 32.0));
    assert_eq!(r.workspace.width(), 149.0);
    assert!(control(&h, "Workspace").is_none());
    // The View button is at the lower left of the canvas.
    let view = h.state().pad.view_rect.unwrap();
    assert_eq!(view.min, pos2(72.0, 754.0));
    assert_eq!(control(&h, VIEW).unwrap().min, pos2(76.0, 758.0));
    assert_eq!(
        r.actions,
        Rect::from_min_size(pos2(985.0, 28.0), vec2(209.0, 52.0))
    );
    // Undo, Redo, a line, the Panels button, and the Dock button.
    assert_eq!(control(&h, PANELS).unwrap().min, pos2(1098.0, 32.0));
    for name in ["Layers", "Toolbox", select::ATTRIBUTES] {
        assert!(control(&h, name).is_none(), "{name}");
    }
    // The tool column: six tools, a divider, two sliders, and the Brush
    // button.
    assert_eq!(
        r.tools,
        Rect::from_min_size(pos2(0.0, 92.0), vec2(56.0, 661.0))
    );
    assert_eq!(r.brush, None);
    assert_eq!(
        control(&h, SCULPT).unwrap(),
        Rect::from_min_size(pos2(6.0, 98.0), vec2(44.0, 44.0))
    );
    assert_eq!(
        control(&h, TERRAIN).unwrap(),
        Rect::from_min_size(pos2(6.0, 142.0), vec2(44.0, 44.0))
    );
    assert_eq!(control(&h, "Brush settings").unwrap().min, pos2(6.0, 703.0));

    open_panel(&mut h, "Layers");
    tap(&mut h, "Brush settings");
    // The dock is on the right edge, below the top row.
    let layers = panel(&h, Panel::Layers).unwrap();
    assert_eq!((layers.min, layers.width()), (pos2(854.0, 92.0), 340.0));
    // The header, the import row, and the three layers of the sample.
    assert_eq!(layers.height(), 52.0 + 4.0 * 48.0 + 10.0);
    // The action button is at the right end of the header.
    assert_eq!(control(&h, ADD_LAYER).unwrap().min, pos2(1146.0, 96.0));
    // The Brush panel is a pane below a bar.
    let brush = panel(&h, Panel::Brush).unwrap();
    assert_eq!(
        brush.min,
        pos2(854.0, layers.bottom() + vale_app::ui::pad::panes::BAR)
    );
    assert_eq!(brush.width(), 340.0);
    // Toolbox opens below the Brush pane, which gets shorter.
    toolbox(&mut h);
    let layers = panel(&h, Panel::Layers).unwrap();
    let brush = panel(&h, Panel::Brush).unwrap();
    assert_eq!(brush.top(), layers.bottom() + vale_app::ui::pad::panes::BAR);
    let toolbox = panel(&h, Panel::Toolbox).unwrap();
    assert_eq!(
        toolbox.top(),
        panel(&h, Panel::Brush).unwrap().bottom() + vale_app::ui::pad::panes::BAR
    );
    assert!(panel(&h, Panel::Layers).is_some());
}

#[test]
fn the_top_row_and_the_panels_match_the_portrait_board() {
    let mut h = pad(BOARDS[1]);
    let r = h.state().pad.rects.unwrap();
    assert_eq!(r.class, WidthClass::Medium);
    // The workspace buttons are icons, as in the landscape board.
    assert_eq!(control(&h, WORLDS).unwrap().min, pos2(4.0, 32.0));
    assert_eq!(control(&h, "World").unwrap().min, pos2(57.0, 32.0));
    assert!(control(&h, "Workspace").is_none());
    assert_eq!(
        (r.workspace.min, r.workspace.height()),
        (pos2(0.0, 28.0), 52.0)
    );
    assert!(control(&h, VIEW).is_some());
    assert_eq!(r.actions.right(), 834.0);

    toolbox(&mut h);
    let toolbox = panel(&h, Panel::Toolbox).unwrap();
    assert_eq!((toolbox.min, toolbox.width()), (pos2(494.0, 92.0), 340.0));
    tap(&mut h, "Brush settings");
    assert!(h.state().pad.is_open(Panel::Toolbox) && h.state().pad.is_open(Panel::Brush));

    // The workspace buttons change the workspace.
    tap(&mut h, "Atlas");
    assert_eq!(h.state().workspace, Workspace::Atlas);
    tap(&mut h, "World");
    assert_eq!(h.state().workspace, Workspace::Globe);
}

#[test]
fn the_top_row_and_the_panels_match_the_split_view_boards() {
    for size in [BOARDS[2], BOARDS[3]] {
        let mut h = pad(size);
        let r = h.state().pad.rects.unwrap();
        assert_eq!(r.class, WidthClass::Compact);
        assert_eq!(r.workspace.min, pos2(0.0, 28.0));
        assert_eq!(r.actions.right(), size.0);
        assert!(r.workspace.right() < r.actions.left());
        // The readout is above the card of the tool, and the card is above
        // the row of the strip. The card of the View button is at the
        // bottom of the band of the readout.
        let readout = r.readout.unwrap();
        assert_eq!(readout.left_bottom(), pos2(16.0, 670.0));
        assert_eq!(readout.top(), 670.0 - 52.0 - 40.0);
        let view = h.state().pad.view_rect.unwrap();
        assert_eq!(view.left_bottom(), readout.left_bottom());
        assert_eq!(r.brush.unwrap().bottom(), r.tools.top() - 8.0);
        assert_eq!(r.tools.bottom(), 834.0 - 20.0 - 8.0);

        // The menu holds the workspaces and the Worlds row.
        tap(&mut h, "Workspace");
        let menu = h.state().pad.menu_rect.unwrap();
        assert_eq!(
            menu,
            Rect::from_min_size(pos2(16.0, 84.0), vec2(240.0, 190.0))
        );
        let tops = ["World", "Atlas", WORLDS_ROW].map(|name| control(&h, name).unwrap().top());
        assert_eq!(tops, [94.0, 155.0, 216.0]);
        assert!(control(&h, "Toolbox").is_none());
        h.state_mut().pad.menu = false;
        h.run_steps(2);
        // The View popover has the views and the list of the projections.
        // The list has no room at the right of the popover, so it takes
        // its place.
        tap_in_view(&mut h, "Flat");
        assert_eq!(h.state().globe.world_view, WorldView::Flat);
        tap(&mut h, PROJECTION);
        let list = h.state().pad.projections_rect.unwrap();
        assert!(Rect::from_min_size(Pos2::ZERO, size.into()).contains_rect(list));
        assert_eq!(list.left(), readout.left());
        assert!(h.state().pad.view_menu_rect.is_none());
        check_controls(&h, &format!("{size:?} with the projections"));
        tap(&mut h, "Mercator");
        let kind = h.state().globe.flat.spec().kind;
        assert_eq!(kind, vale_sphere::ProjectionKind::Mercator);
        tap(&mut h, "Globe");
        assert_eq!(h.state().globe.world_view, WorldView::Globe);

        // A panel is a sheet at the bottom, and one sheet shows at a time.
        open_panel(&mut h, "Layers");
        let sheet = Rect::from_min_size(pos2(0.0, 382.0), vec2(size.0, 452.0));
        assert_eq!(panel(&h, Panel::Layers), Some(sheet));
        assert_eq!(h.state().pad.rects.unwrap().readout, None);
        toolbox(&mut h);
        assert_eq!(h.state().pad.panels, vec![(Panel::Toolbox, sheet)]);
    }
}

#[test]
fn you_pull_the_sheet_down_to_its_title_row() {
    let mut h = painted_pad(BOARDS[3]);
    open_panel(&mut h, "Layers");
    assert!(control(&h, "Open Height").is_some());
    let bar = pos2(150.0, 408.0);
    drag(&mut h, bar, bar + vec2(0.0, 250.0), None);
    let sheet = panel(&h, Panel::Layers).unwrap();
    assert_eq!(sheet.top(), 834.0 - 20.0 - 52.0);
    // The title row shows, and its close button is above the bottom band.
    assert!(control(&h, "Open Height").is_none());
    assert_eq!(
        control(&h, "Close Layers").unwrap().bottom(),
        834.0 - 20.0 - 4.0
    );
    // The Brush card has room again.
    assert!(control(&h, "Brush settings").is_some());

    // A short pull does not close the sheet, and a pull up opens it.
    let bar = pos2(150.0, 790.0);
    drag(&mut h, bar, bar - vec2(0.0, 300.0), None);
    assert_eq!(panel(&h, Panel::Layers).unwrap().top(), 382.0);
    let bar = pos2(150.0, 408.0);
    drag(&mut h, bar, bar + vec2(0.0, 60.0), None);
    assert_eq!(panel(&h, Panel::Layers).unwrap().top(), 382.0);
    // A pull up gives the sheet the full height, below the top row.
    drag(&mut h, bar, bar - vec2(0.0, 300.0), None);
    assert_eq!(panel(&h, Panel::Layers).unwrap().top(), 92.0);
    let bar = pos2(150.0, 118.0);
    drag(&mut h, bar, bar + vec2(0.0, 200.0), None);
    assert_eq!(panel(&h, Panel::Layers).unwrap().top(), 382.0);

    tap(&mut h, "Close Layers");
    assert!(h.state().pad.panels.is_empty());
}

#[test]
fn the_layout_follows_the_turn_of_the_ipad() {
    let mut h = pad(BOARDS[0]);
    open_panel(&mut h, "Layers");
    tap(&mut h, "Brush settings");
    for size in [BOARDS[1], BOARDS[3], BOARDS[0]] {
        h.set_size(size.into());
        h.run_steps(3);
        let screen = Rect::from_min_size(Pos2::ZERO, size.into());
        assert_eq!(h.state().globe.rect, screen);
        check_controls(&h, &format!("turned to {size:?}"));
    }
    assert!(h.state().pad.is_open(Panel::Brush));
    assert!(h.state().pad.is_open(Panel::Layers));
}

#[test]
fn a_short_window_makes_the_sliders_short_and_then_leaves_them_out() {
    let column = |h: &Pad| h.state().pad.rects.unwrap();
    // The sliders have their full length with room.
    let h = pad((1024.0, 784.0));
    assert_eq!(column(&h).brush_card, BrushCard::Full(120.0));
    check_controls(&h, "784 high");

    // At 744 points high, the sliders are shorter, and they work.
    let mut h = pad((1133.0, 744.0));
    let r = column(&h);
    assert_eq!(r.brush_card, BrushCard::Full(101.5));
    assert_eq!(r.tools.height(), r.column_content);
    let slider = control(&h, "Size").unwrap();
    assert!(slider.height() > slider.width());
    assert!(r.tools.contains_rect(slider));
    let size = h.state().globe.brush.size_points;
    drag(&mut h, slider.center(), slider.center_bottom(), None);
    assert!(h.state().globe.brush.size_points < size);
    slide_to_end(&mut h, "Flow", false);
    let flow = h.state().globe.brush.flow;
    slide_to_end(&mut h, "Flow", true);
    assert!(h.state().globe.brush.flow > flow);
    check_controls(&h, "744 high");

    // With less room, the Brush panel holds the two sliders.
    let mut h = pad((1024.0, 600.0));
    assert_eq!(column(&h).brush_card, BrushCard::Button);
    assert!(control(&h, "Size").is_none() && control(&h, "Flow").is_none());
    check_controls(&h, "600 high");
    tap(&mut h, "Brush settings");
    assert!(control(&h, "Brush size").is_some() && control(&h, "Brush flow").is_some());
    check_controls(&h, "600 high with the Brush panel");

    // Then the column scrolls.
    let h = pad((1024.0, 360.0));
    let r = column(&h);
    assert!(r.column_content > r.tools.height());
    assert_eq!(r.tools.bottom(), 360.0 - 20.0 - 8.0);
    check_controls(&h, "360 high");
}

#[test]
fn the_cards_stay_out_of_the_safe_area_of_the_device() {
    let mut h = pad(BOARDS[0]);
    let insets = egui::epaint::MarginF32 {
        left: 0.0,
        right: 0.0,
        top: 40.0,
        bottom: 34.0,
    };
    h.input_mut().safe_area_insets = Some(egui::SafeAreaInsets(insets));
    h.run_steps(3);
    let screen = Rect::from_min_size(Pos2::ZERO, BOARDS[0].into());
    assert_eq!(h.state().globe.rect, screen);
    for c in &h.state().pad.controls {
        assert!(
            c.rect.top() >= 40.0 && c.rect.bottom() <= 834.0 - 34.0,
            "{}",
            c.name
        );
    }
}

/// The size of an iPhone 16 in portrait and in landscape, and its safe area:
/// the bands at the top, the right, the bottom, and the left.
const IPHONES: [((f64, f64), [f32; 4]); 2] = [
    ((393.0, 852.0), [59.0, 0.0, 34.0, 0.0]),
    ((852.0, 393.0), [0.0, 59.0, 21.0, 59.0]),
];

#[test]
#[ignore = "gpu"]
fn the_screenshot_of_an_iphone_keeps_the_controls_in_the_safe_area() {
    let _gpu = one_gpu_test();
    std::fs::create_dir_all("../../target/app").unwrap();
    for ((size, [top, right, bottom, left]), name) in
        IPHONES.into_iter().zip(["portrait", "landscape"])
    {
        for panel in [None, Some(Panel::Layers)] {
            let mut state = state();
            state.globe.set_face_size(64);
            state.add_height();
            if let Some(panel) = panel {
                state.pad.open(panel);
            }
            let insets = egui::epaint::MarginF32 {
                left,
                right,
                top,
                bottom,
            };
            let safe = Some(egui::SafeAreaInsets(insets));
            let (image, state) = headless::ui_png_safe(state, size, 1.0, safe, |_| Ok(())).unwrap();
            let what = format!("{name}, {panel:?}");
            assert_eq!(image.dimensions(), (size.0 as u32, size.1 as u32), "{what}");
            let file = match panel {
                None => format!("../../target/app/test-pad-iphone-{name}.png"),
                Some(_) => format!("../../target/app/test-pad-iphone-{name}-layers.png"),
            };
            image.save(file).unwrap();
            let (width, height) = (size.0 as f32, size.1 as f32);
            let controls = &state.pad.controls;
            assert!(controls.len() >= 7, "{what}: {} controls", controls.len());
            for c in controls {
                let r = c.rect;
                assert!(
                    r.top() >= top
                        && r.bottom() <= height - bottom
                        && r.left() >= left
                        && r.right() <= width - right,
                    "{what}: {} is {r:?}",
                    c.name
                );
            }
        }
    }
}

/// The screens of the iPhone mockups in portrait, and the change from the
/// drawing screen that each one has.
type PhoneScreen = (&'static str, fn(&mut AppState));

const PHONE_SCREENS: [PhoneScreen; 5] = [
    ("PhoneDraw", |_| {}),
    ("PhoneDrawMenu", |s| s.pad.menu = true),
    ("PhoneDrawLayers", |s| s.pad.open(Panel::Layers)),
    ("PhoneDrawBrush", |s| s.pad.open(Panel::Brush)),
    ("PhoneDrawStyle", |s| {
        let layers = s.doc.project.layers();
        let rivers = layers.iter().find(|l| l.name == "Rivers").unwrap().id;
        s.select(Some(LayerRef::Vector(rivers)));
        s.globe.tool = Tool::Line;
        s.pad.open(Panel::Layers);
        s.pad.layer = Some(LayerRef::Vector(rivers));
        // The style is at the full height.
        s.pad.sheet_offset = -1000.0;
    }),
];

#[test]
#[ignore = "gpu"]
fn the_five_iphone_screens_render_at_393_by_852() {
    let _gpu = one_gpu_test();
    std::fs::create_dir_all("../../target/app").unwrap();
    let (size, [top, right, bottom, left]) = IPHONES[0];
    for (name, change) in PHONE_SCREENS {
        let mut state = state();
        state.globe.set_face_size(64);
        state.add_height();
        state.globe.tool = Tool::Brush;
        state.globe.brush.mode = Mode::Raise;
        let insets = egui::epaint::MarginF32 {
            left,
            right,
            top,
            bottom,
        };
        let safe = Some(egui::SafeAreaInsets(insets));
        let prepare = |s: &mut AppState| {
            change(s);
            Ok(())
        };
        let (image, state) = headless::ui_png_safe(state, size, 1.0, safe, prepare).unwrap();
        assert_eq!(image.dimensions(), (393, 852), "{name}");
        image
            .save(format!("../../target/app/test-{name}.png"))
            .unwrap();
        // An open sheet goes to the bottom of the screen, below the safe
        // area.
        if state.pad.rects.unwrap().sheet.is_some() {
            let x = 196;
            let above = image.get_pixel(x, 852 - bottom as u32 - 6);
            let below = image.get_pixel(x, 851);
            assert_eq!(
                above, below,
                "{name}: the sheet stops above the bottom band"
            );
        }
        let (width, height) = (size.0 as f32, size.1 as f32);
        for c in &state.pad.controls {
            let r = c.rect;
            let inside = r.top() >= top
                && r.bottom() <= height - bottom
                && r.left() >= left
                && r.right() <= width - right;
            assert!(inside, "{name}: {} is {r:?}", c.name);
        }
    }
}

/// The phone with the screen of a mockup of the atlas, the attribute
/// table, the form of a new point, or the start screen.
fn phone_screen(name: &str, dir: &std::path::Path) -> Pad {
    let ((width, height), [top, right, bottom, left]) = IPHONES[0];
    let mut state = state();
    state.globe.set_face_size(64);
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let mut h = headless::ui_harness(state, (width, height), 1.0, setup);
    let insets = egui::epaint::MarginF32 {
        left,
        right,
        top,
        bottom,
    };
    h.input_mut().safe_area_insets = Some(egui::SafeAreaInsets(insets));
    h.run_steps(3);
    h.state_mut().add_height();
    h.run_steps(2);
    match name {
        "PhoneAtlas" | "PhoneAtlasFrame" => {
            let state = h.state_mut();
            state.workspace = Workspace::Atlas;
            vale_app::ui::atlas::add_frame(state).unwrap();
            if name == "PhoneAtlas" {
                vale_app::ui::atlas::clear_selection(state);
            }
        }
        "PhoneData" => {
            let places = vector(&h, "Places");
            h.state_mut().open_table(places);
        }
        "PhonePoint" => {
            let places = vector(&h, "Places");
            h.state_mut().select(Some(LayerRef::Vector(places)));
            h.run_steps(2);
            quick_tap(&mut h, VECTOR);
            h.state_mut().globe.snap.on = false;
            open_panel(&mut h, select::ATTRIBUTES);
            h.state_mut().pad.sheet_offset = h.state().pad.rects.unwrap().sheet_travel;
            h.run_steps(2);
            let screen = h.state().pad.rects.unwrap().screen;
            pen_tap_at(&mut h, screen.center());
            let fields = h.state().doc.project.layer(places).unwrap().field_names();
            tap(&mut h, &fields[0]);
        }
        "PhoneStart" => {
            for world in ["Kareth", "Isles of Morrow", "The Sundering", "Tidewater"] {
                let path = vale_app::session::new_project_path(dir, world);
                let world = World {
                    name: world.to_string(),
                    ..World::default()
                };
                let project = vale_store::Project::new(world);
                vale_store::ProjectFile::create(&path, &project).unwrap();
            }
            let state = h.state_mut();
            state.projects = Locations::single(dir);
            state.show_projects();
        }
        _ => unreachable!("{name}"),
    }
    h.run_steps(3);
    h
}

#[test]
#[ignore = "gpu"]
fn the_atlas_the_table_the_point_card_and_the_start_screen_render_at_393_by_852() {
    let _gpu = one_gpu_test();
    std::fs::create_dir_all("../../target/app").unwrap();
    let (_, [top, right, bottom, left]) = IPHONES[0];
    for name in [
        "PhoneAtlas",
        "PhoneAtlasFrame",
        "PhoneData",
        "PhonePoint",
        "PhoneStart",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut h = phone_screen(name, dir.path());
        // The globe goes to the GPU in parts. Each render sends one part.
        let mut image = h.render().unwrap();
        for _ in 0..10_000 {
            let globe = &h.state().globe;
            if !globe.busy() && !globe.loading() {
                break;
            }
            h.step();
            image = h.render().unwrap();
        }
        assert_eq!(image.dimensions(), (393, 852), "{name}");
        image
            .save(format!("../../target/app/test-{name}.png"))
            .unwrap();
        if name == "PhoneStart" {
            continue;
        }
        for c in &h.state().pad.controls {
            let r = c.rect;
            let inside = r.top() >= top
                && r.bottom() <= 852.0 - bottom
                && r.left() >= left
                && r.right() <= 393.0 - right;
            assert!(inside, "{name}: {} is {r:?}", c.name);
        }
    }
}

#[test]
fn the_readout_shows_the_place_and_the_elevation() {
    assert_eq!(
        readout_text(Some([24.6, 31.2]), Some("640 m")),
        "24.6°E 31.2°N · 640 m"
    );
    assert_eq!(
        readout_text(Some([-70.04, -8.0]), Some("-120 m")),
        "70.0°W 8.0°S · -120 m"
    );
    assert_eq!(readout_text(None, None), "–");
}

#[test]
#[ignore = "gpu"]
fn the_screenshots_show_the_export_row() {
    let _gpu = one_gpu_test();
    std::fs::create_dir_all("../../target/app").unwrap();
    let dir = tempfile::tempdir().unwrap();
    for (name, size) in [("landscape", (1194.0, 834.0)), ("third", (375.0, 834.0))] {
        let mut state = state();
        state.globe.set_face_size(64);
        state.pad.open(Panel::Layers);
        let height = state.add_height();
        state.pad.layer = Some(LayerRef::Raster(height));
        let paint = |state: &mut AppState| {
            state.globe.map.set(0, 0, 0, meters_to_level(100.0));
            let path = dir.path().join("Terra.tif");
            state.run_action(Action::ExportHeightmap(path));
            Ok(())
        };
        let (image, state) = headless::ui_png_with(state, size, 1.0, paint).unwrap();
        image
            .save(format!("../../target/app/test-pad-export-{name}.png"))
            .unwrap();
        let note = state.export_note.unwrap();
        assert!(note.text.starts_with("Exported Terra.tif"), "{}", note.text);
        let rows = &state.pad.controls;
        assert!(rows.iter().any(|c| c.name == EXPORT_GEOTIFF), "{name}");
    }
}

#[test]
#[ignore = "gpu"]
fn the_screenshots_show_the_boards() {
    let _gpu = one_gpu_test();
    std::fs::create_dir_all("../../target/app").unwrap();
    // The name, the size, the open panels, and the state of the menu. The
    // shot "more" has the menu of the More cell in the Weather mode.
    type Shot = (&'static str, (f64, f64), &'static [Panel], bool);
    let shots: [Shot; 6] = [
        ("landscape", (1194.0, 834.0), &[], false),
        ("more", (1194.0, 834.0), &[Panel::Brush], false),
        (
            "open",
            (1194.0, 834.0),
            &[Panel::Brush, Panel::Layers],
            false,
        ),
        ("portrait", (834.0, 1194.0), &[Panel::Toolbox], false),
        ("half", (597.0, 834.0), &[], true),
        ("third", (375.0, 834.0), &[Panel::Layers], false),
    ];
    for (name, size, panels, menu) in shots {
        let mut state = state();
        state.add_height();
        for panel in panels {
            state.pad.docks.world.open_new(*panel);
        }
        state.pad.menu = menu;
        if name == "more" {
            state.pad.flyouts.open = Some(Flyout::Brush(ModeGroup::Terrain));
            state.globe.tool = Tool::Brush;
            state.globe.brush.mode = Mode::Weather;
        }
        let (image, state) = headless::ui_png(state, size, 1.0).unwrap();
        image
            .save(format!("../../target/app/test-pad-{name}.png"))
            .unwrap();
        // The canvas is behind the cards, and a card is not as dark as the canvas.
        let r = state.pad.rects.unwrap();
        let card = image.get_pixel(r.tools.center().x as u32, r.tools.bottom() as u32 - 3);
        // The dock is on the right edge, above the bottom band.
        let y = if r.sheet.is_some() {
            300
        } else {
            size.1 as u32 - 3
        };
        let canvas = image.get_pixel(size.0 as u32 - 3, y);
        assert_eq!(canvas.0, [15, 17, 19, 255], "{name}");
        assert_ne!(card.0, canvas.0, "{name}");
        assert_eq!(state.pad.flyouts.rect.is_some(), name == "more", "{name}");
    }
    // The menu of the new layers, and the panel of a vector layer.
    for (name, size) in [
        ("add", (1194.0, 834.0)),
        ("add-third", (375.0, 834.0)),
        ("layer", (1194.0, 834.0)),
    ] {
        let mut state = state();
        state.pad.open(Panel::Layers);
        if name == "layer" {
            let layers = state.doc.project.layers();
            let rivers = layers.iter().find(|l| l.name == "Rivers").unwrap().id;
            state.pad.layer = Some(LayerRef::Vector(rivers));
        } else {
            state.pad.add = true;
        }
        let (image, state) = headless::ui_png(state, size, 1.0).unwrap();
        image
            .save(format!("../../target/app/test-pad-{name}.png"))
            .unwrap();
        assert_eq!(state.pad.add_rect.is_some(), name != "layer", "{name}");
    }
    // The page of a new field, and the page of a field.
    for (name, page) in [("new-field", Page::NewField), ("field", Page::Field(0))] {
        let mut state = state();
        state.pad.open(Panel::Layers);
        let layers = state.doc.project.layers();
        let rivers = layers.iter().find(|l| l.name == "Rivers").unwrap().id;
        state.pad.layer = Some(LayerRef::Vector(rivers));
        state.pad.page = Some(page);
        let (image, state) = headless::ui_png(state, (1194.0, 834.0), 1.0).unwrap();
        image
            .save(format!("../../target/app/test-pad-{name}.png"))
            .unwrap();
        assert_eq!(state.pad.page, Some(page), "{name}");
    }
}

/// Types text into the field that has the focus, in place of its text.
fn type_text(h: &mut Pad, name: &str, text: &str) {
    tap(h, name);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.event(egui::Event::Text(text.to_owned()));
    h.run_steps(2);
}

/// Opens the menu of the worlds.
fn open_worlds(h: &mut Pad) {
    if h.state().pad.rects.unwrap().class == WidthClass::Compact {
        tap(h, "Workspace");
        tap(h, WORLDS_ROW);
    } else {
        tap(h, WORLDS);
    }
    assert!(h.state().pad.worlds.is_some() && !h.state().pad.menu);
}

#[test]
fn the_menu_of_the_worlds_opens_another_world() {
    for size in BOARDS {
        let dir = tempfile::tempdir().unwrap();
        let earth = dir.path().join("Earth.gpkg");
        let mut h = painted_pad(size);
        let state = h.state_mut();
        state.projects = Locations::single(dir.path());
        state.session = Some(Session::open(&earth, &mut state.doc).unwrap());
        let layers = state.doc.project.layers().len();
        let world = World {
            name: "Pebble".to_string(),
            radius_km: 200.0,
            units: Units::Metric,
            ..World::default()
        };
        state.run_action(Action::NewWorld(world));
        assert!(!state.has_raster());
        h.run_steps(2);

        // The menu lists the two worlds, and the open world is not a new row.
        open_worlds(&mut h);
        let menu = h.state().pad.worlds_rect.unwrap();
        let screen = h.state().pad.rects.unwrap().screen;
        assert!(
            screen.shrink(16.0).contains_rect(menu),
            "{size:?}: {menu:?}"
        );
        let rows = ["Earth (sample)", "Pebble", NEW_WORLD, WORLD_PROPERTIES];
        let tops = rows.map(|name| {
            let row = control(&h, name).unwrap_or_else(|| panic!("{size:?}: no {name}"));
            assert!(menu.contains_rect(row) && row.height() >= 44.0, "{name}");
            row.top()
        });
        assert!(tops.is_sorted(), "{size:?}: {tops:?}");

        tap(&mut h, "Earth (sample)");
        assert!(h.state().pad.worlds.is_none());
        let actions = std::mem::take(&mut h.state_mut().actions);
        assert_eq!(actions, [Action::OpenWorld(earth.clone())]);
        h.state_mut().run_action(actions[0].clone());
        h.run_steps(2);
        let state = h.state();
        assert_eq!(state.doc.project.world.name, "Earth (sample)");
        assert_eq!(state.doc.project.layers().len(), layers);
        assert!(state.has_raster(), "{size:?}");
        assert_eq!(state.session.as_ref().unwrap().path(), earth);

        // A touch off the menu closes it.
        open_worlds(&mut h);
        let off = h.state().globe.rect.right_bottom() - vec2(30.0, 120.0);
        touch(&mut h, egui::TouchPhase::Start, off, None);
        h.step();
        touch(&mut h, egui::TouchPhase::End, off, None);
        h.run_steps(2);
        assert!(h.state().pad.worlds.is_none(), "{size:?}");
    }
}

#[test]
fn the_world_form_makes_a_new_world_with_a_small_radius() {
    for size in BOARDS {
        let mut h = painted_pad(size);
        let earth = brush_size_text(h.state());
        open_worlds(&mut h);
        tap(&mut h, WORLD_PROPERTIES);
        assert!(h.state().pad.worlds.is_none());
        let form = h.state().world_form.clone().unwrap();
        assert_eq!(form.mode, FormMode::Properties);
        assert_eq!(
            (form.name.as_str(), form.radius.as_str()),
            ("Earth (sample)", "6371")
        );
        check_form(&h, &format!("{size:?} properties"));

        tap(&mut h, "Close World");
        assert!(h.state().world_form.is_none());
        open_worlds(&mut h);
        tap(&mut h, NEW_WORLD);
        assert_eq!(h.state().world_form.as_ref().unwrap().mode, FormMode::New);
        check_form(&h, &format!("{size:?} new"));
        drag_radius(&mut h, 1737.4);
        assert_eq!(h.state().world_form.as_ref().unwrap().radius, "1737.4");
        type_text(&mut h, NAME, "Pebble");
        type_text(&mut h, "Radius (km)", "200");
        let form = h.state().world_form.as_ref().unwrap();
        assert_eq!(form.radius_km(), Some(200.0), "{size:?}: {:?}", form.radius);
        assert_eq!(form.preset(), None);

        tap(&mut h, CREATE);
        assert!(h.state().world_form.is_none());
        let world = World {
            name: "Pebble".to_string(),
            radius_km: 200.0,
            units: Units::Metric,
            ..World::default()
        };
        let actions = std::mem::take(&mut h.state_mut().actions);
        assert_eq!(actions, [Action::NewWorld(world.clone())]);
        h.state_mut().run_action(actions[0].clone());
        h.run_steps(2);

        let state = h.state();
        assert_eq!(state.doc.project.world, world);
        assert!(state.doc.project.layers().is_empty());
        assert!(state.doc.project.rasters().is_empty());
        assert!(!state.has_raster() && !state.globe.can_undo());
        // The brush has the same size on the screen, and less ground under it.
        let km = state.globe.brush.radius_km(state.globe.radius(), 200.0);
        assert_eq!(
            brush_size_text(state),
            format!("{} km", three_significant(km))
        );
        assert_ne!(brush_size_text(state), earth);
        assert_eq!(state.globe.world_km, 200.0);

        // The menu of the worlds also opens the form of a new world, and
        // the first form of an app with no project does not close.
        open_worlds(&mut h);
        tap(&mut h, NEW_WORLD);
        let form = h.state_mut().world_form.as_mut().unwrap();
        assert_eq!(form.mode, FormMode::New);
        form.required = true;
        h.run_steps(2);
        tap(&mut h, "Close New world");
        assert!(h.state().world_form.is_some(), "{size:?}");
        tap(&mut h, CREATE);
        assert_eq!(h.state().actions.len(), 1, "{size:?}");
    }
}

/// A drag on the radius slider of the open form, from the place of the
/// radius of the form to the place of this radius.
fn drag_radius(h: &mut Pad, km: f64) {
    let slider = control(h, RADIUS_SLIDER).unwrap();
    let (left, width) = (
        slider.left() + TRACK_INSET,
        slider.width() - 2.0 * TRACK_INSET,
    );
    let at = |km: f64| pos2(left + radius_fraction(km) as f32 * width, slider.center().y);
    let form = h.state().world_form.as_ref().unwrap();
    let from = form.radius_km().unwrap_or(6371.0);
    drag(h, at(from), at(km), None);
}

#[test]
fn the_radius_slider_snaps_to_the_known_radii() {
    for size in BOARDS {
        let mut h = pad(size);
        h.state_mut().world_form = Some(WorldForm::new_world());
        h.run_steps(2);
        check_form(&h, &format!("{size:?} slider"));
        let slider = control(&h, RADIUS_SLIDER).unwrap();
        let (left, width) = (
            slider.left() + TRACK_INSET,
            slider.width() - 2.0 * TRACK_INSET,
        );
        let at = |km: f64| pos2(left + radius_fraction(km) as f32 * width, slider.center().y);
        let radius = |h: &Pad| h.state().world_form.as_ref().unwrap().radius_km().unwrap();

        // A drag to a place near Mars gives the radius of Mars.
        drag(&mut h, at(6371.0), at(3389.5) + vec2(3.0, 0.0), None);
        assert_eq!(radius(&h), 3389.5, "{size:?}");
        let form = h.state().world_form.as_ref().unwrap();
        assert_eq!((form.radius.as_str(), form.preset()), ("3389.5", Some(1)));
        // A place off the presets gives a round value.
        drag(&mut h, at(3389.5), at(12_000.0), None);
        let km = radius(&h);
        assert!(10_000.0 < km && km < 14_000.0, "{size:?}: {km}");
        assert_eq!(three_significant(km).parse::<f64>().unwrap(), km);
        assert_eq!(h.state().world_form.as_ref().unwrap().preset(), None);
        // The end of the slider is its upper limit.
        drag(
            &mut h,
            at(km),
            pos2(slider.right(), slider.center().y),
            None,
        );
        assert_eq!(radius(&h), 20_000.0);
        // A touch with no drag does not move the radius.
        let (pos, phase) = (at(3389.5), egui::TouchPhase::Start);
        touch(&mut h, phase, pos, None);
        h.step();
        touch(&mut h, egui::TouchPhase::End, pos, None);
        h.run_steps(3);
        assert_eq!(radius(&h), 20_000.0);
        drag_radius(&mut h, 1737.4);
        assert_eq!(radius(&h), 1737.4);
        // A typed value can be outside the limits of the slider.
        type_text(&mut h, "Radius (km)", "250");
        assert_eq!(radius(&h), 250.0, "{size:?}");
    }
}

/// Makes sure that the open form holds each control of the frame, and that
/// a finger can hit each one.
fn check_form(h: &Pad, what: &str) {
    let screen = h.state().pad.rects.unwrap().screen;
    let controls = &h.state().pad.controls;
    assert!(controls.len() >= 6, "{what}: {} controls", controls.len());
    for (i, a) in controls.iter().enumerate() {
        let (name, rect) = (&a.name, a.rect);
        assert!(
            rect.width() >= 44.0 && rect.height() >= 44.0,
            "{what}: {name} is {rect:?}"
        );
        assert!(rect.top() >= BANDS.0, "{what}: {name}");
        assert!(rect.bottom() <= screen.bottom() - BANDS.1, "{what}: {name}");
        assert!(
            rect.left() >= 0.0 && rect.right() <= screen.right(),
            "{what}: {name}"
        );
        for b in &controls[i + 1..] {
            assert_ne!(a.name, b.name, "{what}");
            let handle =
                [&a.name, &b.name].contains(&&vale_app::ui::pad::dock_ui::DOCK_WIDTH.to_owned());
            let free = !a.rect.shrink(0.5).intersects(b.rect.shrink(0.5));
            assert!(free || handle, "{what}: {name} and {} overlap", b.name);
        }
    }
}

#[test]
fn the_properties_of_the_world_apply_at_once_and_a_touch_does_not_reach_the_canvas() {
    let mut h = pad(BOARDS[0]);
    h.state_mut().world_form = Some(WorldForm::properties(&World::default()));
    h.run_steps(2);
    let view = h.state().globe.view;
    let layers = h.state().doc.project.layers().to_vec();
    drag(&mut h, pos2(900.0, 600.0), pos2(700.0, 500.0), None);
    assert!(h.state().globe.view == view);

    drag_radius(&mut h, 3389.5);
    tap(&mut h, "Miles");
    let world = &h.state().doc.project.world;
    assert_eq!((world.radius_km, world.units), (3389.5, Units::Imperial));
    assert_eq!(h.state().world_form.as_ref().unwrap().radius, "2106.1");
    // The change of the units keeps the radius.
    tap(&mut h, "Kilometers");
    assert_eq!(h.state().doc.project.world.radius_km, 3389.5);
    // A radius that is too small does not go to the world.
    type_text(&mut h, "Radius (km)", "5");
    assert_eq!(h.state().doc.project.world.radius_km, 3389.5);
    assert!(h.query_by_label_contains("From 100 to 100000 km").is_some());
    type_text(&mut h, "Radius (km)", "500");
    assert_eq!(h.state().doc.project.world.radius_km, 500.0);
    assert_eq!(h.state().doc.project.layers(), layers);
    tap(&mut h, "Close World");
    assert!(h.state().world_form.is_none());
}

/// Opens the panel of the height layer, and returns its ID.
fn open_height(h: &mut Pad) -> RasterId {
    open_panel(h, "Layers");
    tap(h, "Open Height");
    h.state().doc.project.rasters()[0].id
}

#[test]
fn the_panel_of_a_raster_layer_fits_the_ipad_layout() {
    use vale_app::ui::pad::raster::{
        COLOR, COLOR_RAMPS, COLOR_STOPS, LINE, REMOVE_STOP, RESET_STOPS, SHOW_AS, STOP_VALUE, UNIT,
    };
    for size in BOARDS {
        let what = format!("{size:?}");
        let mut h = painted_pad(size);
        let height = open_height(&mut h);
        check_controls(&h, &format!("{what} in the panel of a raster layer"));
        let screen = Rect::from_min_size(Pos2::ZERO, size.into());
        let card = panel(&h, Panel::Layers).unwrap();
        assert!(screen.contains_rect(card), "{what}: {card:?}");
        assert!(
            control(&h, UNIT).is_none(),
            "{what}: a height layer has no unit row"
        );
        // The rows that are on the screen are inside the card.
        for name in [LAYER_NAME, OPACITY, SHOW_AS, COLOR_STOPS] {
            if let Some(rect) = control(&h, name) {
                assert!(card.contains_rect(rect), "{what}: {name} is {rect:?}");
            }
        }
        open_stops(&mut h);
        check_controls(&h, &format!("{what} on the page of the color stops"));
        let card = panel(&h, Panel::Layers).unwrap();
        assert!(screen.contains_rect(card), "{what}: {card:?}");
        for name in [GRADIENT, RESET_STOPS, COLOR_RAMPS] {
            if let Some(rect) = control(&h, name) {
                assert!(card.contains_rect(rect), "{what}: {name} is {rect:?}");
            }
        }
        show_stop(&mut h, 1);
        tap_handle(&mut h, 1);
        assert_eq!(
            h.state().globe.preview.selected,
            Some((height, 1)),
            "{what}"
        );
        check_controls(&h, &format!("{what} with a selected stop"));
        let shade = format!("{COLOR} shade");
        for name in [STOP_VALUE, &shade] {
            assert!(control(&h, name).is_some(), "{what}: no {name}");
        }
        // Above a sheet, the card has less room and scrolls.
        if h.state().pad.rects.unwrap().sheet.is_none() {
            assert!(
                control(&h, REMOVE_STOP).is_some(),
                "{what}: no {REMOVE_STOP}"
            );
        }
        let stop = h.state().pad.style.card.unwrap();
        assert!(screen.contains_rect(stop), "{what}: {stop:?}");
        if size.0 > size.1 {
            assert!(stop.right() < card.left(), "{what}: {stop:?}");
        }
        // The band above sea level has a line at the coast and no field.
        let sea = h.state().globe.map.bands.sea().unwrap();
        show_stop(&mut h, sea);
        tap_handle(&mut h, sea);
        assert_eq!(h.state().globe.preview.selected, Some((height, sea)));
        check_controls(&h, &format!("{what} with the stop at sea level"));
        for name in [LINE, &shade] {
            assert!(control(&h, name).is_some(), "{what}: no {name}");
        }
        for name in [STOP_VALUE, REMOVE_STOP] {
            assert!(control(&h, name).is_none(), "{what}: {name}");
        }
        assert_eq!(h.state().pad.layer, Some(LayerRef::Raster(height)));
    }
}

#[test]
fn the_style_section_has_a_strip_of_the_color_stops_and_no_rows_of_them() {
    use vale_app::ui::pad::raster::{COLOR_RAMPS, COLOR_STOPS, RESET_STOPS};
    for size in BOARDS {
        let what = format!("{size:?}");
        let mut h = painted_pad(size);
        open_height(&mut h);
        scroll_to_row(&mut h, COLOR_STOPS);
        check_controls(&h, &format!("{what} at the strip of the color stops"));
        // The one control of the name is the strip, with no drag area.
        assert_eq!(h.query_all_by_label(COLOR_STOPS).count(), 1, "{what}");
        let strip = control(&h, COLOR_STOPS).unwrap_or_else(|| panic!("{what}: no strip"));
        assert!(strip.height() < 2.0 * COLUMN.row, "{what}: {strip:?}");
        assert!(h.query_by_label(&band_row(0)).is_none(), "{what}");
        for name in [RESET_STOPS, COLOR_RAMPS] {
            assert!(h.query_by_label(name).is_none(), "{what}: {name}");
        }
    }
}

#[test]
fn a_tap_on_the_strip_opens_the_page_and_back_from_the_ramps_goes_to_it() {
    use vale_app::ui::pad::raster::{COLOR_RAMPS, COLOR_STOPS, RESET_STOPS};
    let mut h = painted_pad(BOARDS[1]);
    let height = open_height(&mut h);
    scroll_to_row(&mut h, COLOR_STOPS);
    tap(&mut h, COLOR_STOPS);
    assert_eq!(h.state().pad.page, Some(Page::Stops));
    let count = h.state().globe.map.bands.colors().len();
    for index in 0..count {
        assert!(h.query_by_label(&band_row(index)).is_some(), "{index}");
    }
    check_controls(&h, "on the page of the color stops");
    // The back button of the ramps goes back to the page of the stops.
    scroll_to_row(&mut h, COLOR_RAMPS);
    assert!(control(&h, RESET_STOPS).is_some());
    tap(&mut h, COLOR_RAMPS);
    assert_eq!(h.state().pad.page, Some(Page::Ramps));
    tap(&mut h, &format!("Back to {COLOR_STOPS}"));
    assert_eq!(h.state().pad.page, Some(Page::Stops));
    // The back button of the stops closes the page and clears the
    // selection.
    show_stop(&mut h, 1);
    let row = band_rect(&h, 1).center();
    finger_tap(&mut h, row);
    assert_eq!(h.state().globe.preview.selected, Some((height, 1)));
    tap(&mut h, "Back to Height");
    assert_eq!(h.state().pad.page, None);
    assert_eq!(h.state().pad.layer, Some(LayerRef::Raster(height)));
    assert_eq!(h.state().globe.preview.selected, None);
    assert!(h.query_by_label(RESET_STOPS).is_none());
    assert!(h.query_by_label(&band_row(0)).is_none());
}

#[test]
fn twenty_bands_have_rows_of_touch_size_with_the_highest_at_the_top() {
    let mut h = painted_pad(BOARDS[1]);
    open_height(&mut h);
    add_stops(&mut h, 21);
    open_stops(&mut h);
    check_controls(&h, "on the page of 20 bands");
    let rows: Vec<Rect> = (0..20).map(|i| band_rect(&h, i)).collect();
    for (index, row) in rows.iter().enumerate() {
        assert!(row.height() >= 44.0, "{index}: {row:?}");
    }
    for (index, pair) in rows.windows(2).enumerate() {
        assert!(pair[1].bottom() <= pair[0].top() + 0.5, "{index}: {pair:?}");
    }
}

/// The lines that the last frame painted, as lists of points.
fn painted_lines(h: &Pad) -> Vec<Vec<Pos2>> {
    fn visit(shape: &egui::Shape, lines: &mut Vec<Vec<Pos2>>) {
        match shape {
            egui::Shape::Path(path) => lines.push(path.points.clone()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| visit(s, lines)),
            _ => {}
        }
    }
    let mut lines = Vec::new();
    for clipped in &h.output().shapes {
        visit(&clipped.shape, &mut lines);
    }
    lines
}

#[test]
fn each_handle_is_at_its_value_and_its_line_goes_to_its_row_with_no_crossing() {
    let mut h = painted_pad(BOARDS[1]);
    open_height(&mut h);
    add_stops(&mut h, 20);
    open_stops(&mut h);
    show_stop(&mut h, 10);
    let card = panel(&h, Panel::Layers).unwrap();
    let parts = column_parts(&h);
    let bands = h.state().globe.map.bands.clone();
    let lines = painted_lines(&h);
    let mut ends = Vec::new();
    // Each band has the row of the stop at its lower edge.
    for (index, stop) in bands.stops().iter().enumerate().take(bands.colors().len()) {
        let (at, row) = (parts.handle(&bands, stop.value), band_rect(&h, index));
        assert!(parts.bar.top() - 0.5 <= at.y && at.y <= parts.bar.bottom() + 0.5);
        if !(card.contains(at) && card.contains(row.center())) {
            continue;
        }
        // The leader line goes from the handle to the middle of the row.
        let near = |a: f32, b: f32| (a - b).abs() < 0.5;
        let leader = lines.iter().find(|line| {
            let (first, last) = (line[0], line[line.len() - 1]);
            line.len() == 4
                && near(first.y, at.y)
                && near(last.y, row.center().y)
                && first.x < row.left()
                && last.x > row.left()
        });
        assert!(leader.is_some(), "{index}: no line from {at:?} to {row:?}");
        ends.push((at.y, row.center().y));
    }
    assert!(ends.len() >= 5, "{ends:?}");
    // A higher band has a higher handle and a higher row.
    for pair in ends.windows(2) {
        assert!(pair[1].0 < pair[0].0 && pair[1].1 < pair[0].1, "{pair:?}");
    }
    // The lowest stop is at the bottom of the bar and the highest at the top.
    let stops = bands.stops();
    let (low, high) = (stops[0].value, stops[stops.len() - 1].value);
    assert!((parts.stop_y(&bands, low) - parts.bar.bottom()).abs() < 0.5);
    assert!((parts.stop_y(&bands, high) - parts.bar.top()).abs() < 0.5);
}

/// A tap of a finger at a place.
fn finger_tap(h: &mut Pad, pos: Pos2) {
    touch(h, egui::TouchPhase::Start, pos, None);
    h.step();
    touch(h, egui::TouchPhase::End, pos, None);
    h.run_steps(3);
}

/// Opens the page of the color stops of the open raster layer.
fn open_stops(h: &mut Pad) {
    h.state_mut().pad.page = Some(Page::Stops);
    h.run_steps(3);
}

/// Adds stops to the height layer until it has `count`.
fn add_stops(h: &mut Pad, count: usize) {
    let bands = &mut h.state_mut().globe.map.bands;
    while bands.stops().len() < count {
        let value = widest_gap_value(bands);
        bands.add(value).expect("a free value");
    }
    h.run_steps(3);
}

/// A tap of a finger on the handle of a stop.
fn tap_handle(h: &mut Pad, index: usize) {
    let pos = handle(h, index);
    finger_tap(h, pos);
}

/// The parts of the control of the page of the color stops. A part of them
/// can be out of the card.
fn column_parts(h: &Pad) -> ColumnParts {
    let grab = h.get_by_label(GRADIENT).rect();
    let right = band_rect(h, 0).right();
    let area = Rect::from_min_max(grab.min, pos2(right, grab.bottom()));
    ColumnParts::new(area, &COLUMN)
}

/// The row of a band on the page of the color stops.
fn band_rect(h: &Pad, index: usize) -> Rect {
    h.get_by_label(&band_row(index)).rect()
}

/// The place of a value on the bar of the page of the color stops.
fn on_bar(h: &Pad, value: f64) -> Pos2 {
    let parts = column_parts(h);
    let y = parts.stop_y(&h.state().globe.map.bands, value);
    pos2(parts.bar.center().x, y)
}

/// The middle of the handle of a stop.
fn handle(h: &Pad, index: usize) -> Pos2 {
    let bands = &h.state().globe.map.bands;
    column_parts(h).handle(bands, bands.stops()[index].value)
}

/// Scrolls the Layers panel until the height `y` is near the middle of the
/// card, with drags on the margin at the left of the rows.
fn scroll_to_y(h: &mut Pad, y: impl Fn(&Pad) -> f32) {
    for _ in 0..8 {
        let card = panel(h, Panel::Layers).unwrap();
        let at = y(h);
        if card.top() + 120.0 < at && at < card.bottom() - 120.0 {
            return;
        }
        // The drag starts above or below the width handle, which is at the
        // middle of the left edge of the dock.
        let reach = card.height() / 2.0 - 60.0;
        let dy = (card.center().y - at).clamp(-reach, reach);
        let side = if dy > 0.0 { -90.0 } else { 90.0 };
        let from = pos2(card.left() + 6.0, card.center().y + side);
        drag(h, from, from + vec2(0.0, dy), None);
    }
}

/// Scrolls the Layers panel until a row is near the middle of the card.
fn scroll_to_row(h: &mut Pad, name: &str) {
    scroll_to_y(h, |h| h.get_by_label(name).rect().center().y);
}

/// Scrolls the page of the color stops until the handle of a stop is near
/// the middle of the card.
fn show_stop(h: &mut Pad, index: usize) {
    scroll_to_y(h, |h| handle(h, index).y);
}

#[test]
fn a_tap_on_the_bar_adds_a_stop_a_vertical_drag_moves_it_and_the_card_removes_it() {
    use vale_app::ui::pad::raster::REMOVE_STOP;
    let mut h = painted_pad(BOARDS[1]);
    let height = open_height(&mut h);
    open_stops(&mut h);
    let stops = |h: &Pad| h.state().globe.map.bands.stops().to_vec();
    let before = stops(&h);
    let sea = h.state().globe.map.bands.sea().unwrap();
    // The two stops with the widest gap on the bar.
    let gap = |h: &Pad, i: &usize| handle(h, *i).y - handle(h, i + 1).y;
    let low = (0..before.len() - 1)
        .max_by(|a, b| gap(&h, a).total_cmp(&gap(&h, b)))
        .unwrap();
    show_stop(&mut h, low);
    // A tap on the bar between two stops adds a stop there.
    let (a, b) = (
        on_bar(&h, before[low].value),
        on_bar(&h, before[low + 1].value),
    );
    assert!(a.y - b.y > 40.0, "{a:?} {b:?}");
    finger_tap(&mut h, a.lerp(b, 0.5));
    let added = stops(&h);
    let index = low + 1;
    assert_eq!(added.len(), before.len() + 1);
    assert!(added[index].value > before[low].value);
    assert!(added[index].value < before[low + 1].value);
    assert_eq!(h.state().globe.preview.selected, Some((height, index)));

    // A drag up on the handle moves the stop up, and the page does not
    // scroll.
    let row = band_rect(&h, index);
    let from = handle(&h, index);
    let up = (from.y - on_bar(&h, added[index + 1].value).y) / 2.0;
    drag(&mut h, from, from - vec2(0.0, up), None);
    let moved = stops(&h)[index].value;
    assert!(moved > added[index].value, "{moved} {:?}", added[index]);
    assert!(moved < added[index + 1].value, "{moved}");
    assert_eq!(stops(&h).len(), added.len());
    assert_eq!(h.state().globe.preview.selected, Some((height, index)));
    assert_eq!(band_rect(&h, index), row);

    // The button of the card removes the stop.
    tap(&mut h, REMOVE_STOP);
    assert_eq!(stops(&h), before);
    assert_eq!(h.state().globe.preview.selected, None);

    // A drag on a fixed stop does not move it.
    let from = handle(&h, sea);
    drag(&mut h, from, from - vec2(0.0, 30.0), None);
    assert_eq!(stops(&h), before);
}

#[test]
fn a_vertical_drag_on_the_bar_or_on_a_row_scrolls_the_page() {
    // With 20 stops, the page is longer than its card.
    for size in [BOARDS[2], BOARDS[3]] {
        let mut h = painted_pad(size);
        open_height(&mut h);
        add_stops(&mut h, 20);
        open_stops(&mut h);
        let stops = h.state().globe.map.bands.stops().to_vec();
        let card = panel(&h, Panel::Layers).unwrap();
        let inside = |y: f32| card.top() + 80.0 < y && y < card.bottom() - 80.0;
        // A place on the bar far from the handles.
        let ys: Vec<f32> = (0..stops.len()).map(|i| handle(&h, i).y).collect();
        let bar = column_parts(&h).bar.center().x;
        let gap = ys
            .windows(2)
            .map(|w| (w[0] + w[1]) / 2.0)
            .find(|y| inside(*y))
            .unwrap();
        let on_bar = pos2(bar, gap);
        let row = (0..stops.len() - 1)
            .map(|i| band_rect(&h, i).center())
            .find(|p| inside(p.y))
            .unwrap();
        // A drag up on the bar scrolls up, and a drag down on a row scrolls
        // back down.
        for (what, from, dy) in [("bar", on_bar, -30.0), ("row", row, 30.0)] {
            let top = band_rect(&h, 0).top();
            drag(&mut h, from, from + vec2(0.0, dy), None);
            let moved = band_rect(&h, 0).top() - top;
            assert!(moved * dy > 100.0, "{size:?} {what}: {moved}");
            assert_eq!(h.state().globe.map.bands.stops(), stops, "{size:?} {what}");
        }
        assert_eq!(h.state().globe.preview.selected, None, "{size:?}");
    }
}

#[test]
fn a_tap_on_a_row_opens_the_card_of_its_stop_and_a_second_tap_closes_it() {
    let mut h = painted_pad(BOARDS[1]);
    let height = open_height(&mut h);
    open_stops(&mut h);
    let sea = h.state().globe.map.bands.sea().unwrap();
    let index = sea + 1;
    show_stop(&mut h, index);
    let row = band_rect(&h, index);
    finger_tap(&mut h, row.center());
    assert_eq!(h.state().globe.preview.selected, Some((height, index)));
    let stop = h.state().pad.style.card.unwrap();
    assert!(stop.top() < row.center().y && row.center().y < stop.bottom());
    finger_tap(&mut h, row.center());
    assert_eq!(h.state().globe.preview.selected, None);
    assert_eq!(h.state().pad.style.card, None);
}

/// The shapes that the last frame painted, with no nesting.
fn painted_shapes(h: &Pad) -> Vec<egui::Shape> {
    fn visit(shape: &egui::Shape, out: &mut Vec<egui::Shape>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| visit(s, out)),
            shape => out.push(shape.clone()),
        }
    }
    let mut out = Vec::new();
    for clipped in &h.output().shapes {
        visit(&clipped.shape, &mut out);
    }
    out
}

fn color32(c: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

#[test]
fn one_row_per_band_highest_first() {
    use vale_terrain::StopLine;
    let mut h = painted_pad(BOARDS[1]);
    open_height(&mut h);
    let line = StopLine {
        on: true,
        weight: 3.0,
        color: [250, 10, 10],
        ..StopLine::default()
    };
    let bands = &mut h.state_mut().globe.map.bands;
    let sea = bands.sea().unwrap();
    assert!(bands.set_line(sea, line));
    open_stops(&mut h);
    show_stop(&mut h, sea);
    let bands = h.state().globe.map.bands.clone();
    let count = bands.colors().len();
    assert!(h.query_by_label(&band_row(count)).is_none());
    let rows: Vec<Rect> = (0..count).map(|i| band_rect(&h, i)).collect();
    for pair in rows.windows(2) {
        assert!(pair[1].bottom() <= pair[0].top() + 0.5, "{pair:?}");
    }
    // A row shows the color of its band, the value of its lower edge, and
    // the line at that edge.
    let card = panel(&h, Panel::Layers).unwrap();
    let texts = painted_texts(&h);
    let shapes = painted_shapes(&h);
    let span = bands.max() - bands.min();
    let mut seen = 0;
    for (index, row) in rows.iter().enumerate() {
        if !card.contains_rect(*row) {
            continue;
        }
        seen += 1;
        let value = vale_app::world::value_text(bands.stops()[index].value, span, "m");
        let text = texts
            .iter()
            .any(|(t, r)| *t == value && row.contains(r.center()));
        assert!(text, "{index}: no {value}");
        let color = color32(bands.colors()[index]);
        let swatch = shapes.iter().any(|shape| match shape {
            egui::Shape::Rect(r) => r.fill == color && row.contains_rect(r.rect),
            _ => false,
        });
        assert!(swatch, "{index}: no swatch");
        let lined = shapes.iter().any(|shape| match shape {
            egui::Shape::LineSegment { points, stroke } => {
                stroke.color == color32(line.color) && row.contains(points[0])
            }
            _ => false,
        });
        assert_eq!(lined, index == sea, "{index}");
    }
    assert!(seen >= 5, "{seen}");
}

#[test]
fn a_tap_on_a_row_or_lower_handle_opens_the_band_card() {
    let mut h = painted_pad(BOARDS[1]);
    let height = open_height(&mut h);
    open_stops(&mut h);
    let sea = h.state().globe.map.bands.sea().unwrap();
    for index in [0, sea, sea + 2] {
        show_stop(&mut h, index);
        let row = band_rect(&h, index);
        finger_tap(&mut h, row.center());
        assert_eq!(h.state().globe.preview.selected, Some((height, index)));
        let card = h.state().pad.style.card.unwrap();
        assert!(card.top() < row.center().y && row.center().y < card.bottom());
        finger_tap(&mut h, row.center());
        assert_eq!(h.state().globe.preview.selected, None);
        // The handle of the lower edge of the band opens the same card.
        tap_handle(&mut h, index);
        assert_eq!(h.state().globe.preview.selected, Some((height, index)));
        assert!(h.state().pad.style.card.is_some());
        tap_handle(&mut h, index);
        assert_eq!(h.state().globe.preview.selected, None);
    }
}

#[test]
fn a_tap_on_the_top_handle_does_nothing() {
    let mut h = painted_pad(BOARDS[1]);
    let height = open_height(&mut h);
    open_stops(&mut h);
    let before = h.state().globe.map.bands.clone();
    let top = before.stops().len() - 1;
    show_stop(&mut h, top);
    tap_handle(&mut h, top);
    assert_eq!(h.state().globe.preview.selected, None);
    assert_eq!(h.state().pad.style.card, None);
    assert_eq!(h.state().globe.map.bands, before);
    // The card of a band stays open.
    let below = top - 1;
    tap_handle(&mut h, below);
    assert_eq!(h.state().globe.preview.selected, Some((height, below)));
    tap_handle(&mut h, top);
    assert_eq!(h.state().globe.preview.selected, Some((height, below)));
    assert!(h.state().pad.style.card.is_some());
    assert_eq!(h.state().globe.map.bands, before);
}

#[test]
fn the_lowest_band_card_has_no_value_or_line() {
    use vale_app::ui::pad::raster::{COLOR, LINE, LINE_WEIGHT, REMOVE_STOP, STOP_VALUE};
    let mut h = painted_pad(BOARDS[1]);
    let height = open_height(&mut h);
    open_stops(&mut h);
    show_stop(&mut h, 0);
    tap_handle(&mut h, 0);
    assert_eq!(h.state().globe.preview.selected, Some((height, 0)));
    check_controls(&h, "the card of the lowest band");
    for name in [STOP_VALUE, LINE, LINE_WEIGHT, REMOVE_STOP] {
        assert!(control(&h, name).is_none(), "{name}");
    }
    // The picker sets the color of the band.
    let before = h.state().globe.map.bands.colors().to_vec();
    let square = control(&h, &format!("{COLOR} shade")).unwrap();
    finger_tap(&mut h, square.center() + vec2(40.0, 30.0));
    let after = h.state().globe.map.bands.colors().to_vec();
    assert_ne!(after[0], before[0]);
    assert_eq!(after[1..], before[1..]);
}

#[test]
fn the_band_card_renames_the_band() {
    use vale_app::ui::pad::raster::BAND_NAME;
    let mut h = painted_pad(BOARDS[1]);
    let dir = tempfile::tempdir().unwrap();
    let state = h.state_mut();
    state.session = Some(Session::open(&dir.path().join("w.gpkg"), &mut state.doc).unwrap());
    state.save_waiting().unwrap();
    let height = open_height(&mut h);
    open_stops(&mut h);
    let index = h.state().globe.map.bands.sea().unwrap();
    show_stop(&mut h, index);
    tap_handle(&mut h, index);
    assert_eq!(h.state().globe.preview.selected, Some((height, index)));
    check_controls(&h, "the card with the name field");
    type_text(&mut h, BAND_NAME, "Lowland");
    assert_eq!(h.state().globe.map.bands.name(index), "Lowland");

    // The name reaches the record, and an undo takes it away.
    h.state_mut().save_waiting().unwrap();
    let names = |h: &Pad| h.state().doc.project.raster(height).unwrap().names.clone();
    assert_eq!(names(&h)[index], "Lowland");
    h.state_mut().undo();
    h.run_steps(2);
    assert_eq!(h.state().globe.map.bands.name(index), "");
    assert!(names(&h).is_empty());
}

#[test]
fn add_and_remove_keep_band_colors() {
    use vale_app::ui::pad::raster::REMOVE_STOP;
    let mut h = painted_pad(BOARDS[1]);
    let height = open_height(&mut h);
    let bands = &mut h.state_mut().globe.map.bands;
    for i in 0..bands.colors().len() {
        bands.set_color(i, [10 * i as u8, 100, 200]);
    }
    let before = bands.clone();
    open_stops(&mut h);
    let colors = |h: &Pad| h.state().globe.map.bands.colors().to_vec();
    // A tap on the bar in the band from 1000 m to 2000 m splits it, and the
    // two parts keep its color.
    let low = before
        .stops()
        .iter()
        .position(|s| s.value == 1000.0)
        .unwrap();
    show_stop(&mut h, low);
    let (a, b) = (on_bar(&h, 1000.0), on_bar(&h, 2000.0));
    finger_tap(&mut h, a.lerp(b, 0.5));
    assert_eq!(h.state().globe.preview.selected, Some((height, low + 1)));
    let mut want = before.colors().to_vec();
    want.insert(low + 1, want[low]);
    assert_eq!(colors(&h), want);
    tap(&mut h, REMOVE_STOP);
    assert_eq!(h.state().globe.map.bands, before);
    // The removal of the stop at 1000 m joins the band below it and the band
    // above it, with the color of the band below.
    tap_handle(&mut h, low);
    assert_eq!(h.state().globe.preview.selected, Some((height, low)));
    tap(&mut h, REMOVE_STOP);
    let mut want = before.colors().to_vec();
    want.remove(low);
    assert_eq!(colors(&h), want);
    assert_eq!(colors(&h)[low - 1], before.colors()[low - 1]);
}

#[test]
fn a_tap_on_the_bar_adds_a_stop_until_the_layer_has_the_most_bands() {
    use vale_app::ui::pad::raster::{FULL, RESET_STOPS};
    use vale_terrain::MAX_BANDS;
    let mut h = painted_pad(BOARDS[1]);
    let height = open_height(&mut h);
    open_stops(&mut h);
    let has_note = |h: &Pad| painted_texts(h).iter().any(|(t, _)| t == FULL);
    assert!(!has_note(&h));
    // A tap in the widest gap of the bar adds a stop there.
    let before = h.state().globe.map.bands.clone();
    let value = widest_gap_value(&before);
    scroll_to_y(&mut h, |h| on_bar(h, value).y);
    let at = on_bar(&h, value);
    finger_tap(&mut h, at);
    let stops = h.state().globe.map.bands.stops().to_vec();
    assert_eq!(stops.len(), before.stops().len() + 1);
    let index = stops.iter().position(|s| (s.value - value).abs() < 50.0);
    let index = index.unwrap_or_else(|| panic!("no stop near {value}: {stops:?}"));
    assert_eq!(h.state().globe.preview.selected, Some((height, index)));
    assert!(h.state().pad.style.card.is_some());
    // With the most bands, the page shows a note, and a tap adds no stop.
    tap_handle(&mut h, index);
    add_stops(&mut h, MAX_BANDS + 1);
    scroll_to_row(&mut h, RESET_STOPS);
    assert!(has_note(&h));
    check_controls(&h, "on the page of a full layer");
    let value = widest_gap_value(&h.state().globe.map.bands);
    scroll_to_y(&mut h, |h| on_bar(h, value).y);
    let at = on_bar(&h, value);
    finger_tap(&mut h, at);
    assert_eq!(h.state().globe.map.bands.stops().len(), MAX_BANDS + 1);
}

#[test]
fn a_tap_on_a_handle_opens_the_card_of_its_stop_and_a_tap_elsewhere_closes_it() {
    use vale_app::ui::pad::raster::COLOR;
    let mut h = painted_pad(BOARDS[1]);
    let height = open_height(&mut h);
    open_stops(&mut h);
    show_stop(&mut h, 1);
    let shade = format!("{COLOR} shade");
    let card = |h: &Pad| h.state().pad.style.card;
    tap_handle(&mut h, 1);
    assert_eq!(h.state().globe.preview.selected, Some((height, 1)));
    // The card is at the left of the panel, at the height of the row.
    let stop = card(&h).unwrap();
    let panel = panel(&h, Panel::Layers).unwrap();
    assert!(stop.right() < panel.left(), "{stop:?}");
    let y = band_rect(&h, 1).center().y;
    assert!(stop.top() < y && y < stop.bottom(), "{stop:?}");
    // A tap in the square of the picker changes the color of the stop.
    let bands = |h: &Pad| h.state().globe.map.bands.clone();
    let before = bands(&h);
    let square = control(&h, &shade).unwrap();
    finger_tap(&mut h, square.center() + vec2(40.0, 30.0));
    let after = bands(&h);
    assert_ne!(after.colors()[1], before.colors()[1]);
    assert_eq!(after.stops()[1].value, before.stops()[1].value);
    assert_eq!(h.state().globe.preview.selected, Some((height, 1)));
    // A second tap on the handle closes the card.
    tap_handle(&mut h, 1);
    assert_eq!(h.state().globe.preview.selected, None);
    assert_eq!(card(&h), None);
    assert!(control(&h, &shade).is_none());
    // A tap at another place on the panel also closes the card.
    tap_handle(&mut h, 1);
    assert!(card(&h).is_some());
    // The margin at the left of the bar has no control.
    let parts = column_parts(&h);
    let left = pos2(panel.left() + 6.0, band_rect(&h, 1).center().y);
    assert!(left.x < parts.bar.left(), "{left:?}");
    finger_tap(&mut h, left);
    assert_eq!(h.state().globe.preview.selected, None);
    assert_eq!(card(&h), None);
}

#[test]
fn the_card_of_a_stop_sets_its_line() {
    use vale_app::ui::pad::raster::{COLOR, LINE, LINE_COLOR, LINE_WEIGHT};
    let mut h = painted_pad(BOARDS[1]);
    let height = open_height(&mut h);
    open_stops(&mut h);
    let sea = h.state().globe.map.bands.sea().unwrap();
    show_stop(&mut h, sea);
    tap_handle(&mut h, sea);
    assert_eq!(h.state().globe.preview.selected, Some((height, sea)));
    check_controls(&h, "the card of a stop");
    let stop = |h: &Pad| h.state().globe.map.bands.stops()[sea];
    let colors = |h: &Pad| h.state().globe.map.bands.colors().to_vec();
    let before = stop(&h);
    let colors_before = colors(&h);
    assert!(!before.line.on);
    assert!(!switch_is_on(&h, LINE));

    let switch = control(&h, LINE).unwrap();
    finger_tap(&mut h, switch.center());
    assert!(stop(&h).line.on);
    assert!(switch_is_on(&h, LINE));

    // The weight goes in steps of a quarter point from a quarter point to
    // 8 points.
    // A button is off at the end of the range.
    for (up, end) in [(true, 8.0), (false, 0.25)] {
        let button = format!("{} {LINE_WEIGHT}", if up { "Increase" } else { "Decrease" });
        for _ in 0..40 {
            if control(&h, &button).is_none() {
                break;
            }
            let weight = stop(&h).line.weight;
            step(&mut h, LINE_WEIGHT, up);
            assert_eq!((stop(&h).line.weight - weight).abs(), 0.25);
        }
        assert_eq!(stop(&h).line.weight, end);
    }
    // The keypad of the weight opens in the place of the picker, inside the
    // card, and a typed weight takes the nearest step.
    type_value(&mut h, LINE_WEIGHT, "2.4");
    assert_eq!(stop(&h).line.weight, 2.5);
    type_value(&mut h, LINE_WEIGHT, "0.25");

    // With the line chosen, the picker sets the color of the line and not
    // the colors of the stop.
    let choice = control(&h, LINE_COLOR).unwrap();
    finger_tap(&mut h, choice.center());
    let square = control(&h, &format!("{COLOR} shade")).unwrap();
    finger_tap(&mut h, square.center() + vec2(40.0, 30.0));
    let after = stop(&h);
    assert_ne!(after.line.color, before.line.color);
    assert_eq!(colors(&h), colors_before);
    assert_eq!(h.state().globe.preview.selected, Some((height, sea)));

    // The project gets the line with the stop.
    let mut raster = h.state().doc.project.raster(height).unwrap().clone();
    vale_app::session::sync_record(&mut raster, &h.state().globe.map);
    let line = raster.stops[sea].line;
    let want = (true, 0.25, after.line.color);
    assert_eq!((line.on, line.weight, line.color), want);

    // A darker line takes the sea color of the stop at sea level, and it has
    // no color to pick.
    use vale_app::ui::pad::raster::{CUSTOM, DARKEN, DARKER};
    let darker = control(&h, DARKER).unwrap();
    finger_tap(&mut h, darker.center());
    assert!(stop(&h).line.darker);
    assert!(control(&h, LINE_COLOR).is_none());
    let slider = control(&h, DARKEN).unwrap();
    drag(
        &mut h,
        slider.left_center(),
        slider.right_center() + vec2(40.0, 0.0),
        None,
    );
    assert_eq!(stop(&h).line.darken, 1.0);
    let bands = &h.state().globe.map.bands;
    assert_eq!(bands.line_color(sea), Some([0; 3]));
    check_controls(&h, "the card of a stop with a darker line");
    let custom = control(&h, CUSTOM).unwrap();
    finger_tap(&mut h, custom.center());
    assert!(!stop(&h).line.darker);
    assert_eq!(stop(&h).line.color, after.line.color);
    assert!(control(&h, DARKEN).is_none());
}

/// Opens the keypad of a field, taps the keys of `text`, and closes the
/// keypad.
fn type_keys(h: &mut Pad, field: &str, text: &str) {
    use vale_app::ui::pad::widgets::DONE;
    tap(h, field);
    for c in text.chars() {
        tap(h, &format!("{field} {c}"));
    }
    tap(h, &format!("{field} {DONE}"));
}

/// Holds the clipboard, which all tests share.
fn one_clipboard_test() -> std::sync::MutexGuard<'static, ()> {
    static CLIPBOARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
    CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner())
}

/// A long press of a finger on the middle of a control.
fn long_press(h: &mut Pad, name: &str) {
    let rect = control(h, name).unwrap_or_else(|| panic!("no control `{name}`"));
    touch(h, egui::TouchPhase::Start, rect.center(), None);
    h.run_steps(8);
    touch(h, egui::TouchPhase::End, rect.center(), None);
    h.run_steps(2);
}

/// True if no text field has the keyboard.
fn no_keyboard(h: &Pad) -> bool {
    h.ctx.memory(|m| m.focused()).is_none() && h.output().platform_output.ime.is_none()
}

#[test]
fn the_hex_keypad_sets_the_color_of_a_stop_and_the_card_stays_in_place() {
    use vale_app::ui::pad::raster::{COLOR, DARKER, LINE};
    use vale_app::ui::pad::widgets::DELETE;
    let mut h = painted_pad(BOARDS[1]);
    open_height(&mut h);
    open_stops(&mut h);
    let sea = h.state().globe.map.bands.sea().unwrap();
    show_stop(&mut h, sea);
    tap_handle(&mut h, sea);
    // A darker line makes the card taller.
    tap(&mut h, LINE);
    tap(&mut h, DARKER);
    let card = |h: &Pad| h.state().pad.style.card.unwrap();
    let before = card(&h);
    let hex = format!("{COLOR} hex");
    let shade = format!("{COLOR} shade");
    tap(&mut h, &hex);
    assert!(no_keyboard(&h));
    assert!(control(&h, &shade).is_none());
    assert!(control(&h, &format!("{hex} A")).is_some());
    assert_eq!(card(&h), before);
    check_controls(&h, "the hex keypad of a stop");

    // The first key takes the place of the whole code.
    for key in ["1", "2", "A", "B", "3", "F"] {
        tap(&mut h, &format!("{hex} {key}"));
    }
    let stop = |h: &Pad| h.state().globe.map.bands.colors()[sea];
    assert_eq!(stop(&h), [0x12, 0xab, 0x3f]);
    // A code that is too short does not change the color.
    tap(&mut h, &format!("{hex} {DELETE}"));
    assert_eq!(stop(&h), [0x12, 0xab, 0x3f]);
    tap(&mut h, &format!("{hex} 0"));
    assert_eq!(stop(&h), [0x12, 0xab, 0x30]);

    // A second tap on the field closes the keypad.
    tap(&mut h, &hex);
    assert!(control(&h, &shade).is_some());
    type_keys(&mut h, &hex, "00FF00");
    assert_eq!(stop(&h), [0, 255, 0]);
    assert!(control(&h, &shade).is_some());
    assert_eq!(card(&h), before);
}

#[test]
fn a_band_color_and_the_shade_color_stay_opaque() {
    use vale_app::ui::pad::raster::COLOR;
    let mut h = painted_pad(BOARDS[1]);
    open_height(&mut h);
    open_stops(&mut h);
    let sea = h.state().globe.map.bands.sea().unwrap();
    show_stop(&mut h, sea);
    tap_handle(&mut h, sea);
    assert!(control(&h, &format!("{COLOR} hue")).is_some());
    assert!(control(&h, &format!("{COLOR} opacity")).is_none());
    // A code with an alpha does not change an opaque color.
    let hex = format!("{COLOR} hex");
    type_keys(&mut h, &hex, "00FF0080");
    assert_eq!(h.state().globe.map.bands.colors()[sea], [0, 255, 0]);

    let (h, _) = eyedropper_page();
    assert!(control(&h, "Shade color hue").is_some());
    assert!(control(&h, "Shade color opacity").is_none());
}

#[test]
fn a_long_press_on_a_field_opens_copy_and_paste_with_no_keyboard() {
    let _clipboard = one_clipboard_test();
    use vale_app::ui::pad::raster::COLOR;
    use vale_app::ui::pad::widgets::{COPY, PASTE};
    let mut h = painted_pad(BOARDS[1]);
    open_height(&mut h);
    open_stops(&mut h);
    let sea = h.state().globe.map.bands.sea().unwrap();
    show_stop(&mut h, sea);
    tap_handle(&mut h, sea);
    let hex = format!("{COLOR} hex");
    let code = vale_app::ui::pad::widgets::hex_text(vale_app::ui::pad::widgets::opaque(
        h.state().globe.map.bands.colors()[sea],
    ));
    long_press(&mut h, &hex);
    assert!(no_keyboard(&h));
    assert!(control(&h, COPY).is_some());
    screenshot::save(&mut h, "pad-edit-menu");
    check_controls(&h, "the edit menu of a field");
    tap(&mut h, COPY);
    assert!(control(&h, COPY).is_none());
    assert_eq!(vale_app::clipboard::get(), Some(code));

    // A paste takes the place of the whole code, and the card stays open.
    vale_app::clipboard::set(&h.ctx, " #123456 ");
    long_press(&mut h, &hex);
    tap(&mut h, PASTE);
    assert!(no_keyboard(&h));
    assert_eq!(h.state().globe.map.bands.colors()[sea], [0x12, 0x34, 0x56]);
    assert!(h.state().globe.preview.selected.is_some());

    // A press outside the menu closes it.
    long_press(&mut h, &hex);
    let shade = control(&h, &format!("{COLOR} shade")).unwrap();
    touch(&mut h, egui::TouchPhase::Start, shade.center(), None);
    h.step();
    touch(&mut h, egui::TouchPhase::End, shade.center(), None);
    h.run_steps(2);
    assert!(control(&h, COPY).is_none());
}

#[test]
fn a_paste_from_the_edit_menu_renames_a_layer() {
    let _clipboard = one_clipboard_test();
    use vale_app::ui::pad::widgets::PASTE;
    let mut h = painted_pad(BOARDS[1]);
    let height = open_height(&mut h);
    vale_app::clipboard::set(&h.ctx, "Ridges");
    long_press(&mut h, LAYER_NAME);
    assert!(no_keyboard(&h));
    tap(&mut h, PASTE);
    assert_eq!(raster_name(&h, height), "Ridges");
}

/// The texts that the last frame painted, with their places.
fn painted_texts(h: &Pad) -> Vec<(String, Rect)> {
    fn visit(shape: &egui::Shape, texts: &mut Vec<(String, Rect)>) {
        match shape {
            egui::Shape::Text(text) => {
                texts.push((text.galley.text().to_owned(), text.visual_bounding_rect()));
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| visit(s, texts)),
            _ => {}
        }
    }
    let mut texts = Vec::new();
    for clipped in &h.output().shapes {
        visit(&clipped.shape, &mut texts);
    }
    texts
}

#[test]
fn a_dragged_stop_shows_its_value_left_of_the_bar() {
    let mut h = painted_pad(BOARDS[1]);
    open_height(&mut h);
    open_stops(&mut h);
    // The stop that can move the farthest up.
    let bands = h.state().globe.map.bands.clone();
    let stops = bands.stops();
    let gap = |h: &Pad, i: &usize| handle(h, *i).y - handle(h, i + 1).y;
    let index = (1..stops.len() - 1)
        .filter(|i| !bands.fixed(*i))
        .max_by(|a, b| gap(&h, a).total_cmp(&gap(&h, b)))
        .unwrap();
    show_stop(&mut h, index);
    let (from, next) = (handle(&h, index), handle(&h, index + 1).y);
    let to = pos2(from.x, (next + 10.0).max(from.y - 150.0));
    let bar = column_parts(&h).bar.left();
    let span = bands.max() - bands.min();
    touch(&mut h, egui::TouchPhase::Start, from, None);
    h.step();
    let start = h.state().globe.map.bands.stops()[index].value;
    let mut seen = 0;
    for i in 1..=6 {
        let pos = from.lerp(to, i as f32 / 6.0);
        touch(&mut h, egui::TouchPhase::Move, pos, None);
        h.step();
        let value = h.state().globe.map.bands.stops()[index].value;
        // The drag starts after a few points.
        if value == start {
            continue;
        }
        seen += 1;
        let text = vale_app::world::value_text(value, span, "m");
        let texts = painted_texts(&h);
        let shown = texts.iter().find(|(t, r)| *t == text && r.right() < bar);
        let (_, rect) = shown.unwrap_or_else(|| panic!("frame {i}: no {text} in {texts:?}"));
        // The value is at the height of the finger, so the finger does not
        // hide it.
        assert!(
            (rect.center().y - pos.y).abs() < 30.0,
            "frame {i}: {rect:?} {pos:?}"
        );
    }
    assert!(seen >= 3, "{seen}");
    touch(&mut h, egui::TouchPhase::End, to, None);
    h.run_steps(3);
    let value = h.state().globe.map.bands.stops()[index].value;
    let text = vale_app::world::value_text(value, span, "m");
    let left = |(t, r): &(String, Rect)| *t == text && r.right() < bar;
    assert!(!painted_texts(&h).iter().any(left));
}

#[test]
fn reset_colors_asks_first_and_puts_back_the_default_stops() {
    use vale_app::ui::pad::raster::{RESET, RESET_STOPS};
    let mut h = painted_pad(BOARDS[0]);
    let height = open_height(&mut h);
    let bands = h.state().globe.map.bands.clone();
    let default = vale_terrain::Bands::with_range(bands.min(), bands.max(), true).unwrap();
    let default = default.stops().to_vec();
    let disabled = |h: &Pad| h.get_by_label(RESET_STOPS).accesskit_node().is_disabled();
    // A new stop changes the stops.
    let count = bands.stops().len();
    open_stops(&mut h);
    let value = widest_gap_value(&bands);
    scroll_to_y(&mut h, |h| on_bar(h, value).y);
    let at = on_bar(&h, value);
    finger_tap(&mut h, at);
    assert_eq!(h.state().globe.map.bands.stops().len(), count + 1);
    // A second tap on its handle closes its card.
    let (_, index) = h.state().globe.preview.selected.unwrap();
    tap_handle(&mut h, index);
    assert_eq!(h.state().globe.preview.selected, None);
    scroll_to_row(&mut h, RESET_STOPS);
    assert!(!disabled(&h));
    // A tap on the row opens the card, and a tap outside closes it with no
    // change.
    tap(&mut h, RESET_STOPS);
    let card = h.state().pad.style.reset_card.unwrap();
    check_controls(&h, "with the card of the reset");
    let panel = panel(&h, Panel::Layers).unwrap();
    assert!(card.right() < panel.left(), "{card:?}");
    finger_tap(&mut h, pos2(panel.left() + 6.0, panel.top() + 130.0));
    assert_eq!(h.state().pad.style.reset_card, None);
    assert_eq!(h.state().globe.map.bands.stops().len(), count + 1);
    // The Reset button of the card puts back the default stops.
    tap(&mut h, RESET_STOPS);
    tap(&mut h, RESET);
    assert_eq!(h.state().globe.map.bands.stops(), default);
    assert_eq!(h.state().pad.style.reset_card, None);
    assert_eq!(h.state().globe.preview.selected, None);
    assert!(disabled(&h));
    assert_eq!(h.state().pad.layer, Some(LayerRef::Raster(height)));
    assert_eq!(h.state().pad.page, Some(Page::Stops));
}

#[test]
fn an_imported_ramp_gives_one_color_per_band() {
    use vale_terrain::Part;
    let mut h = painted_pad(BOARDS[0]);
    let height = open_height(&mut h);
    let colors = [[200, 30, 30], [30, 200, 30], [30, 30, 200]];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legend.png");
    let legend = image::RgbImage::from_fn(90, 12, |x, _| image::Rgb(colors[x as usize / 30]));
    legend.save(&path).unwrap();
    h.state_mut()
        .run_action(Action::ImportRamp(Some(height), path));
    h.run();
    let before = h.state().globe.map.bands.clone();
    let count = before.colors().len();
    assert!(count > colors.len());
    // The new ramp is the last ramp of the library.
    let at = h.state().ramps.list().len() - 1;

    // A fit keeps the stops, and each band takes the color at its middle
    // along the ramp.
    h.state_mut()
        .apply_ramp(height, at, true, Part::All)
        .unwrap();
    let bands = h.state().globe.map.bands.clone();
    assert_eq!(bands.stops(), before.stops());
    assert_eq!(bands.colors().len(), count);
    let want: Vec<[u8; 3]> = (0..count)
        .map(|k| colors[(2 * k + 1) * 3 / (2 * count)])
        .collect();
    assert_eq!(bands.colors(), want);
    let shown: Vec<[u8; 3]> = bands.bands().iter().map(|b| b.color).collect();
    assert_eq!(shown, want);

    // A ramp with new stops gives one band for each color.
    h.state_mut()
        .apply_ramp(height, at, false, Part::All)
        .unwrap();
    assert_eq!(h.state().globe.map.bands.colors(), colors);
}

#[test]
fn a_custom_ramp_gets_labels_and_a_built_in_ramp_shows_them() {
    use vale_app::ramps::KOPPEN;
    use vale_app::ui::pad::raster::{COLOR_RAMPS, EDIT_LABELS, VIEW_LABELS};
    let mut h = painted_pad(BOARDS[0]);
    let height = open_height(&mut h);
    open_stops(&mut h);
    scroll_to_row(&mut h, COLOR_RAMPS);
    tap(&mut h, COLOR_RAMPS);
    let colors = [[33, 113, 181], [116, 196, 118], [140, 81, 10]];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zones.png");
    let legend = image::RgbImage::from_fn(90, 12, |x, _| image::Rgb(colors[x as usize / 30]));
    legend.save(&path).unwrap();
    h.state_mut()
        .run_action(Action::ImportRamp(Some(height), path));
    h.run();
    let at = h.state().ramps.list().len() - 1;

    scroll_to_control(&mut h, "zones");
    tap(&mut h, "zones");
    tap(&mut h, EDIT_LABELS);
    assert_eq!(h.state().pad.page, Some(Page::RampLabels(at)));
    check_controls(&h, "the labels of a ramp");
    type_text(&mut h, "Label 2", "Cfb");
    screenshot::save(&mut h, "pad-ramp-labels");
    assert_eq!(h.state().ramps.custom()[0].labels, ["", "Cfb", ""]);
    tap(&mut h, "Back to Color ramps");
    assert_eq!(h.state().pad.page, Some(Page::Ramps));

    // The labels of a built-in ramp show, and they do not change.
    scroll_to_control(&mut h, KOPPEN);
    tap(&mut h, KOPPEN);
    assert!(control(&h, EDIT_LABELS).is_none());
    tap(&mut h, VIEW_LABELS);
    assert!(matches!(h.state().pad.page, Some(Page::RampLabels(_))));
    assert!(control(&h, "Label 1").is_none());
    screenshot::save(&mut h, "pad-koppen-labels");
}

#[test]
fn an_imported_ramp_stays_in_the_list_and_applies_from_its_card() {
    use vale_app::ui::pad::raster::{COLOR_RAMPS, DELETE_RAMP, IMPORT_RAMP, REVERSE_RAMP};
    let mut h = painted_pad(BOARDS[0]);
    let height = open_height(&mut h);
    open_stops(&mut h);
    scroll_to_row(&mut h, COLOR_RAMPS);
    // The ramps have their own page in the Layers panel.
    tap(&mut h, COLOR_RAMPS);
    assert_eq!(h.state().pad.page, Some(Page::Ramps));
    tap(&mut h, IMPORT_RAMP);
    assert!(
        h.state()
            .actions
            .contains(&Action::ImportRampDialog(Some(height)))
    );
    h.state_mut().actions.clear();

    let colors = [[33, 113, 181], [116, 196, 118], [140, 81, 10]];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legend.png");
    let legend = image::RgbImage::from_fn(90, 12, |x, _| image::Rgb(colors[x as usize / 30]));
    legend.save(&path).unwrap();
    let before = h.state().globe.map.bands.clone();
    h.state_mut()
        .run_action(Action::ImportRamp(Some(height), path));
    h.run();
    // The import changes no layer.
    assert_eq!(h.state().globe.map.bands, before);
    assert_eq!(h.state().ramps.custom().len(), 1);
    assert!(h.query_by_label("Added legend (3 colors)").is_some());

    // The layer has more bands than the ramp has colors, so the card offers
    // two ways.
    let bands = before.stops().len() - 1;
    scroll_to_control(&mut h, "legend");
    tap(&mut h, "legend");
    assert!(h.state().pad.style.ramp_card.is_some());
    check_controls(&h, "with the card of a ramp");
    assert!(control(&h, &format!("Fit to {bands} bands")).is_some());
    // A reverse keeps the card open, and a second reverse puts back the
    // order.
    tap(&mut h, REVERSE_RAMP);
    let reversed: Vec<_> = colors.iter().rev().copied().collect();
    assert_eq!(h.state().ramps.custom()[0].colors, reversed);
    assert!(h.state().pad.style.ramp_card.is_some());
    tap(&mut h, REVERSE_RAMP);
    assert_eq!(h.state().ramps.custom()[0].colors, colors);
    tap(&mut h, "Make 3 bands");
    let found: Vec<_> = h
        .state()
        .globe
        .map
        .bands
        .bands()
        .iter()
        .map(|b| b.color)
        .collect();
    assert_eq!(found, colors);
    assert_eq!(h.state().pad.style.ramp_card, None);

    scroll_to_control(&mut h, "legend");
    tap(&mut h, "legend");
    tap(&mut h, DELETE_RAMP);
    assert!(h.state().ramps.custom().is_empty());
    assert_eq!(h.state().globe.map.bands.bands().len(), 3);

    // A built-in ramp applies, but it does not reverse, and it stays.
    scroll_to_control(&mut h, "Viridis");
    tap(&mut h, "Viridis");
    assert!(h.state().pad.style.ramp_card.is_some());
    assert!(control(&h, REVERSE_RAMP).is_none());
    assert!(control(&h, DELETE_RAMP).is_none());
    tap(&mut h, "Viridis");

    // A ramp with no blue colors changes only the land.
    let land = [[116, 196, 118], [140, 81, 10], [246, 244, 240]];
    let path = dir.path().join("land.png");
    let legend = image::RgbImage::from_fn(90, 12, |x, _| image::Rgb(land[x as usize / 30]));
    legend.save(&path).unwrap();
    h.state_mut()
        .run_action(Action::ImportRamp(Some(height), path));
    h.run();
    scroll_to_control(&mut h, "land");
    tap(&mut h, "land");
    check_controls(&h, "with the card of a land ramp");
    // The user can choose another part.
    tap(&mut h, "Sea");
    assert!(control(&h, "Make 3 sea bands").is_some());
    tap(&mut h, "Land");
    tap(&mut h, "Make 3 land bands");
    let found: Vec<_> = h
        .state()
        .globe
        .map
        .bands
        .bands()
        .iter()
        .map(|b| b.color)
        .collect();
    assert_eq!(found, [colors[0], land[0], land[1], land[2]]);

    let path = dir.path().join("none.png");
    h.state_mut()
        .run_action(Action::ImportRamp(Some(height), path));
    h.run();
    assert!(h.state().ramp_note.as_ref().is_some_and(|n| n.warning));

    // The back arrow goes back to the page of the color stops.
    tap(&mut h, "Back to Color stops");
    assert_eq!(h.state().pad.page, Some(Page::Stops));
    assert_eq!(h.state().pad.layer, Some(LayerRef::Raster(height)));
}

#[test]
fn a_typed_value_moves_the_selected_stop() {
    use vale_app::ui::pad::raster::STOP_VALUE;
    let mut h = painted_pad(BOARDS[1]);
    let height = open_height(&mut h);
    open_stops(&mut h);
    let sea = h.state().globe.map.bands.sea().unwrap();
    let index = sea + 1;
    show_stop(&mut h, index);
    tap_handle(&mut h, index);
    assert_eq!(h.state().globe.preview.selected, Some((height, index)));
    type_keys(&mut h, STOP_VALUE, "123");
    assert!(no_keyboard(&h));
    assert_eq!(h.state().globe.map.bands.stops()[index].value, 123.0);
    // A value past the next stop stops short of it.
    let next = h.state().globe.map.bands.stops()[index + 1].value;
    type_keys(&mut h, STOP_VALUE, "99999");
    let value = h.state().globe.map.bands.stops()[index].value;
    assert!(value < next && value > next - 2.0, "{value}");
}

#[test]
fn a_tap_chooses_each_display() {
    let mut h = painted_pad(BOARDS[1]);
    let height = open_height(&mut h);
    let display = |h: &Pad| h.state().doc.project.raster(height).unwrap().display;
    assert_eq!(display(&h), RasterDisplay::Bands);
    for (name, shown) in [
        ("Gradient", RasterDisplay::Gradient),
        ("Grey", RasterDisplay::Grey),
        ("Bands", RasterDisplay::Bands),
    ] {
        tap(&mut h, name);
        assert_eq!(display(&h), shown, "{name}");
    }
    let card = panel(&h, Panel::Layers).unwrap();
    let row = h
        .get_by_label(vale_app::ui::pad::raster::COLOR_STOPS)
        .rect();
    assert!(card.contains_rect(row), "{row:?} is not in {card:?}");
}

#[test]
#[ignore = "gpu"]
fn a_change_in_the_panel_of_a_raster_layer_shows_on_the_globe_in_the_same_frame() {
    let _gpu = one_gpu_test();
    let mut h = gpu_pad(BOARDS[1]);
    let height = open_height(&mut h);
    assert!(h.state_mut().set_raster_rivers(height, false));
    let globe = &mut h.state_mut().globe;
    globe.preview.graticule = false;
    globe.view = GlobeView::centered(0.0, 0.0);
    let meters = 1100.0;
    globe.map.fill(meters_to_level(meters));
    h.run_steps(3);
    let center = |h: &mut Pad| {
        let img = h.render().unwrap();
        let c = h.state().globe.rect.center();
        let p = img.get_pixel(c.x as u32, c.y as u32).0;
        [p[0], p[1], p[2]]
    };
    let card = panel(&h, Panel::Layers).unwrap();
    assert!(!card.contains(h.state().globe.rect.center()));
    // The color of the band of the height in the Bands display.
    let band_color = |h: &Pad| {
        let bands = h.state().globe.map.bands.bands();
        let band = bands.iter().find(|b| b.min <= meters && meters < b.max);
        band.unwrap().color
    };
    let near = |a: [u8; 3], b: [u8; 3]| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 3);
    assert!(near(center(&mut h), band_color(&h)));

    // The frame of the tap on Grey draws the globe in grey.
    let pos = control(&h, "Grey").unwrap().center();
    touch(&mut h, egui::TouchPhase::Start, pos, None);
    h.step();
    touch(&mut h, egui::TouchPhase::End, pos, None);
    h.step();
    let display = h.state().doc.project.raster(height).unwrap().display;
    assert_eq!(display, RasterDisplay::Grey);
    let level = (f32::from(meters_to_level(meters)) / 65535.0 * 255.0).round() as u8;
    let grey = center(&mut h);
    assert!(near(grey, [level; 3]), "{grey:?}");
    tap(&mut h, "Bands");

    // Each frame of the drag of a stop draws the globe with the stop of
    // that frame.
    let bands = h.state().globe.map.bands.clone();
    let index = bands.stops().iter().position(|s| s.value > meters).unwrap();
    let to = bands.stops()[index - 1].value + 50.0;
    assert!(to < meters);
    open_stops(&mut h);
    show_stop(&mut h, index);
    let (from, to) = (handle(&h, index), on_bar(&h, to));
    let to = pos2(from.x, to.y);
    let before = band_color(&h);
    touch(&mut h, egui::TouchPhase::Start, from, None);
    h.step();
    for i in 1..=6 {
        let pos = from.lerp(to, i as f32 / 6.0);
        touch(&mut h, egui::TouchPhase::Move, pos, None);
        h.step();
        let shown = center(&mut h);
        assert!(near(shown, band_color(&h)), "frame {i}: {shown:?}");
    }
    touch(&mut h, egui::TouchPhase::End, to, None);
    h.step();
    assert_ne!(band_color(&h), before);
}

#[test]
fn the_panel_of_a_layer_that_is_not_a_height_layer_changes_its_unit() {
    use vale_app::ui::pad::raster::UNIT;
    for size in BOARDS {
        let what = format!("{size:?}");
        let mut h = painted_pad(size);
        let spec = vale_app::ui::RasterSpec {
            name: "Rain".to_string(),
            unit: "mm".to_string(),
            min: 0.0,
            max: 4000.0,
            face_size: 128,
        };
        let rain = h.state_mut().add_raster(spec).unwrap();
        h.run_steps(2);
        open_panel(&mut h, "Layers");
        tap(&mut h, "Open Rain");
        check_controls(&h, &format!("{what} in the panel of Rain"));
        let card = panel(&h, Panel::Layers).unwrap();
        assert!(card.contains_rect(control(&h, UNIT).unwrap()), "{what}");
        tap(&mut h, UNIT);
        h.event(egui::Event::Text("/yr".to_string()));
        h.run_steps(2);
        let unit = &h.state().doc.project.raster(rain).unwrap().unit;
        assert_eq!(unit, "mm/yr", "{what}");
    }
}

/// The line features of a layer.
fn line_features(h: &Pad, id: vale_store::LayerId) -> Vec<vale_store::Feature> {
    h.state().doc.project.layer(id).unwrap().features.clone()
}

/// A pen stroke on the canvas, and the saves of the frames after it.
fn line_stroke(h: &mut Pad, from: Pos2, to: Pos2) {
    drag(h, from, to, Some(0.5));
    for _ in 0..3 {
        step_and_save(h, false);
    }
}

#[test]
fn the_vector_cell_is_the_line_cell_for_a_line_layer() {
    let mut h = pad(BOARDS[0]);
    let draw = draw_cell(&h);
    let rivers = vector(&h, "Rivers");
    h.state_mut().select(Some(LayerRef::Vector(rivers)));
    h.run_steps(2);
    assert_eq!(control(&h, VECTOR), Some(draw));
    assert_eq!(h.state().pad.vector_face, Some(LINE));
    for name in [SCULPT, TERRAIN, "Size", "Flow"] {
        assert!(control(&h, name).is_none(), "{name}");
    }
    // The context strip shows the controls of the line tool only while it
    // is the tool.
    assert!(control(&h, SMOOTHING).is_none());
    assert!(!h.state().globe.lining());
    quick_tap(&mut h, VECTOR);
    assert_eq!(h.state().globe.tool, Tool::Line);
    assert!(h.state().globe.lining());
    assert!(control(&h, SMOOTHING).is_some());
    // A finger turns the globe and draws no line.
    let center = h.state().globe.rect.center();
    let before = line_features(&h, rivers);
    drag(&mut h, center, center + vec2(120.0, 0.0), None);
    assert_eq!(line_features(&h, rivers), before);
    // A layer of points has no line tool.
    let places = vector(&h, "Places");
    h.state_mut().select(Some(LayerRef::Vector(places)));
    h.run_steps(2);
    assert_eq!(h.state().pad.vector_face, Some(vale_app::ui::pad::POINT));
    assert!(!h.state().globe.lining());
}

#[test]
fn a_line_stroke_is_one_feature_that_undo_removes_and_a_stroke_from_its_end_extends_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = saved_pad(BOARDS[0], dir.path());
    let rivers = vector(&h, "Rivers");
    h.state_mut().select(Some(LayerRef::Vector(rivers)));
    h.run_steps(2);
    quick_tap(&mut h, VECTOR);
    let before = line_features(&h, rivers);
    let saved = saves(&h);

    let center = h.state().globe.rect.center();
    let (from, to) = (center + vec2(-150.0, 40.0), center + vec2(150.0, -40.0));
    line_stroke(&mut h, from, to);
    let after = line_features(&h, rivers);
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(after[..before.len()], before[..]);
    let line = after.last().unwrap().clone();
    assert!(before.iter().all(|f| f.uuid() != line.uuid()));
    let vale_store::Geometry::Lines(parts) = &line.geometry else {
        panic!("not a line");
    };
    assert_eq!(parts.len(), 1);
    // The line runs from the start of the stroke to its end.
    let globe = &h.state().globe;
    let ends = [parts[0][0], parts[0][parts[0].len() - 1]];
    let [start, end] = ends.map(|p| {
        let dir = vale_app::globe::math::lonlat_to_dir(p[0], p[1]);
        globe.view.project(globe.rect, dir).unwrap()
    });
    assert!(start.distance(from) < 0.5 && end.distance(to) < 0.5);
    assert_eq!(saves(&h), saved + 1);

    tap(&mut h, "Undo");
    assert_eq!(line_features(&h, rivers), before);
    tap(&mut h, "Redo");
    assert_eq!(line_features(&h, rivers), after);

    // A stroke that starts near the end of the line extends it.
    line_stroke(&mut h, end + vec2(4.0, 3.0), end + vec2(60.0, 80.0));
    let extended = line_features(&h, rivers);
    assert_eq!(extended.len(), after.len());
    let joined = extended.last().unwrap();
    assert_eq!(joined.uuid(), line.uuid());
    let vale_store::Geometry::Lines(joined_parts) = &joined.geometry else {
        panic!("not a line");
    };
    assert_eq!(joined_parts[0][..parts[0].len()], parts[0][..]);
    assert!(joined_parts[0].len() > parts[0].len());
    tap(&mut h, "Undo");
    assert_eq!(line_features(&h, rivers), after);
}

#[test]
fn a_fitted_line_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = empty_pad(BOARDS[0]);
    h.state_mut().globe.set_face_size(256);
    h.state_mut().add_height();
    h.run_steps(2);
    let state = h.state_mut();
    state.session = Some(Session::open(&dir.path().join("w.gpkg"), &mut state.doc).unwrap());
    state.save_waiting().unwrap();
    h.run_steps(2);
    let rivers = vector(&h, "Rivers");
    h.state_mut().select(Some(LayerRef::Vector(rivers)));
    h.run_steps(2);
    quick_tap(&mut h, VECTOR);
    assert!(h.state().globe.fit_layer.is_some());
    tap(&mut h, TERRAIN_FIT);
    assert!(h.state().globe.line.fit);
    let before = line_features(&h, rivers);
    let saved = saves(&h);

    let center = h.state().globe.rect.center();
    line_stroke(
        &mut h,
        center + vec2(-150.0, 40.0),
        center + vec2(150.0, -40.0),
    );
    let after = line_features(&h, rivers);
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(saves(&h), saved + 1);
    tap(&mut h, "Undo");
    assert_eq!(line_features(&h, rivers), before);
    tap(&mut h, "Redo");
    assert_eq!(line_features(&h, rivers), after);
}

#[test]
fn the_terrain_fit_controls_are_touch_size_in_each_layout() {
    let touch = |r: Rect| r.width() >= 44.0 && r.height() >= 44.0;
    for (size, safe) in measure_screens() {
        let what = format!("{size:?}");
        let mut h = safe_pad(size, safe);
        let lines = h.state_mut().doc.add_layer(vale_store::GeometryKind::Line);
        h.state_mut().select(Some(LayerRef::Vector(lines)));
        h.run_steps(2);
        quick_tap(&mut h, VECTOR);
        scroll_column_to_end(&mut h);
        if matches!(h.state().pad.rects.unwrap().brush_card, BrushCard::Full(_)) {
            for name in [TERRAIN_FIT, SMOOTHING] {
                let rect = control(&h, name).unwrap_or_else(|| panic!("{what}: no {name}"));
                assert!(touch(rect), "{what}: {name} is {rect:?}");
            }
            assert!(!disabled(&h, TERRAIN_FIT), "{what}");
            tap(&mut h, TERRAIN_FIT);
            assert!(h.state().globe.line.fit, "{what}");
            tap(&mut h, TERRAIN_FIT);
            assert!(!h.state().globe.line.fit, "{what}");
            check_controls(&h, &what);
        } else {
            assert!(control(&h, SMOOTHING).is_none(), "{what}");
        }
        scroll_strip_to(&mut h, "Brush settings");
        tap(&mut h, "Brush settings");
        // The balance slider is disabled while the fit is off.
        assert!(disabled(&h, TERRAIN_BALANCE), "{what}");
        scroll_brush_to(&mut h, TERRAIN_FIT_SWITCH);
        tap(&mut h, TERRAIN_FIT_SWITCH);
        assert!(h.state().globe.line.fit, "{what}");
        assert!(!disabled(&h, TERRAIN_BALANCE), "{what}");
        for name in [TERRAIN_FIT_SWITCH, TERRAIN_BALANCE] {
            scroll_brush_to(&mut h, name);
            let rect = control(&h, name).unwrap_or_else(|| panic!("{what}: no {name}"));
            assert!(touch(rect), "{what}: {name} is {rect:?}");
        }
        check_controls(&h, &format!("{what} with the brush panel"));
        check_safe(&h, size, safe, &what);
        screenshot::save(&mut h, &format!("pad-terrain-fit-{}", size.0));
    }
}

/// Scrolls the Brush panel until a control is inside it, with a drag up on
/// the margin at the left of the rows.
fn scroll_brush_to(h: &mut Pad, name: &str) {
    for _ in 0..8 {
        let card = panel(h, Panel::Brush).unwrap();
        let inside = |rect: Rect| card.contains_rect(rect);
        if control(h, name).is_some_and(inside) {
            return;
        }
        let from = clear_of_handle(h, pos2(card.left() + 6.0, card.bottom() - 60.0));
        drag(h, from, from - vec2(0.0, 120.0), None);
    }
}

/// `at`, or a place right of the width handle of the dock if the handle
/// takes a drag at `at`.
fn clear_of_handle(h: &Pad, at: Pos2) -> Pos2 {
    match control(h, vale_app::ui::pad::dock_ui::DOCK_WIDTH) {
        Some(handle) if handle.contains(at) => pos2(handle.right() + 6.0, at.y),
        _ => at,
    }
}

#[test]
fn with_no_height_layer_the_terrain_fit_is_disabled() {
    let mut h = empty_pad(BOARDS[0]);
    let rivers = vector(&h, "Rivers");
    h.state_mut().select(Some(LayerRef::Vector(rivers)));
    h.run_steps(2);
    quick_tap(&mut h, VECTOR);
    assert!(h.state().globe.fit_layer.is_none());
    // A disabled control takes no touch.
    assert!(disabled(&h, TERRAIN_FIT) && control(&h, TERRAIN_FIT).is_none());
    tap(&mut h, "Brush settings");
    for name in [TERRAIN_FIT_SWITCH, TERRAIN_BALANCE] {
        assert!(disabled(&h, name) && control(&h, name).is_none(), "{name}");
    }
    // A height layer enables the switch.
    h.state_mut().globe.set_face_size(64);
    h.state_mut().add_height();
    h.state_mut().select(Some(LayerRef::Vector(rivers)));
    h.run_steps(2);
    assert!(!disabled(&h, TERRAIN_FIT));
}

#[test]
fn the_draw_cell_is_the_polygon_cell_for_a_polygon_layer() {
    let mut h = pad(BOARDS[0]);
    let draw = draw_cell(&h);
    let areas = h
        .state_mut()
        .doc
        .add_layer(vale_store::GeometryKind::Polygon);
    h.state_mut().select(Some(LayerRef::Vector(areas)));
    h.run_steps(2);
    assert_eq!(control(&h, VECTOR), Some(draw));
    assert_eq!(h.state().pad.vector_face, Some(POLYGON));
    assert!(control(&h, SMOOTHING).is_none());
    assert!(control(&h, vale_app::ui::pad::POLYGON_MODE).is_none());
    assert!(!h.state().globe.filling());
    quick_tap(&mut h, VECTOR);
    assert_eq!(h.state().globe.tool, Tool::Polygon);
    assert!(h.state().globe.filling());
    assert!(control(&h, SMOOTHING).is_some());
    assert!(control(&h, vale_app::ui::pad::POLYGON_MODE).is_some());
    // In a line layer, the tool draws lines.
    let rivers = vector(&h, "Rivers");
    h.state_mut().select(Some(LayerRef::Vector(rivers)));
    h.run_steps(2);
    assert!(h.state().globe.lining());
    h.state_mut().select(Some(LayerRef::Vector(areas)));
    h.run_steps(2);
    assert!(h.state().globe.filling());
}

#[test]
fn a_polygon_stroke_is_one_feature_that_undo_removes() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = saved_pad(BOARDS[0], dir.path());
    let areas = h
        .state_mut()
        .doc
        .add_layer(vale_store::GeometryKind::Polygon);
    h.state_mut().select(Some(LayerRef::Vector(areas)));
    h.step();
    let now = std::time::Instant::now();
    h.state_mut().save_tick(false, now);
    h.state_mut()
        .save_tick(false, now + std::time::Duration::from_secs(5));
    quick_tap(&mut h, VECTOR);
    h.state_mut().globe.snap.on = false;
    let saved = saves(&h);

    // Three sides of a square. The polygon closes the fourth side.
    let c = h.state().globe.rect.center();
    let corners = [
        vec2(-80.0, -80.0),
        vec2(-80.0, 80.0),
        vec2(80.0, 80.0),
        vec2(80.0, -80.0),
    ];
    pen_path(&mut h, &corners.map(|v| c + v));
    for _ in 0..3 {
        step_and_save(&mut h, false);
    }
    let after = line_features(&h, areas);
    assert_eq!(after.len(), 1);
    let vale_store::Geometry::Polygons(polygons) = &after[0].geometry else {
        panic!("not a polygon");
    };
    assert_eq!(polygons.len(), 1);
    // The fourth side is the short arc between the ends of the stroke, and
    // the middle of the square is inside.
    use vale_app::globe::math::{cross, dot, lonlat_to_dir, normalize};
    let globe = &h.state().globe;
    let dirs: Vec<[f64; 3]> = polygons[0][0]
        .iter()
        .map(|p| lonlat_to_dir(p[0], p[1]))
        .collect();
    let ring: Vec<Pos2> = dirs
        .iter()
        .map(|&d| globe.view.project(globe.rect, d).unwrap())
        .collect();
    let at = |v: egui::Vec2| ring.iter().position(|p| p.distance(c + v) < 0.5);
    let (start, end) = (at(corners[0]).unwrap(), at(corners[3]).unwrap());
    let normal = normalize(cross(dirs[end], dirs[start]));
    let n = ring.len() - 1;
    let mut k = end;
    while k != start {
        assert!(dot(dirs[k], normal).abs() < 1e-9, "{ring:?}");
        k = (k + 1) % n;
    }
    let middle = globe.view.unproject(globe.rect, c).unwrap();
    let rings = [vale_app::globe::vector::ring(&polygons[0][0], false)];
    assert!(vale_app::globe::vector::contains(&rings, middle));
    assert_eq!(saves(&h), saved + 1);

    tap(&mut h, "Undo");
    assert!(line_features(&h, areas).is_empty());
    tap(&mut h, "Redo");
    assert_eq!(line_features(&h, areas), after);
}

#[test]
fn the_style_section_has_the_smoothing_and_the_minimum_feature_size() {
    use vale_app::ui::pad::raster::{FEATURE_SIZE, SMOOTHING};
    for size in BOARDS {
        let what = format!("{size:?}");
        let mut h = painted_pad(size);
        let height = open_height(&mut h);
        let smoothing = |h: &Pad| h.state().doc.project.raster(height).unwrap().smoothing;
        assert!(smoothing(&h).on, "{what}");
        scroll_to(&mut h, FEATURE_SIZE);
        let card = panel(&h, Panel::Layers).unwrap();
        for name in [SMOOTHING, FEATURE_SIZE] {
            let rect = control(&h, name).unwrap_or_else(|| panic!("{what}: no {name}"));
            assert!(card.contains_rect(rect), "{what}: {name} is {rect:?}");
        }
        check_controls(&h, &format!("{what} with the smoothing"));
        screenshot::save(&mut h, &format!("pad-smoothing-{}", size.0));

        set_number(&mut h, FEATURE_SIZE, "0");
        assert_eq!(smoothing(&h).feature_size, 0.0, "{what}");
        scroll_to_control(&mut h, SMOOTHING);

        tap(&mut h, SMOOTHING);
        assert!(!smoothing(&h).on, "{what}");
        assert!(control(&h, FEATURE_SIZE).is_none(), "{what}");
        tap(&mut h, SMOOTHING);
        assert!(smoothing(&h).on, "{what}");
    }
}

#[test]
fn the_style_section_has_the_switch_of_the_band_lines() {
    use vale_app::ui::pad::raster::BAND_LINES;
    for size in BOARDS {
        let what = format!("{size:?}");
        let mut h = painted_pad(size);
        let height = open_height(&mut h);
        let lines = |h: &Pad| h.state().doc.project.raster(height).unwrap().lines;
        assert!(lines(&h), "{what}");
        scroll_to(&mut h, BAND_LINES);
        let card = panel(&h, Panel::Layers).unwrap();
        let rect = control(&h, BAND_LINES).unwrap_or_else(|| panic!("{what}: no switch"));
        assert!(card.contains_rect(rect), "{what}: the switch is {rect:?}");
        check_controls(&h, &format!("{what} with the band lines"));
        tap(&mut h, BAND_LINES);
        assert!(!lines(&h), "{what}");
        tap(&mut h, BAND_LINES);
        assert!(lines(&h), "{what}");
    }
}

/// The controls of the page of the hillshade, from the top.
const HILLSHADE_PAGE: [&str; 7] = [
    raster::HILLSHADE_ON,
    raster::HILLSHADE_OPACITY,
    raster::HILLSHADE_HIGHLIGHTS,
    raster::HILLSHADE_AZIMUTH,
    raster::HILLSHADE_ALTITUDE,
    raster::HILLSHADE_RELIEF,
    raster::HILLSHADE_COLOR,
];

/// A pad of `size` with the page of the hillshade of the height layer open
/// and the hillshade on.
fn hillshade_page(size: (f32, f32)) -> (Pad, RasterId) {
    let mut h = painted_pad(size);
    let height = open_height(&mut h);
    scroll_to(&mut h, raster::HILLSHADE);
    tap(&mut h, raster::HILLSHADE);
    tap(&mut h, raster::HILLSHADE_ON);
    (h, height)
}

fn hillshade_of(h: &Pad, id: RasterId) -> Hillshade {
    h.state().doc.project.raster(id).unwrap().hillshade
}

#[test]
fn the_hillshade_row_of_a_height_layer_opens_its_page() {
    use raster::{COLOR_STOPS, HILLSHADE};
    for size in BOARDS {
        let what = format!("{size:?}");
        let mut h = painted_pad(size);
        open_height(&mut h);
        scroll_to(&mut h, HILLSHADE);
        let card = panel(&h, Panel::Layers).unwrap();
        let rect = control(&h, HILLSHADE).unwrap_or_else(|| panic!("{what}: no row"));
        assert!(card.contains_rect(rect), "{what}: the row is {rect:?}");
        for name in HILLSHADE_PAGE {
            assert!(control(&h, name).is_none(), "{what}: {name}");
        }
        tap(&mut h, HILLSHADE);
        assert_eq!(h.state().pad.page, Some(Page::Hillshade), "{what}");
        tap(&mut h, "Back to Height");
        assert_eq!(h.state().pad.page, None, "{what}");

        // A layer that is not a height layer has no hillshade.
        let spec = vale_app::ui::RasterSpec {
            name: "Rain".to_string(),
            unit: "mm".to_string(),
            min: 0.0,
            max: 4000.0,
            face_size: 128,
        };
        let rain = h.state_mut().add_raster(spec).unwrap();
        h.state_mut().pad.layer = Some(LayerRef::Raster(rain));
        h.run_steps(2);
        scroll_to(&mut h, COLOR_STOPS);
        assert!(control(&h, COLOR_STOPS).is_some(), "{what}");
        assert!(control(&h, HILLSHADE).is_none(), "{what}");
    }
}

#[test]
fn the_hillshade_page_has_each_control_inside_the_card() {
    use raster::{HILLSHADE, HILLSHADE_HIGHLIGHTS, HILLSHADE_ON, HILLSHADE_OPACITY};
    for size in BOARDS {
        let what = format!("{size:?}");
        let mut h = painted_pad(size);
        let height = open_height(&mut h);
        scroll_to(&mut h, HILLSHADE);
        tap(&mut h, HILLSHADE);
        assert!(!hillshade_of(&h, height).on, "{what}");
        for name in &HILLSHADE_PAGE[1..] {
            assert!(control(&h, name).is_none(), "{what}: {name}");
        }
        tap(&mut h, HILLSHADE_ON);
        assert!(hillshade_of(&h, height).on, "{what}");
        for name in HILLSHADE_PAGE {
            scroll_to(&mut h, name);
            let card = panel(&h, Panel::Layers).unwrap();
            let rect = control(&h, name).unwrap_or_else(|| panic!("{what}: no {name}"));
            assert!(card.contains_rect(rect), "{what}: {name} is {rect:?}");
            check_controls(&h, &format!("{what} at {name}"));
        }
        screenshot::save(&mut h, &format!("pad-hillshade-{}", size.0));

        // The opacity is in whole percents.
        set_number(&mut h, HILLSHADE_OPACITY, "0");
        assert_eq!(hillshade_of(&h, height).opacity, 0.0, "{what}");
        set_number(&mut h, HILLSHADE_OPACITY, "40.4");
        assert_eq!(hillshade_of(&h, height).opacity, 0.4, "{what}");
        let opacity = hillshade_of(&h, height).opacity;
        assert!(opacity > 0.0, "{what}: {opacity}");

        // The highlights start on, and a tap turns them off.
        assert!(hillshade_of(&h, height).highlights, "{what}");
        scroll_to_row(&mut h, HILLSHADE_HIGHLIGHTS);
        tap(&mut h, HILLSHADE_HIGHLIGHTS);
        assert!(!hillshade_of(&h, height).highlights, "{what}");

        // The switch hides the rows, and the layer keeps their values.
        scroll_to_row(&mut h, HILLSHADE_ON);
        tap(&mut h, HILLSHADE_ON);
        let off = Hillshade {
            on: false,
            opacity,
            highlights: false,
            ..Hillshade::default()
        };
        assert_eq!(hillshade_of(&h, height), off, "{what}");
        for name in &HILLSHADE_PAGE[1..] {
            assert!(control(&h, name).is_none(), "{what}: {name}");
        }
    }
}

#[test]
fn a_tap_on_the_compass_dial_sets_the_light_direction() {
    use raster::HILLSHADE_AZIMUTH;
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let (mut h, height) = hillshade_page(size);
        scroll_to_row(&mut h, HILLSHADE_AZIMUTH);
        let r = control(&h, HILLSHADE_AZIMUTH).unwrap();
        let center = r.center();
        let azimuth = |h: &Pad| hillshade_of(h, height).azimuth;
        assert_eq!(azimuth(&h), 315.0, "{what}");
        finger_tap(&mut h, pos2(r.right() - 4.0, center.y));
        assert_eq!(azimuth(&h), 90.0, "{what}");
        finger_tap(&mut h, pos2(center.x, r.top() + 4.0));
        assert_eq!(azimuth(&h), 0.0, "{what}");
        drag(
            &mut h,
            pos2(r.right() - 4.0, center.y),
            pos2(center.x, r.bottom() - 4.0),
            None,
        );
        assert_eq!(azimuth(&h), 180.0, "{what}");
        // A tap near a compass point snaps to it.
        let a = 47.0_f32.to_radians();
        finger_tap(&mut h, center + 60.0 * vec2(a.sin(), -a.cos()));
        assert_eq!(azimuth(&h), 45.0, "{what}");
    }
}

#[test]
fn the_hillshade_sliders_set_the_light_height_and_the_relief() {
    use raster::{HILLSHADE_ALTITUDE, HILLSHADE_RELIEF};
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let (mut h, height) = hillshade_page(size);
        // The light height is in whole degrees from 5° to 85°.
        set_number(&mut h, HILLSHADE_ALTITUDE, "0");
        assert_eq!(hillshade_of(&h, height).altitude, 5.0, "{what}");
        set_number(&mut h, HILLSHADE_ALTITUDE, "90");
        assert_eq!(hillshade_of(&h, height).altitude, 85.0, "{what}");
        set_number(&mut h, HILLSHADE_ALTITUDE, "30.4");
        assert_eq!(hillshade_of(&h, height).altitude, 30.0, "{what}");

        // The relief is in hundredths from 0.25 to 4, and a drag from the
        // left end stops at 1.
        set_number(&mut h, HILLSHADE_RELIEF, "0");
        assert_eq!(hillshade_of(&h, height).exaggeration, 0.25, "{what}");
        set_number(&mut h, HILLSHADE_RELIEF, "9");
        assert_eq!(hillshade_of(&h, height).exaggeration, 4.0, "{what}");
        set_number(&mut h, HILLSHADE_RELIEF, "1.234");
        assert_eq!(hillshade_of(&h, height).exaggeration, 1.23, "{what}");
    }
}

#[test]
fn the_shade_color_row_opens_the_color_picker() {
    use raster::HILLSHADE_COLOR;
    let (hue, shade) = ("Shade color hue", "Shade color shade");
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let (mut h, height) = hillshade_page(size);
        scroll_to(&mut h, HILLSHADE_COLOR);
        assert!(control(&h, hue).is_none(), "{what}");
        tap(&mut h, HILLSHADE_COLOR);
        scroll_to(&mut h, hue);
        assert!(control(&h, hue).is_some(), "{what}");
        let square = control(&h, shade).unwrap_or_else(|| panic!("{what}: no square"));
        finger_tap(&mut h, square.right_top() + vec2(-4.0, 4.0));
        let color = hillshade_of(&h, height).color;
        assert_ne!(color, [0, 0, 0], "{what}");
        scroll_to_row(&mut h, HILLSHADE_COLOR);
        tap(&mut h, HILLSHADE_COLOR);
        assert!(control(&h, hue).is_none(), "{what}");
        assert_eq!(hillshade_of(&h, height).color, color, "{what}");
    }
}

/// A renderer of a gradient: the pixel at (x, y) is (x, y, 128) for x and y
/// below 256. It answers the screenshot of the eyedropper with no GPU.
struct Gradient;

impl egui_kittest::TestRenderer for Gradient {
    fn handle_delta(&mut self, delta: &mut egui::TexturesDelta) {
        delta.clear();
    }

    fn render(
        &mut self,
        ctx: &egui::Context,
        _: &egui::FullOutput,
    ) -> Result<image::RgbaImage, String> {
        let size = ctx.viewport_rect().size() * ctx.pixels_per_point();
        let pixel = |x: u32, y: u32| image::Rgba([x.min(255) as u8, y.min(255) as u8, 128, 255]);
        Ok(image::RgbaImage::from_fn(
            size.x as u32,
            size.y as u32,
            pixel,
        ))
    }
}

/// The hillshade page of `hillshade_page`, with the color picker of the
/// hillshade open, in a harness that renders a gradient.
fn eyedropper_page() -> (Pad, RasterId) {
    let (width, height) = BOARDS[0];
    let mut h = Harness::builder()
        .with_size(vec2(width, height))
        .with_pixels_per_point(1.0)
        .renderer(Gradient)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state());
    h.run_steps(3);
    h.state_mut().add_height();
    h.run_steps(2);
    h.state_mut().globe.map.set(0, 0, 0, meters_to_level(100.0));
    h.run_steps(2);
    let id = open_height(&mut h);
    scroll_to(&mut h, raster::HILLSHADE);
    tap(&mut h, raster::HILLSHADE);
    tap(&mut h, raster::HILLSHADE_ON);
    scroll_to(&mut h, raster::HILLSHADE_COLOR);
    tap(&mut h, raster::HILLSHADE_COLOR);
    scroll_to(&mut h, "Shade color hue");
    (h, id)
}

const SHADE_EYEDROPPER: &str = "Shade color eyedropper";

#[test]
fn the_eyedropper_picks_the_color_of_the_screen_under_the_finger() {
    let (mut h, id) = eyedropper_page();
    assert!(control(&h, eyedropper::CANCEL).is_none());
    tap(&mut h, SHADE_EYEDROPPER);
    assert!(control(&h, eyedropper::CANCEL).is_some());

    // The pixel at (x, y) of the gradient is (x, y, 128), up to 255.
    let gradient = |at: Pos2| [at.x.min(255.0) as u8, at.y.min(255.0) as u8, 128];
    let from = h.state().globe.rect.center();
    let at = pos2(200.0, 230.0);
    let view = h.state().globe.view;
    drag(&mut h, from, at, None);
    assert_eq!(hillshade_of(&h, id).color, gradient(at));
    assert_eq!(h.state().globe.view, view);
    assert!(control(&h, eyedropper::CANCEL).is_none());
    assert!(control(&h, "Shade color hue").is_some());
}

#[test]
fn a_touch_does_not_go_to_a_control_while_the_eyedropper_is_on() {
    let (mut h, id) = eyedropper_page();
    // A tap on the color row closes the picker.
    let row = control(&h, raster::HILLSHADE_COLOR).unwrap();
    tap(&mut h, SHADE_EYEDROPPER);
    finger_tap(&mut h, row.center());
    assert!(control(&h, "Shade color hue").is_some());
    let at = row.center();
    let picked = [at.x.min(255.0) as u8, at.y.min(255.0) as u8, 128];
    assert_eq!(hillshade_of(&h, id).color, picked);
}

#[test]
#[ignore = "gpu"]
fn the_eyedropper_picks_a_pixel_of_the_globe() {
    let mut h = gpu_pad(BOARDS[0]);
    let id = open_height(&mut h);
    scroll_to(&mut h, raster::HILLSHADE);
    tap(&mut h, raster::HILLSHADE);
    tap(&mut h, raster::HILLSHADE_ON);
    scroll_to(&mut h, raster::HILLSHADE_COLOR);
    tap(&mut h, raster::HILLSHADE_COLOR);
    scroll_to(&mut h, "Shade color hue");
    settle(&mut h);
    let at = h.state().globe.rect.center() - vec2(60.0, 40.0);
    let pixel = h.render().unwrap().get_pixel(at.x as u32, at.y as u32).0;

    tap(&mut h, SHADE_EYEDROPPER);
    touch(&mut h, egui::TouchPhase::Start, at, None);
    h.run_steps(2);
    screenshot::save(&mut h, "pad-eyedropper");
    touch(&mut h, egui::TouchPhase::End, at, None);
    h.run_steps(3);
    assert_eq!(hillshade_of(&h, id).color, [pixel[0], pixel[1], pixel[2]]);
}

#[test]
fn cancel_ends_the_eyedropper_and_keeps_the_color() {
    let (mut h, id) = eyedropper_page();
    let color = hillshade_of(&h, id).color;
    tap(&mut h, SHADE_EYEDROPPER);
    tap(&mut h, eyedropper::CANCEL);
    assert!(control(&h, eyedropper::CANCEL).is_none());
    assert_eq!(hillshade_of(&h, id).color, color);
    // The next touch goes to the picker again.
    tap(&mut h, SHADE_EYEDROPPER);
    assert!(control(&h, eyedropper::CANCEL).is_some());
}

/// Scrolls the panel of a layer until a control is inside the card, with a
/// drag up on the margin at the left of the rows.
fn scroll_to(h: &mut Pad, name: &str) {
    for _ in 0..8 {
        let card = panel(h, Panel::Layers).unwrap();
        let inside = |rect: Rect| card.shrink2(vec2(0.0, 40.0)).contains_rect(rect);
        if control(h, name).is_some_and(inside) {
            return;
        }
        let from = pos2(card.left() + 6.0, card.bottom() - 60.0);
        drag(h, from, from - vec2(0.0, 200.0), None);
    }
}

/// A click of the mouse on the middle of a control.
fn click(h: &mut Pad, name: &str) {
    let rect = control(h, name).unwrap_or_else(|| panic!("no control `{name}`"));
    let pos = rect.center();
    h.event(egui::Event::PointerMoved(pos));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(2);
}

/// A tap of the pen on the middle of a control.
fn pen_tap(h: &mut Pad, name: &str) {
    let rect = control(h, name).unwrap_or_else(|| panic!("no control `{name}`"));
    let pos = rect.center();
    touch(h, egui::TouchPhase::Start, pos, Some(0.5));
    h.step();
    touch(h, egui::TouchPhase::End, pos, Some(0.5));
    h.run_steps(3);
}

/// The layout with the line tool in a new line layer. The rivers stay
/// visible.
fn line_pad(size: (f32, f32)) -> (Pad, vale_store::LayerId) {
    let mut h = pad(size);
    let lines = h.state_mut().doc.add_layer(vale_store::GeometryKind::Line);
    h.state_mut().select(Some(LayerRef::Vector(lines)));
    h.run_steps(2);
    quick_tap(&mut h, VECTOR);
    assert!(h.state().globe.lining());
    (h, lines)
}

#[test]
fn the_snap_controls_work_with_a_pen_a_finger_and_a_mouse() {
    for size in BOARDS {
        let what = format!("{size:?}");
        let (mut h, _) = line_pad(size);
        if matches!(h.state().pad.rects.unwrap().brush_card, BrushCard::Full(_)) {
            assert!(h.state().globe.snap.on, "{what}");
            tap(&mut h, SNAP);
            assert!(!h.state().globe.snap.on, "{what}");
            pen_tap(&mut h, SNAP);
            assert!(h.state().globe.snap.on, "{what}");
            click(&mut h, SNAP);
            assert!(!h.state().globe.snap.on, "{what}");
            tap(&mut h, SNAP);
        }
        tap(&mut h, "Brush settings");
        check_controls(&h, &format!("{what} with the snap controls"));
        screenshot::save(&mut h, &format!("pad-snap-{}", size.0));
        let snap = |h: &Pad| {
            let s = &h.state().globe.snap;
            [s.on, s.vertices, s.edges]
        };
        for (i, name) in [SNAPPING, SNAP_VERTICES, SNAP_EDGES]
            .into_iter()
            .enumerate()
        {
            assert!(snap(&h)[i], "{what}: {name}");
            tap(&mut h, name);
            assert!(!snap(&h)[i], "{what}: {name} after a finger");
            pen_tap(&mut h, name);
            assert!(snap(&h)[i], "{what}: {name} after a pen");
            click(&mut h, name);
            assert!(!snap(&h)[i], "{what}: {name} after a mouse");
            tap(&mut h, name);
        }
        slide_to_end(&mut h, SNAP_DISTANCE, false);
        assert_eq!(h.state().globe.snap.distance, 4.0, "{what}");
        slide_to_end(&mut h, SNAP_DISTANCE, true);
        assert_eq!(h.state().globe.snap.distance, 32.0, "{what}");
    }
}

#[test]
fn the_snap_sheet_has_a_switch_for_the_stops_of_each_raster_layer() {
    for size in BOARDS {
        let what = format!("{size:?}");
        let mut h = pad(size);
        let rain = vale_app::ui::RasterSpec {
            name: "Rain".to_string(),
            unit: "mm".to_string(),
            min: 0.0,
            max: 4000.0,
            face_size: 256,
        };
        h.state_mut().add_raster(rain).unwrap();
        let lines = h.state_mut().doc.add_layer(vale_store::GeometryKind::Line);
        h.state_mut().select(Some(LayerRef::Vector(lines)));
        h.run_steps(2);
        quick_tap(&mut h, VECTOR);
        assert!(h.state().globe.lining(), "{what}");
        tap(&mut h, "Brush settings");
        let rasters = panels::raster_layers(h.state());
        assert_eq!(rasters.len(), 2, "{what}");
        for (id, name) in rasters {
            let name = panels::snap_stops_name(&name);
            let on = |h: &Pad| h.state().globe.snap.stops(id);
            assert!(on(&h), "{what}: {name}");
            tap(&mut h, &name);
            assert!(!on(&h), "{what}: {name} after a finger");
            pen_tap(&mut h, &name);
            assert!(on(&h), "{what}: {name} after a pen");
            click(&mut h, &name);
            assert!(!on(&h), "{what}: {name} after a mouse");
            tap(&mut h, &name);
            assert!(on(&h), "{what}: {name}");
        }
        check_controls(&h, &format!("{what} with the switches of the stops"));
        screenshot::save(&mut h, &format!("pad-snap-stops-{}", size.0));
    }
}

/// A pen stroke through screen points, with a sample each 3 points.
fn pen_path(h: &mut Pad, points: &[Pos2]) {
    touch(h, egui::TouchPhase::Start, points[0], Some(0.5));
    h.step();
    for pair in points.windows(2) {
        let n = (pair[0].distance(pair[1]) / 3.0).ceil().max(1.0) as usize;
        for i in 1..=n {
            let pos = pair[0].lerp(pair[1], i as f32 / n as f32);
            touch(h, egui::TouchPhase::Move, pos, Some(0.5));
            h.step();
        }
    }
    touch(
        h,
        egui::TouchPhase::End,
        points[points.len() - 1],
        Some(0.5),
    );
    h.run_steps(3);
}

/// Draws along the longest river, a few points to one side. Returns the
/// river vertices that the stroke goes along, and the new line.
fn stroke_along_a_river(snap: bool) -> (Vec<[f64; 2]>, Vec<[f64; 2]>) {
    let (mut h, lines) = line_pad(BOARDS[0]);
    h.state_mut().globe.snap.on = snap;
    let rivers = vector(&h, "Rivers");
    let longest = line_features(&h, rivers)
        .into_iter()
        .filter_map(|f| match f.geometry {
            vale_store::Geometry::Lines(parts) => parts.into_iter().next(),
            _ => None,
        })
        .max_by_key(|part| part.len())
        .unwrap();
    let mid = longest.len() / 2;
    let way = longest[mid - 3..=mid + 3].to_vec();
    let globe = &mut h.state_mut().globe;
    globe.view = GlobeView::centered(way[3][0], way[3][1]);
    let dir = |p: [f64; 2]| vale_app::globe::math::lonlat_to_dir(p[0], p[1]);
    let span = |g: &vale_app::globe::Globe| {
        let a = g.view.project(g.rect, dir(way[0])).unwrap();
        let b = g.view.project(g.rect, dir(way[6])).unwrap();
        a.distance(b)
    };
    globe.view.zoom = (300.0 / f64::from(span(globe))).min(40.0);
    h.run_steps(2);
    let globe = &h.state().globe;
    let points: Vec<Pos2> = way
        .iter()
        .map(|&p| globe.view.project(globe.rect, dir(p)).unwrap() + vec2(0.0, 3.0))
        .collect();
    let before = line_features(&h, lines).len();
    pen_path(&mut h, &points);
    let after = line_features(&h, lines);
    assert_eq!(after.len(), before + 1);
    let vale_store::Geometry::Lines(parts) = &after[before].geometry else {
        panic!("not a line");
    };
    (way, parts.concat())
}

#[test]
fn a_pen_stroke_along_a_river_traces_it() {
    let (way, line) = stroke_along_a_river(true);
    for p in &way[1..way.len() - 1] {
        assert!(line.contains(p), "{p:?} is not in {line:?}");
    }
}

#[test]
fn with_snapping_off_a_stroke_along_a_river_does_not_trace_it() {
    let (way, line) = stroke_along_a_river(false);
    for p in &way {
        assert!(!line.contains(p), "{p:?} is in the line");
    }
}

/// A tap of the pen on the canvas.
fn pen_tap_at(h: &mut Pad, pos: Pos2) {
    touch(h, egui::TouchPhase::Start, pos, Some(0.5));
    h.step();
    touch(h, egui::TouchPhase::End, pos, Some(0.5));
    h.run_steps(3);
}

/// Adds a point layer and a saved project, and selects the point tool with
/// no snapping.
fn new_point_layer(h: &mut Pad) -> vale_store::LayerId {
    let id = h.state_mut().doc.add_layer(vale_store::GeometryKind::Point);
    h.state_mut().select(Some(LayerRef::Vector(id)));
    h.step();
    let now = std::time::Instant::now();
    h.state_mut().save_tick(false, now);
    h.state_mut()
        .save_tick(false, now + std::time::Duration::from_secs(5));
    quick_tap(h, VECTOR);
    h.state_mut().globe.snap.on = false;
    id
}

/// The names of the fields of the inspector.
fn form_fields(h: &Pad) -> Vec<String> {
    let form = h.state().selection_form.as_ref().expect("no form");
    form.fields.iter().map(|f| f.name.clone()).collect()
}

/// Types a text and runs the frames of the edit.
fn type_in_focus(h: &mut Pad, text: &str) {
    h.event(egui::Event::Text(text.into()));
    h.run_steps(2);
}

/// Presses Return and runs the frames of the edit.
fn press_return(h: &mut Pad) {
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
}

#[test]
fn with_the_inspector_open_a_new_point_is_the_selection_with_no_keyboard() {
    let mut h = pad(BOARDS[0]);
    let draw = draw_cell(&h);
    let places = vector(&h, "Places");
    h.state_mut().select(Some(LayerRef::Vector(places)));
    h.run_steps(2);
    assert_eq!(control(&h, VECTOR), Some(draw));
    assert_eq!(h.state().pad.vector_face, Some(vale_app::ui::pad::POINT));
    quick_tap(&mut h, VECTOR);
    assert_eq!(h.state().globe.tool, Tool::Point);
    open_panel(&mut h, select::ATTRIBUTES);
    let before = line_features(&h, places);
    let center = h.state().globe.rect.center();
    // A finger turns the globe and places no point.
    drag(&mut h, center, center + vec2(60.0, 0.0), None);
    assert_eq!(line_features(&h, places), before);
    assert!(h.state().new_feature.is_none());

    pen_tap_at(&mut h, center);
    let after = line_features(&h, places);
    assert_eq!(after.len(), before.len() + 1);
    let new = after.last().unwrap().uuid();
    assert_eq!(
        selection(&h),
        [FeatureRef {
            layer: places,
            uuid: new
        }]
    );
    let fields = h.state().doc.project.layer(places).unwrap().field_names();
    assert!(!fields.is_empty());
    assert_eq!(form_fields(&h), fields);
    assert_eq!(
        h.state().selection_form.as_ref().unwrap().features[0].uuid,
        new
    );
    // No card opens over the canvas. The inspector is the only panel.
    let panels: Vec<Panel> = h.state().pad.panels.iter().map(|(p, _)| *p).collect();
    assert_eq!(panels, [Panel::Attributes]);
    // The inspector does not ask for a value.
    assert!(no_keyboard(&h));
    tap(&mut h, &fields[0]);
    type_in_focus(&mut h, "Avalon");
    let form = h.state().selection_form.as_ref().unwrap();
    assert!(form.fields[0].text.ends_with("Avalon"));
    // Another tool ends the edit.
    quick_tap(&mut h, "Pan");
    assert!(h.state().new_feature.is_none());
    let feature = line_features(&h, places).pop().unwrap();
    let value = feature.attributes[&fields[0]].label().unwrap_or_default();
    assert!(value.ends_with("Avalon"));
}

#[test]
fn a_new_point_layer_has_a_name_field_and_return_ends_the_edit() {
    let mut h = pad(BOARDS[0]);
    let id = new_point_layer(&mut h);
    let layer = h.state().doc.project.layer(id).unwrap();
    assert_eq!(layer.field_names(), ["name"]);
    assert_eq!(layer.fields[0].kind, vale_store::FieldType::Text);
    // The layer labels its points with the name.
    let entry = h.state().doc.frame.entries.iter().find(|e| e.layer == id);
    let label = &entry.unwrap().style.label;
    assert_eq!(label.text, "name");
    assert!(label.enabled);
    open_panel(&mut h, select::ATTRIBUTES);
    let center = h.state().globe.rect.center();
    pen_tap_at(&mut h, center);
    assert_eq!(form_fields(&h), ["name"]);
    tap(&mut h, "name");
    type_in_focus(&mut h, "Avalon");
    press_return(&mut h);
    assert!(h.state().new_feature.is_none());
    assert!(no_keyboard(&h));
    let layer = h.state().doc.project.layer(id).unwrap();
    let name = &layer.features[0].attributes["name"];
    assert_eq!(*name, vale_store::Value::Text("Avalon".into()));
    assert_eq!(selection(&h).len(), 1);
}

#[test]
fn with_the_inspector_closed_a_new_feature_gets_the_defaults_and_nothing_opens() {
    use vale_store::Value;
    let dir = tempfile::tempdir().unwrap();
    let mut h = saved_pad(BOARDS[0], dir.path());
    let id = new_point_layer(&mut h);
    let town = Value::Text("Town".into());
    assert!(
        h.state_mut()
            .doc
            .project
            .set_field_default(id, "name", town.clone())
    );
    for _ in 0..3 {
        step_and_save(&mut h, false);
    }
    let saved = saves(&h);
    let center = h.state().globe.rect.center();
    pen_tap_at(&mut h, center);
    assert!(h.state().new_feature.is_none());
    assert!(h.state().pad.panels.is_empty());
    assert!(no_keyboard(&h));
    assert_eq!(line_features(&h, id)[0].attributes["name"], town);
    assert_eq!(saves(&h), saved + 1);

    // A new line also gets the default values.
    let rivers = vector(&h, "Rivers");
    select_layer(&mut h, rivers);
    quick_tap(&mut h, VECTOR);
    let field = vale_store::Field {
        name: "kind".into(),
        kind: vale_store::FieldType::Text,
        default: Value::Text("River".into()),
    };
    assert!(h.state_mut().doc.project.add_field(rivers, field));
    let before = line_features(&h, rivers).len();
    line_stroke(
        &mut h,
        center + vec2(-150.0, 40.0),
        center + vec2(150.0, -40.0),
    );
    let lines = line_features(&h, rivers);
    assert_eq!(lines.len(), before + 1);
    let kind = &lines.last().unwrap().attributes["kind"];
    assert_eq!(*kind, Value::Text("River".into()));
    assert!(h.state().new_feature.is_none());
    assert!(h.state().pad.panels.is_empty());
    assert!(no_keyboard(&h));
}

#[test]
fn a_point_layer_with_no_fields_places_a_point_in_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = saved_pad(BOARDS[0], dir.path());
    let id = new_point_layer(&mut h);
    h.state_mut().doc.project.remove_field(id, "name");
    open_panel(&mut h, select::ATTRIBUTES);
    for _ in 0..3 {
        step_and_save(&mut h, false);
    }
    let saved = saves(&h);
    let center = h.state().globe.rect.center();
    pen_tap_at(&mut h, center);
    assert!(h.state().new_feature.is_none());
    assert!(h.query_by_label(select::NO_FIELDS).is_some());
    assert_eq!(line_features(&h, id).len(), 1);
    assert!(line_features(&h, id)[0].attributes.is_empty());
    assert_eq!(saves(&h), saved + 1);
    tap(&mut h, "Undo");
    assert!(line_features(&h, id).is_empty());
}

#[test]
fn the_inspector_shows_each_field_of_a_new_point_with_its_default() {
    use vale_store::{Field, FieldType, Value};
    let mut h = pad(BOARDS[0]);
    let id = new_point_layer(&mut h);
    let fields = [
        ("town", FieldType::Text, Value::Text("Hamlet".into())),
        ("people", FieldType::Integer, Value::Number(7.0)),
        ("height", FieldType::Real, Value::Number(2.5)),
        ("port", FieldType::Bool, Value::Bool(false)),
    ];
    let project = &mut h.state_mut().doc.project;
    project.remove_field(id, "name");
    for (name, kind, default) in fields.clone() {
        let field = Field {
            name: name.into(),
            kind,
            default,
        };
        assert!(project.add_field(id, field));
    }
    open_panel(&mut h, select::ATTRIBUTES);
    let center = h.state().globe.rect.center();
    pen_tap_at(&mut h, center);
    assert_eq!(form_fields(&h), ["town", "people", "height", "port"]);
    let form = h.state().selection_form.as_ref().unwrap();
    let texts: Vec<&str> = form.fields.iter().map(|f| f.text.as_str()).collect();
    assert_eq!(texts, ["Hamlet", "7", "2.5", "false"]);
    tap(&mut h, "town");
    // Return goes from field to field. The whole number and the true or
    // false field get new values.
    let typed = [None, Some((1, "12")), None, Some((5, "true"))];
    for edit in typed {
        assert!(h.state().new_feature.is_some());
        if let Some((old, new)) = edit {
            for _ in 0..old {
                h.key_press(egui::Key::Backspace);
            }
            type_in_focus(&mut h, new);
        }
        press_return(&mut h);
    }
    assert!(h.state().new_feature.is_none());
    let attributes = &line_features(&h, id)[0].attributes;
    assert_eq!(attributes["town"], Value::Text("Hamlet".into()));
    assert_eq!(attributes["people"], Value::Number(12.0));
    assert_eq!(attributes["height"], Value::Number(2.5));
    assert_eq!(attributes["port"], Value::Bool(true));
}

#[test]
fn a_text_that_is_not_a_value_of_the_field_type_does_not_go_in_the_new_point() {
    use vale_store::{Field, FieldType, Value};
    let mut h = pad(BOARDS[0]);
    let id = new_point_layer(&mut h);
    let field = Field::new("people", FieldType::Integer);
    assert!(h.state_mut().doc.project.add_field(id, field));
    open_panel(&mut h, select::ATTRIBUTES);
    let center = h.state().globe.rect.center();
    pen_tap_at(&mut h, center);
    assert_eq!(form_fields(&h), ["name", "people"]);
    tap(&mut h, "people");
    type_in_focus(&mut h, "many");
    press_return(&mut h);
    assert!(h.state().new_feature.is_none());
    let attributes = &line_features(&h, id)[0].attributes;
    assert_eq!(attributes["name"], Value::Null);
    assert_eq!(attributes["people"], Value::Null);
}

#[test]
fn a_name_does_not_give_the_focus_to_the_next_new_point() {
    let mut h = pad(BOARDS[0]);
    let id = new_point_layer(&mut h);
    open_panel(&mut h, select::ATTRIBUTES);
    assert!(!h.state().tools.focus_new);
    let center = h.state().globe.rect.center();
    pen_tap_at(&mut h, center);
    tap(&mut h, "name");
    type_in_focus(&mut h, "Avalon");
    pen_tap_at(&mut h, center + vec2(40.0, 0.0));
    assert_eq!(line_features(&h, id).len(), 2);
    assert!(no_keyboard(&h));
    press_return(&mut h);
    let name = &line_features(&h, id)[0].attributes["name"];
    assert_eq!(*name, vale_store::Value::Text("Avalon".into()));
}

#[test]
fn with_focus_new_features_ten_points_take_ten_taps_and_ten_names_in_ten_undo_steps() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = saved_pad(BOARDS[0], dir.path());
    let id = new_point_layer(&mut h);
    open_panel(&mut h, select::ATTRIBUTES);
    // The toggle is in the title row of the inspector.
    let toggle = control(&h, select::FOCUS_NEW).expect("no toggle");
    let inspector = panel(&h, Panel::Attributes).unwrap();
    assert!(inspector.contains_rect(toggle));
    tap(&mut h, select::FOCUS_NEW);
    assert!(h.state().tools.focus_new);
    for _ in 0..3 {
        step_and_save(&mut h, false);
    }
    let saved = saves(&h);
    let center = h.state().globe.rect.center();
    for i in 0..10 {
        // The pen tap is the only touch: the name field has the focus.
        pen_tap_at(&mut h, center + vec2(30.0 * i as f32 - 135.0, 0.0));
        assert!(!no_keyboard(&h), "{i}");
        type_in_focus(&mut h, &format!("City {i}"));
        // Return ends the edit, and so does the tap of the next point.
        if i % 2 == 0 || i == 9 {
            press_return(&mut h);
        }
        for _ in 0..3 {
            step_and_save(&mut h, false);
        }
    }
    assert!(h.state().new_feature.is_none());
    let names: Vec<_> = line_features(&h, id)
        .iter()
        .map(|f| f.attributes["name"].clone())
        .collect();
    let want: Vec<_> = (0..10)
        .map(|i| vale_store::Value::Text(format!("City {i}")))
        .collect();
    assert_eq!(names, want);
    assert_eq!(saves(&h), saved + 10);
    // The point and its name are one undo step.
    tap(&mut h, "Undo");
    let features = line_features(&h, id);
    assert_eq!(features.len(), 9);
    let last = &features[8].attributes["name"];
    assert_eq!(*last, vale_store::Value::Text("City 8".into()));
    // A second tap turns the toggle off.
    tap(&mut h, select::FOCUS_NEW);
    assert!(!h.state().tools.focus_new);
    pen_tap_at(&mut h, center + vec2(0.0, 60.0));
    assert!(no_keyboard(&h));
}

#[test]
fn with_focus_new_features_a_new_point_takes_the_focus_in_the_field_edited_last() {
    use vale_store::{Field, FieldType, Value};
    let mut h = pad(BOARDS[0]);
    let id = new_point_layer(&mut h);
    let field = Field::new("people", FieldType::Integer);
    assert!(h.state_mut().doc.project.add_field(id, field));
    open_panel(&mut h, select::ATTRIBUTES);
    tap(&mut h, select::FOCUS_NEW);
    let center = h.state().globe.rect.center();
    // The first new point takes the focus in the first field.
    pen_tap_at(&mut h, center);
    type_in_focus(&mut h, "Avalon");
    press_return(&mut h);
    type_in_focus(&mut h, "12");
    press_return(&mut h);
    // The next takes it in the field that was edited last.
    pen_tap_at(&mut h, center + vec2(40.0, 0.0));
    type_in_focus(&mut h, "30");
    press_return(&mut h);
    let features = line_features(&h, id);
    assert_eq!(features[0].attributes["name"], Value::Text("Avalon".into()));
    assert_eq!(features[0].attributes["people"], Value::Number(12.0));
    assert_eq!(features[1].attributes["name"], Value::Null);
    assert_eq!(features[1].attributes["people"], Value::Number(30.0));
}

#[test]
fn a_new_line_takes_its_name_in_the_inspector_in_one_undo_step() {
    use vale_store::Value;
    let dir = tempfile::tempdir().unwrap();
    let mut h = saved_pad(BOARDS[0], dir.path());
    let rivers = vector(&h, "Rivers");
    select_layer(&mut h, rivers);
    quick_tap(&mut h, VECTOR);
    let field = vale_store::Field::new("given", vale_store::FieldType::Text);
    assert!(h.state_mut().doc.project.add_field(rivers, field));
    h.state_mut().save_waiting().unwrap();
    open_panel(&mut h, select::ATTRIBUTES);
    let saved = saves(&h);
    let before = line_features(&h, rivers);
    let center = h.state().globe.rect.center();
    line_stroke(
        &mut h,
        center + vec2(-150.0, 40.0),
        center + vec2(150.0, -40.0),
    );
    let lines = line_features(&h, rivers);
    assert_eq!(lines.len(), before.len() + 1);
    let new = FeatureRef {
        layer: rivers,
        uuid: lines.last().unwrap().uuid(),
    };
    assert_eq!(selection(&h), [new]);
    assert_eq!(h.state().selection_form.as_ref().unwrap().features, [new]);
    assert!(h.state().new_feature.is_some());
    assert_eq!(saves(&h), saved);
    assert!(no_keyboard(&h));
    // Return goes from the first field to the last field.
    let names = form_fields(&h);
    tap(&mut h, &names[0]);
    for _ in 1..names.len() {
        press_return(&mut h);
    }
    type_in_focus(&mut h, "Avon");
    press_return(&mut h);
    assert!(h.state().new_feature.is_none());
    let line = line_features(&h, rivers).pop().unwrap();
    assert_eq!(line.attributes["given"], Value::Text("Avon".into()));
    assert_eq!(saves(&h), saved + 1);
    tap(&mut h, "Undo");
    assert_eq!(line_features(&h, rivers), before);
}

#[test]
fn a_stroke_that_extends_a_line_does_not_change_the_selection_or_the_inspector() {
    let mut h = pad(BOARDS[0]);
    let rivers = vector(&h, "Rivers");
    select_layer(&mut h, rivers);
    quick_tap(&mut h, VECTOR);
    open_panel(&mut h, select::ATTRIBUTES);
    let center = h.state().globe.rect.center();
    let end = center + vec2(150.0, -40.0);
    line_stroke(&mut h, center + vec2(-150.0, 40.0), end);
    let first = line_features(&h, rivers).pop().unwrap().uuid();
    line_stroke(
        &mut h,
        center + vec2(-150.0, 120.0),
        center + vec2(100.0, 140.0),
    );
    let lines = line_features(&h, rivers);
    let second = FeatureRef {
        layer: rivers,
        uuid: lines.last().unwrap().uuid(),
    };
    assert_ne!(second.uuid, first);
    assert_eq!(selection(&h), [second]);
    line_stroke(&mut h, end + vec2(4.0, 3.0), end + vec2(60.0, 80.0));
    let extended = line_features(&h, rivers);
    assert_eq!(extended.len(), lines.len());
    let line = |features: &[vale_store::Feature]| {
        let feature = features.iter().find(|f| f.uuid() == first).unwrap();
        feature.geometry.clone()
    };
    assert_ne!(line(&extended), line(&lines));
    assert_eq!(selection(&h), [second]);
    assert_eq!(
        h.state().selection_form.as_ref().unwrap().features,
        [second]
    );
    assert!(h.state().new_feature.is_none());
}

#[test]
fn a_new_point_is_not_under_the_inspector_or_the_keyboard() {
    for size in [BOARDS[0], BOARDS[1]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        let id = new_point_layer(&mut h);
        open_panel(&mut h, select::ATTRIBUTES);
        let inspector = panel(&h, Panel::Attributes).expect("no panel");
        let rect = h.state().globe.rect;
        let keyboard = vale_app::ui::new_feature::keyboard_top(rect);
        // With "Focus new features", each new point has the keyboard.
        tap(&mut h, select::FOCUS_NEW);
        // A point low on the screen is under the keyboard. A point next to
        // the panel is too near it.
        let near = pos2(inspector.left() - 6.0, rect.center().y);
        let low = pos2(rect.center().x, keyboard + 20.0);
        for at in [low, near] {
            pen_tap_at(&mut h, at);
            h.run_steps(2);
            let pos = last_point(&h, id);
            assert!(!inspector.expand(20.0).contains(pos), "{what}: {pos:?}");
            assert!(pos.y < keyboard, "{what}: {pos:?}");
        }
    }
}

/// The place of the last point of a layer on the screen.
fn last_point(h: &Pad, id: vale_store::LayerId) -> Pos2 {
    let point = match &line_features(h, id).last().unwrap().geometry {
        vale_store::Geometry::Points(points) => points[0],
        _ => panic!("not a point"),
    };
    let dir = vale_app::globe::math::lonlat_to_dir(point[0], point[1]);
    h.state()
        .globe
        .project(dir)
        .expect("the point does not show")
}

#[test]
fn on_an_iphone_a_new_point_has_a_card_below_the_top_row_and_stays_above_the_keyboard() {
    use vale_app::ui::pad::geometry::{GAP, MARGIN};
    use vale_app::ui::pad::point_card::{ADD, CANCEL};
    let mut h = phone_pad();
    let places = vector(&h, "Places");
    h.state_mut().select(Some(LayerRef::Vector(places)));
    h.run_steps(2);
    quick_tap(&mut h, VECTOR);
    assert_eq!(h.state().globe.tool, Tool::Point);
    h.state_mut().globe.snap.on = false;
    open_panel(&mut h, select::ATTRIBUTES);
    assert!(h.state().pad.rects.unwrap().sheet.is_some());
    let fields = h.state().doc.project.layer(places).unwrap().field_names();
    let screen = h.state().pad.rects.unwrap().screen;
    let keyboard = vale_app::ui::new_feature::keyboard_top(screen);
    assert_eq!(keyboard, 852.0 - vale_app::ui::new_feature::PHONE_KEYBOARD);

    // The sheet at its title row leaves the globe free.
    h.state_mut().pad.sheet_offset = h.state().pad.rects.unwrap().sheet_travel;
    h.run_steps(2);

    // A point low on the screen, where the keyboard comes up. The card of
    // the point is directly below the top row, at the full width inside the
    // margins, and the sheet closes.
    let before = line_features(&h, places).len();
    let low = pos2(screen.center().x, keyboard + 60.0);
    pen_tap_at(&mut h, low);
    assert_eq!(line_features(&h, places).len(), before + 1);
    let rects = h.state().pad.rects.unwrap();
    assert_eq!(rects.sheet, None);
    let card = panel(&h, Panel::Attributes).expect("no card");
    assert_eq!(card.top(), rects.workspace.bottom() + GAP);
    assert_eq!(card.left(), screen.left() + MARGIN);
    assert_eq!(card.right(), screen.right() - MARGIN);
    assert!(card.bottom() < keyboard - 48.0, "{card:?}");
    for name in [ADD, CANCEL, fields[0].as_str()] {
        let rect = control(&h, name).unwrap_or_else(|| panic!("no control `{name}`"));
        assert!(card.contains_rect(rect), "{name}");
    }
    assert!(control(&h, vale_app::ui::table::SHOW_IN_TABLE).is_none());
    check_controls(&h, "point card");
    // With no keyboard, the point keeps its place.
    assert!(no_keyboard(&h));
    assert!((last_point(&h, places) - low).length() < 2.0);
    // A field takes the keyboard, and the point moves between the card and
    // the keyboard.
    tap(&mut h, &fields[0]);
    assert!(!no_keyboard(&h));
    let pos = last_point(&h, places);
    assert!(pos.y > card.bottom() && pos.y < keyboard, "{pos:?}");
    screenshot::save(&mut h, "phone-point-card");
    // Cancel removes the point.
    tap(&mut h, CANCEL);
    assert_eq!(line_features(&h, places).len(), before);
    assert!(h.state().new_feature.is_none());
    assert!(no_keyboard(&h));

    // With "Focus new features", the keyboard comes up with the card. A
    // point under the place of the card moves below the card.
    h.state_mut().tools.focus_new = true;
    let high = pos2(screen.center().x, card.center().y);
    pen_tap_at(&mut h, high);
    h.run_steps(2);
    assert!(!no_keyboard(&h));
    let card = panel(&h, Panel::Attributes).expect("no card");
    let pos = last_point(&h, places);
    assert!(
        pos.y > card.bottom() && pos.y < keyboard,
        "{pos:?} {card:?}"
    );
    type_in_focus(&mut h, "Ferrow");
    tap(&mut h, ADD);
    assert!(h.state().new_feature.is_none());
    let feature = line_features(&h, places).pop().unwrap();
    assert_eq!(line_features(&h, places).len(), before + 1);
    let name = feature.attributes[&fields[0]].label().unwrap_or_default();
    assert!(name.ends_with("Ferrow"), "{name}");
    // The sheet of the inspector shows its title row only.
    let rects = h.state().pad.rects.unwrap();
    assert_eq!(h.state().pad.sheet_offset, rects.sheet_travel);
}

#[test]
fn a_new_point_snaps_to_a_vertex() {
    let mut h = pad(BOARDS[0]);
    let places = vector(&h, "Places");
    h.state_mut().select(Some(LayerRef::Vector(places)));
    h.run_steps(2);
    quick_tap(&mut h, VECTOR);
    let target = match &line_features(&h, places)[0].geometry {
        vale_store::Geometry::Points(points) => points[0],
        _ => panic!("not a point"),
    };
    // The towns near the target are far apart at this zoom.
    let view = GlobeView::centered(target[0], target[1]);
    h.state_mut().globe.view = GlobeView { zoom: 20.0, ..view };
    h.run_steps(2);
    let dir = vale_app::globe::math::lonlat_to_dir(target[0], target[1]);
    let at = h.state().globe.project(dir).unwrap();
    pen_tap_at(&mut h, at + vec2(1.0, 1.0));
    let features = line_features(&h, places);
    let vale_store::Geometry::Points(points) = &features.last().unwrap().geometry else {
        panic!("not a point");
    };
    assert_eq!(points[0], target);
}

/// The name and the type of each field of a layer.
fn schema(layer: &vale_store::Layer) -> Vec<(String, vale_store::FieldType)> {
    layer
        .fields
        .iter()
        .map(|f| (f.name.clone(), f.kind))
        .collect()
}

fn pad_schema(h: &Pad, id: vale_store::LayerId) -> Vec<(String, vale_store::FieldType)> {
    schema(h.state().doc.layer(id).unwrap())
}

#[test]
fn you_add_rename_and_remove_a_field_and_the_file_has_the_same_fields() {
    use vale_app::ui::pad::fields::{ADD, ADD_FIELD, DELETE_FIELD, FIELD_NAME};
    use vale_store::FieldType::{Integer, Text};
    let dir = tempfile::tempdir().unwrap();
    let mut h = saved_pad(BOARDS[0], dir.path());
    open_panel(&mut h, "Layers");
    tap(&mut h, ADD_LAYER);
    tap(&mut h, "Points");
    let id = vector(&h, "Points 1");
    assert_eq!(pad_schema(&h, id), [("name".to_string(), Text)]);
    h.state_mut().save_waiting().unwrap();
    let saved = saves(&h);

    scroll_to_control(&mut h, ADD_FIELD);
    tap(&mut h, ADD_FIELD);
    assert_eq!(h.state().pad.page, Some(Page::NewField));
    assert!(h.state().pad.is_open(Panel::Layers));
    // The name has the focus, so the on-screen keyboard types into it at
    // once.
    h.event(egui::Event::Text("population".into()));
    h.run_steps(2);
    tap(&mut h, "Whole number");
    tap(&mut h, ADD);
    assert_eq!(h.state().pad.page, None);
    let fields = [
        ("name".to_string(), Text),
        ("population".to_string(), Integer),
    ];
    assert_eq!(pad_schema(&h, id), fields);
    assert_eq!(saves(&h), saved + 1);

    tap(&mut h, "population");
    assert_eq!(h.state().pad.page, Some(Page::Field(1)));
    assert!(control(&h, "Back to Points 1").is_some());
    type_text(&mut h, FIELD_NAME, "people");
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    let fields = [("name".to_string(), Text), ("people".to_string(), Integer)];
    assert_eq!(pad_schema(&h, id), fields);
    assert_eq!(saves(&h), saved + 2);
    assert!(h.state().pad.is_open(Panel::Layers));
    tap(&mut h, "Back to Points 1");

    // Return in the name adds the field.
    scroll_to_control(&mut h, ADD_FIELD);
    tap(&mut h, ADD_FIELD);
    h.event(egui::Event::Text("height".into()));
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(h.state().pad.page, None);
    assert_eq!(pad_schema(&h, id).len(), 3);
    assert_eq!(saves(&h), saved + 3);
    tap(&mut h, "height");
    tap(&mut h, DELETE_FIELD);
    assert_eq!(h.state().pad.page, None);
    assert_eq!(pad_schema(&h, id), fields);
    assert_eq!(saves(&h), saved + 4);

    h.state_mut().save_waiting().unwrap();
    let project = vale_store::ProjectFile::open(&dir.path().join("w.gpkg"))
        .unwrap()
        .1;
    assert_eq!(schema(project.layer(id).unwrap()), fields);

    tap(&mut h, "Undo");
    let names = h.state().doc.layer(id).unwrap().field_names();
    assert_eq!(names, ["name", "people", "height"]);
}

#[test]
fn a_name_that_is_not_free_turns_add_off() {
    use vale_app::ui::pad::fields::{ADD, ADD_FIELD, FIELD_NAME};
    let mut h = pad(BOARDS[0]);
    open_panel(&mut h, "Layers");
    tap(&mut h, ADD_LAYER);
    tap(&mut h, "Points");
    let id = vector(&h, "Points 1");
    scroll_to_control(&mut h, ADD_FIELD);
    tap(&mut h, ADD_FIELD);
    // A blank name is not free.
    assert!(control(&h, ADD).is_none());
    for name in ["Name", "fid", "GEOM", " "] {
        type_text(&mut h, FIELD_NAME, name);
        assert!(control(&h, ADD).is_none(), "{name}");
        h.key_press(egui::Key::Enter);
        h.run_steps(2);
        assert_eq!(h.state().pad.page, Some(Page::NewField), "{name}");
    }
    type_text(&mut h, FIELD_NAME, "names");
    assert!(control(&h, ADD).is_some());
    // A default that is not a whole number turns Add off.
    tap(&mut h, "Whole number");
    type_text(&mut h, vale_app::ui::pad::fields::DEFAULT_VALUE, "many");
    assert!(control(&h, ADD).is_none());
    type_text(&mut h, vale_app::ui::pad::fields::DEFAULT_VALUE, "12");
    tap(&mut h, ADD);
    let layer = h.state().doc.layer(id).unwrap();
    assert_eq!(layer.field_names(), ["name", "names"]);
    assert_eq!(layer.fields[1].default, vale_store::Value::Number(12.0));
}

#[test]
fn a_rename_of_the_label_field_keeps_the_labels() {
    use vale_app::pipeline::{Pipeline, Quality, fonts};
    use vale_app::ui::pad::fields::FIELD_NAME;
    let placed = |doc: &Document| {
        let mut fonts = fonts().unwrap();
        let mut pipeline = Pipeline::new();
        let composed = pipeline.compose(doc, (1280.0, 800.0), &mut fonts, Quality::Final);
        composed.unwrap().labels.placed
    };
    let mut h = pad(BOARDS[0]);
    let places = vector(&h, "Places");
    let before = placed(&h.state().doc);
    assert!(before > 0);
    let index = h.state().doc.layer(places).unwrap().field_index("name");
    open_panel(&mut h, "Layers");
    h.state_mut().pad.layer = Some(LayerRef::Vector(places));
    h.state_mut().pad.page = Some(Page::Field(index.unwrap()));
    h.run_steps(2);
    type_text(&mut h, FIELD_NAME, "title");
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    let doc = &h.state().doc;
    assert!(doc.layer(places).unwrap().field("title").is_some());
    let label = &doc.entry(places).unwrap().style.label;
    assert_eq!(label.text, "title");
    assert_eq!(placed(doc), before);
    // A rename of the priority field keeps it too.
    let doc = &mut h.state_mut().doc;
    assert!(doc.rename_field(places, "pop_max", "people"));
    let label = &doc.entry(places).unwrap().style.label;
    assert_eq!(label.priority_field.as_deref(), Some("people"));
}

#[test]
fn the_pages_of_the_fields_have_large_controls() {
    use vale_app::ui::pad::fields::ADD_FIELD;
    for size in BOARDS {
        let mut h = pad(size);
        open_panel(&mut h, "Layers");
        tap(&mut h, ADD_LAYER);
        tap(&mut h, "Points");
        check_controls(&h, &format!("{size:?} on the panel of a point layer"));
        // The Style section is above the fields.
        scroll_to(&mut h, ADD_FIELD);
        tap(&mut h, ADD_FIELD);
        check_controls(&h, &format!("{size:?} on the page of a new field"));
        tap(&mut h, "True or false");
        // A sheet shows the choices after a scroll.
        let sheet = h.state().pad.rects.unwrap().sheet.is_some();
        assert!(sheet || control(&h, "True").is_some(), "{size:?}");
        check_controls(&h, &format!("{size:?} with a true or false field"));
        let id = vector(&h, "Points 1");
        h.state_mut().pad.page = Some(Page::Field(0));
        h.run_steps(2);
        check_controls(&h, &format!("{size:?} on the page of a field"));
        assert_eq!(h.state().doc.layer(id).unwrap().field_names(), ["name"]);
    }
}

/// Runs three frames and the saves of the app, as the window does, and
/// saves a change that waits.
fn save_frames(h: &mut Pad) {
    for _ in 0..3 {
        step_and_save(h, false);
    }
    h.state_mut().save_waiting().unwrap();
}

/// Scrolls the Layers panel down until a control is inside the card, or up
/// while the control is above the card.
fn scroll_to_control(h: &mut Pad, name: &str) {
    scroll_to(h, name);
    for _ in 0..8 {
        if control(h, name).is_some() {
            return;
        }
        let card = panel(h, Panel::Layers).unwrap();
        let from = pos2(card.left() + 6.0, card.top() + 112.0);
        drag(h, from, from + vec2(0.0, 200.0), None);
    }
}

/// Types on the keypad of a field, and scrolls to each key.
fn type_scrolled_keys(h: &mut Pad, field: &str, text: &str) {
    use vale_app::ui::pad::widgets::DONE;
    scroll_to_control(h, field);
    tap(h, field);
    for key in text.chars().map(|c| format!("{field} {c}")) {
        scroll_to_control(h, &key);
        tap(h, &key);
    }
    let done = format!("{field} {DONE}");
    scroll_to_control(h, &done);
    tap(h, &done);
}

/// Types `text` on the keypad of the number `name` in the Layers panel.
fn set_number(h: &mut Pad, name: &str, text: &str) {
    type_scrolled_keys(h, &format!("{name} value"), text);
}

/// A tap on the plus button (`up`) or the minus button of the stepper
/// `name` in the Layers panel.
fn scrolled_step(h: &mut Pad, name: &str, up: bool) {
    let button = if up { "Increase" } else { "Decrease" };
    scroll_to_control(h, &format!("{button} {name}"));
    step(h, name, up);
}

/// A drag along the slider `name` from near its left end to far past its
/// right end, which moves the value to the end of the range.
fn drag_right(h: &mut Pad, name: &str) {
    scroll_to_control(h, name);
    let slider = control(h, name).unwrap();
    let y = slider.center().y;
    let from = pos2(slider.left() + 100.0, y);
    drag(h, from, pos2(from.x + 600.0, y), None);
}

/// The rules of the style of a vector layer.
fn rules(h: &Pad, id: vale_store::LayerId) -> Vec<vale_style::Rule> {
    h.state().doc.entry(id).unwrap().style.style.rules.clone()
}

fn rule_names(h: &Pad, id: vale_store::LayerId) -> Vec<String> {
    rules(h, id).into_iter().map(|r| r.name).collect()
}

/// Opens the panel of a vector layer and the page of its rule `rule`.
fn open_rule(h: &mut Pad, layer: &str, rule: &str) {
    open_panel(h, "Layers");
    tap(h, &format!("Open {layer}"));
    scroll_to_control(h, rule);
    tap(h, rule);
    assert!(matches!(h.state().pad.page, Some(Page::Rule(_))));
}

/// Opens the Classify form of a vector layer.
fn open_classify(h: &mut Pad, layer: &str) {
    use vale_app::ui::pad::classify::CLASSIFY;
    open_panel(h, "Layers");
    tap(h, &format!("Open {layer}"));
    scroll_to_control(h, CLASSIFY);
    tap(h, CLASSIFY);
    assert_eq!(h.state().pad.page, Some(Page::Classify));
}

#[test]
fn classify_makes_quantile_rules_in_one_undo_step() {
    use vale_app::ui::pad::classify::{EXPRESSION, REPLACE_RULES};
    let dir = tempfile::tempdir().unwrap();
    let mut h = saved_pad(BOARDS[0], dir.path());
    let places = vector(&h, "Places");
    let before = rules(&h, places);
    open_classify(&mut h, "Places");
    scroll_to_control(&mut h, EXPRESSION);
    type_text(&mut h, EXPRESSION, "pop_max");
    scroll_to_control(&mut h, "Quantiles");
    tap(&mut h, "Quantiles");
    scroll_to_control(&mut h, REPLACE_RULES);
    tap(&mut h, REPLACE_RULES);
    save_frames(&mut h);
    assert_eq!(h.state().pad.page, None);
    let made = rules(&h, places);
    assert!(
        (2..=5).contains(&made.len()),
        "{:?}",
        rule_names(&h, places)
    );
    let texts: Vec<String> = made
        .iter()
        .map(|r| r.filter.source().unwrap().text().to_owned())
        .collect();
    assert!(texts[0].starts_with("pop_max < "), "{texts:?}");
    assert!(
        texts.last().unwrap().starts_with("pop_max >= "),
        "{texts:?}"
    );
    for r in &made {
        assert!(r.filter.error().is_none(), "{}", r.name);
    }
    let colors: Vec<_> = made.iter().map(|r| r.fill().unwrap()).collect();
    assert_ne!(colors[0], colors[colors.len() - 1]);

    // The rule page of a middle class shows its two conditions.
    let middle = made[1].name.clone();
    assert_eq!(
        vale_style::expr::editor::Conditions::from_text(&texts[1])
            .items
            .len(),
        2,
        "{texts:?}"
    );
    scroll_to_control(&mut h, &middle);
    tap(&mut h, &middle);
    assert_eq!(h.state().pad.page, Some(Page::Rule(1)));
    for n in 1..=2 {
        let remove = format!("Remove condition {n}");
        assert!(control(&h, &remove).is_some(), "{remove}");
    }

    tap(&mut h, "Undo");
    assert_eq!(rules(&h, places), before);
}

#[test]
fn classify_colors_neighbors_differently_in_one_undo_step() {
    use vale_app::ui::pad::classify::{NEIGHBORS_DIFFER, REPLACE_RULES};
    use vale_store::{Feature, FieldType, Geometry, Value};
    let dir = tempfile::tempdir().unwrap();
    let mut h = saved_pad(BOARDS[0], dir.path());

    // A grid of 3 by 3 squares of 10 degrees.
    let state = h.state_mut();
    let id = state.doc.add_layer(GeometryKind::Polygon);
    for i in 0..9 {
        let (x, y) = ((i % 3) as f64 * 10.0, (i / 3) as f64 * 10.0);
        let ring = vec![[x, y], [x + 10.0, y], [x + 10.0, y + 10.0], [x, y + 10.0]];
        let feature = Feature::new(Geometry::Polygons(vec![vec![ring]]), Default::default());
        assert!(state.doc.project.add_feature(id, feature));
    }
    state.touch();
    save_frames(&mut h);
    let name = h.state().doc.layer(id).unwrap().name.clone();
    let before = rules(&h, id);

    open_classify(&mut h, &name);
    tap(&mut h, NEIGHBORS_DIFFER);
    scroll_to_control(&mut h, REPLACE_RULES);
    tap(&mut h, REPLACE_RULES);
    save_frames(&mut h);
    assert_eq!(h.state().pad.page, None);

    let layer = h.state().doc.layer(id).unwrap();
    let field = layer.fields.iter().find(|f| f.name == "color_group");
    assert_eq!(field.map(|f| f.kind), Some(FieldType::Integer));
    let colors: Vec<f64> = layer
        .features
        .iter()
        .map(|f| match f.attributes["color_group"] {
            Value::Number(n) => n,
            ref v => panic!("{v:?}"),
        })
        .collect();
    // Squares that share an edge differ. The grid needs 2 colors at least.
    for i in 0..9 {
        if i % 3 < 2 {
            assert_ne!(colors[i], colors[i + 1], "{colors:?}");
        }
        if i < 6 {
            assert_ne!(colors[i], colors[i + 3], "{colors:?}");
        }
    }
    let count = colors.iter().fold(0.0_f64, |a, b| a.max(*b)) as usize;
    let mut expected: Vec<String> = (1..=count).map(|i| format!("Color {i}")).collect();
    expected.push(vale_app::classify::OTHER_VALUES.to_owned());
    assert_eq!(rule_names(&h, id), expected);
    for r in rules(&h, id) {
        assert!(r.filter.error().is_none(), "{}", r.name);
    }

    tap(&mut h, "Undo");
    assert_eq!(rules(&h, id), before);
    let layer = h.state().doc.layer(id).unwrap();
    assert!(layer.fields.iter().all(|f| f.name != "color_group"));

    // A point layer has no such switch.
    let mut h = pad(BOARDS[0]);
    open_classify(&mut h, "Places");
    assert!(control(&h, NEIGHBORS_DIFFER).is_none());
}

#[test]
fn value_preview_shows_range() {
    use vale_app::ui::expr_help::{range_text, value_range};
    use vale_app::ui::pad::classify::EXPRESSION;
    let mut h = pad(BOARDS[0]);
    let places = vector(&h, "Places");
    open_classify(&mut h, "Places");
    scroll_to_control(&mut h, EXPRESSION);
    type_text(&mut h, EXPRESSION, "pop_max / 1000");
    let layer = h.state().doc.layer(places).unwrap();
    let range = value_range(layer, "pop_max / 1000").unwrap();
    assert_eq!(range.count, layer.features.len());
    assert!(range.min < range.max);
    let line = range_text(&range);
    assert!(h.query_by_label(&line).is_some(), "{line}");
    // The cache computes the range once for the text.
    let computed = h.state().help_cache.computed();
    h.run_steps(3);
    assert_eq!(h.state().help_cache.computed(), computed);
}

#[test]
fn classify_value_editor_wraps_the_value_from_the_tray() {
    use vale_app::ui::pad::classify::{EXPRESSION, REPLACE_RULES};
    use vale_app::ui::pad::value::{FUNCTIONS_TAB, SEARCH, insert_name};
    let mut h = pad(BOARDS[0]);
    let places = vector(&h, "Places");
    open_classify(&mut h, "Places");
    scroll_to_control(&mut h, EXPRESSION);
    type_text(&mut h, EXPRESSION, "pop_max ");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    scroll_to_control(&mut h, FUNCTIONS_TAB);
    tap(&mut h, FUNCTIONS_TAB);
    type_text(&mut h, SEARCH[1], "round");
    let round = insert_name("round");
    scroll_to_control(&mut h, &round);
    tap(&mut h, &round);
    screenshot::save(&mut h, "pad-value-tray");
    scroll_to_control(&mut h, "Quantiles");
    tap(&mut h, "Quantiles");
    scroll_to_control(&mut h, REPLACE_RULES);
    tap(&mut h, REPLACE_RULES);
    let first = rules(&h, places)[0]
        .filter
        .source()
        .unwrap()
        .text()
        .to_owned();
    assert!(first.starts_with("round(pop_max) < "), "{first}");
}

/// Classifies Places by its value with Quantiles, and returns the filter
/// of the first rule.
fn classify_first_filter(h: &mut Pad) -> String {
    use vale_app::ui::pad::classify::REPLACE_RULES;
    let places = vector(h, "Places");
    scroll_to_control(h, "Quantiles");
    tap(h, "Quantiles");
    scroll_to_control(h, REPLACE_RULES);
    tap(h, REPLACE_RULES);
    rules(h, places)[0]
        .filter
        .source()
        .unwrap()
        .text()
        .to_owned()
}

#[test]
fn add_case_writes_nested_if() {
    use vale_app::ui::pad::cases::ADD_CASE;
    use vale_app::ui::pad::classify::EXPRESSION;
    let mut h = pad(BOARDS[0]);
    open_classify(&mut h, "Places");
    scroll_to_control(&mut h, EXPRESSION);
    type_text(&mut h, EXPRESSION, "if(scalerank < 3, 18, 10)");
    // The nested ifs show as a Cases block.
    scroll_to_control(&mut h, ADD_CASE);
    assert!(control(&h, "Case 1 Field 1").is_some());
    assert!(control(&h, "Otherwise result").is_some());
    tap(&mut h, ADD_CASE);
    screenshot::save(&mut h, "pad-value-cases");
    assert!(control(&h, "Case 2 Field 1").is_some());
    let first = classify_first_filter(&mut h);
    assert!(first.contains("if(scalerank < 3, 18, if("), "{first}");
    assert!(first.contains(", 10, 10))"), "{first}");
}

#[test]
fn a_missing_condition_shows_its_message() {
    use vale_app::ui::pad::classify::EXPRESSION;
    let mut h = pad(BOARDS[0]);
    open_classify(&mut h, "Places");
    scroll_to_control(&mut h, EXPRESSION);
    let text = "if(, 12, 10)";
    type_text(&mut h, EXPRESSION, text);
    // Blocks name the part in words.
    let message = "The first case needs a condition.";
    scroll_to(&mut h, message);
    assert!(h.query_by_label(message).is_some(), "{message}");
    assert!(control(&h, "Case 1 condition 1").is_some());
    screenshot::save(&mut h, "pad-value-cases-error");
    // Code keeps the text of the parser.
    let [_, code] = vale_app::ui::pad::value::view_names(EXPRESSION);
    scroll_to_control(&mut h, &code);
    tap(&mut h, &code);
    let short = vale_style::expr::Expr::parse(text).unwrap_err().short();
    assert!(h.query_by_label(&short).is_some(), "{short}");
}

/// The width of the Layers panel.
fn layers_width(h: &Pad) -> f32 {
    panel(h, Panel::Layers).unwrap().width()
}

#[test]
fn expression_page_widens_panel() {
    use vale_app::ui::pad::expression::PANEL_WIDTH;
    for size in [BOARDS[0], BOARDS[1]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        open_panel(&mut h, "Layers");
        tap(&mut h, "Open Places");
        let normal = layers_width(&h);
        assert!(normal < PANEL_WIDTH, "{what}");
        open_classify_page(&mut h);
        assert_eq!(layers_width(&h), PANEL_WIDTH, "{what}");
        check_controls(&h, &what);
        tap(&mut h, "Back to Places");
        assert_eq!(layers_width(&h), normal, "{what}");
    }
    // A sheet keeps its width.
    let mut h = pad(BOARDS[3]);
    open_panel(&mut h, "Layers");
    tap(&mut h, "Open Places");
    let normal = layers_width(&h);
    open_classify_page(&mut h);
    assert_eq!(layers_width(&h), normal);
}

/// Opens the Classify page from the open panel of a layer.
fn open_classify_page(h: &mut Pad) {
    use vale_app::ui::pad::classify::CLASSIFY;
    scroll_to_control(h, CLASSIFY);
    tap(h, CLASSIFY);
    assert_eq!(h.state().pad.page, Some(Page::Classify));
}

#[test]
fn value_bar_inserts_field() {
    use vale_app::ui::pad::classify::EXPRESSION;
    use vale_app::ui::pad::value::{FUNCTIONS_TAB, OPERATORS_TAB, bar_name, insert_name};
    use vale_app::ui::token_field::formula::op_label;
    use vale_style::expr::editor::Op;
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        open_classify(&mut h, "Places");
        scroll_to_control(&mut h, EXPRESSION);
        type_text(&mut h, EXPRESSION, "pop_max ");
        tap_bar(&mut h, &bar_name(OPERATORS_TAB));
        tap_bar(&mut h, &bar_name(&insert_name(op_label(Op::Div))));
        tap_bar(&mut h, &bar_name(FUNCTIONS_TAB));
        // The typed word filters the functions.
        h.event(egui::Event::Text(" ab".into()));
        h.run_steps(2);
        assert!(
            control(&h, &bar_name(&insert_name("round"))).is_none(),
            "{what}"
        );
        tap_bar(&mut h, &bar_name(&insert_name("abs")));
        tap_bar(&mut h, &bar_name("Fields"));
        h.event(egui::Event::Text("scale".into()));
        h.run_steps(2);
        tap_bar(&mut h, &bar_name(&insert_name("scalerank")));
        screenshot::save(&mut h, &format!("pad-value-bar-{}", size.0));
        let first = classify_first_filter(&mut h);
        assert!(
            first.starts_with("(pop_max / abs(scalerank)) < "),
            "{what}: {first}"
        );
    }
}

#[test]
fn a_code_chip_opens_code() {
    use vale_app::ui::pad::classify::EXPRESSION;
    use vale_app::ui::pad::value::{CODE_CHIP, view_names};
    let mut h = pad(BOARDS[0]);
    open_classify(&mut h, "Places");
    let [blocks, code] = view_names(EXPRESSION);
    let text_field = format!("{EXPRESSION} text");
    scroll_to_control(&mut h, &code);
    tap(&mut h, &code);
    type_text(&mut h, &text_field, "round(pop_max) + )");
    tap(&mut h, &blocks);
    let chip = format!("{CODE_CHIP} 1");
    scroll_to_control(&mut h, &chip);
    tap(&mut h, &chip);
    assert!(control(&h, &text_field).is_some());
}

#[test]
fn classify_by_text_gives_a_rule_for_each_value_and_other_values() {
    use vale_app::classify::{MAX_CATEGORIES, OTHER_VALUES};
    use vale_app::ui::pad::classify::REPLACE_RULES;
    let mut h = pad(BOARDS[0]);
    let rivers = vector(&h, "Rivers");
    let layer = h.state().doc.layer(rivers).unwrap();
    let mut kinds: Vec<String> = layer
        .features
        .iter()
        .filter_map(|f| f.attributes.get("featurecla")?.label())
        .collect();
    kinds.sort();
    kinds.dedup();
    open_classify(&mut h, "Rivers");
    scroll_to_control(&mut h, "featurecla");
    tap(&mut h, "featurecla");
    scroll_to_control(&mut h, REPLACE_RULES);
    tap(&mut h, REPLACE_RULES);
    let names = rule_names(&h, rivers);
    assert_eq!(
        names.len(),
        kinds.len().min(MAX_CATEGORIES) + 1,
        "{names:?}"
    );
    assert_eq!(names.last().unwrap(), OTHER_VALUES);
    for kind in kinds.iter().take(MAX_CATEGORIES) {
        assert!(names.contains(kind), "{kind}");
    }
    let made = rules(&h, rivers);
    let first = made[0].filter.source().unwrap().text().to_owned();
    assert_eq!(first, format!("featurecla = '{}'", made[0].name));
}

#[test]
fn classify_gives_a_value_the_color_of_its_label() {
    use vale_app::ui::pad::classify::{REPLACE_RULES, summary};
    use vale_app::ui::pad::raster::{IMPORT_RAMP, USE_RAMP};
    let mut h = pad(BOARDS[0]);
    let rivers = vector(&h, "Rivers");
    let ramps = &mut h.state_mut().ramps;
    ramps.add("Water", vec![[10, 10, 10], [200, 0, 0]]).unwrap();
    let at = ramps.list().len() - 1;
    ramps.set_label(at, 1, "River").unwrap();
    open_classify(&mut h, "Rivers");
    scroll_to_control(&mut h, "featurecla");
    tap(&mut h, "featurecla");
    // The row of the ramp opens the page of the ramps, and "Use" picks one.
    scroll_to_control(&mut h, "Bold");
    tap(&mut h, "Bold");
    assert_eq!(h.state().pad.page, Some(Page::Ramps));
    tap(&mut h, IMPORT_RAMP);
    assert!(h.state().actions.contains(&Action::ImportRampDialog(None)));
    h.state_mut().actions.clear();
    scroll_to_control(&mut h, "Water");
    tap(&mut h, "Water");
    tap(&mut h, USE_RAMP);
    assert_eq!(h.state().pad.page, Some(Page::Classify));
    scroll_to_control(&mut h, "Water");
    tap(&mut h, "Water");
    tap(&mut h, "Back to Classify");
    assert_eq!(h.state().pad.page, Some(Page::Classify));
    scroll_to_control(&mut h, REPLACE_RULES);
    assert!(h.query_by_label(&summary(2, 1)).is_some());
    tap(&mut h, REPLACE_RULES);
    let made = rules(&h, rivers);
    let river = made.iter().find(|r| r.name == "River").unwrap();
    assert_eq!(
        river.stroke().unwrap().color,
        vale_style::Color::rgb(200, 0, 0)
    );
}

#[test]
fn classify_reads_an_expression() {
    use vale_app::ui::pad::classify::{EXPRESSION, REPLACE_RULES};
    let mut h = pad(BOARDS[0]);
    let rivers = vector(&h, "Rivers");
    open_classify(&mut h, "Rivers");
    scroll_to_control(&mut h, EXPRESSION);
    type_text(&mut h, EXPRESSION, "scalerank * 2");
    scroll_to_control(&mut h, "Natural breaks");
    tap(&mut h, "Natural breaks");
    scroll_to_control(&mut h, REPLACE_RULES);
    tap(&mut h, REPLACE_RULES);
    let made = rules(&h, rivers);
    assert!(made.len() >= 2);
    let first = made[0].filter.source().unwrap().text().to_owned();
    assert!(first.starts_with("(scalerank * 2) < "), "{first}");
}

#[test]
fn the_classify_form_fits_a_finger() {
    use vale_app::ui::pad::classify::{REPLACE_RULES, VARY_WIDTH};
    for size in BOARDS {
        let what = format!("{size:?}");
        let mut h = pad(size);
        open_classify(&mut h, "Rivers");
        check_controls(&h, &what);
        scroll_to_control(&mut h, "scalerank");
        tap(&mut h, "scalerank");
        screenshot::save(&mut h, &format!("pad-classify-top-{}", size.0));
        scroll_to_control(&mut h, VARY_WIDTH);
        tap(&mut h, VARY_WIDTH);
        check_controls(&h, &what);
        screenshot::save(&mut h, &format!("pad-classify-{}", size.0));
        scroll_to_control(&mut h, REPLACE_RULES);
        check_controls(&h, &what);
    }
}

#[test]
fn the_style_section_adds_moves_and_deletes_rules() {
    use vale_app::ui::pad::rules::{ADD_RULE, DELETE_RULE, MOVE_UP};
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let dir = tempfile::tempdir().unwrap();
        let mut h = saved_pad(size, dir.path());
        let rivers = vector(&h, "Rivers");
        open_panel(&mut h, "Layers");
        tap(&mut h, "Open Rivers");
        scroll_to_control(&mut h, ADD_RULE);
        tap(&mut h, ADD_RULE);
        save_frames(&mut h);
        assert_eq!(h.state().pad.page, Some(Page::Rule(1)), "{what}");
        tap(&mut h, "Back to Rivers");
        scroll_to_control(&mut h, ADD_RULE);
        tap(&mut h, ADD_RULE);
        save_frames(&mut h);
        assert_eq!(rule_names(&h, rivers), ["Default", "Rule 1", "Rule 2"]);
        // A new rule copies the stroke of the last rule, with a new color.
        let r = rules(&h, rivers);
        let (first, last) = (r[0].stroke().unwrap(), r[2].stroke().unwrap());
        assert_eq!(first.width, last.width, "{what}");
        assert_ne!(first.color, last.color, "{what}");

        scroll_to_control(&mut h, MOVE_UP);
        tap(&mut h, MOVE_UP);
        save_frames(&mut h);
        assert_eq!(rule_names(&h, rivers), ["Default", "Rule 2", "Rule 1"]);
        assert_eq!(h.state().pad.page, Some(Page::Rule(1)), "{what}");
        scroll_to_control(&mut h, DELETE_RULE);
        tap(&mut h, DELETE_RULE);
        save_frames(&mut h);
        assert_eq!(rule_names(&h, rivers), ["Default", "Rule 1"], "{what}");
        assert_eq!(h.state().pad.page, None, "{what}");

        tap(&mut h, "Undo");
        assert_eq!(rule_names(&h, rivers), ["Default", "Rule 2", "Rule 1"]);
        tap(&mut h, "Undo");
        assert_eq!(rule_names(&h, rivers), ["Default", "Rule 1", "Rule 2"]);
    }
}

#[test]
fn a_rule_page_sets_the_filter_and_the_scale_range() {
    use vale_app::ui::pad::filter::{NO_CONDITION, REMOVE_CONDITION, VIEWS};
    use vale_app::ui::pad::rules::{BAD_RANGE, EXPRESSION, MAX_SCALE, MIN_SCALE};
    for size in [BOARDS[1], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        let rivers = vector(&h, "Rivers");
        let rule = |h: &Pad| rules(h, rivers)[0].clone();
        let filter = |h: &Pad| {
            let r = rule(h);
            r.filter.source().map(|s| s.text().to_owned())
        };
        open_rule(&mut h, "Rivers", "Default");

        assert!(h.query_by_label(NO_CONDITION).is_some(), "{what}");
        // A condition on a number: the field menu, the comparison menu, and
        // the keypad.
        add_condition(&mut h, 1, "scalerank");
        pick(&mut h, "Comparison 1", "is less than");
        check_controls(&h, &format!("{what} with a condition"));
        type_scrolled_keys(&mut h, "Value 1", "5");
        assert_eq!(filter(&h).as_deref(), Some("scalerank < 5"), "{what}");

        // A condition on a text: a value of the layer from the menu.
        add_condition(&mut h, 2, "featurecla");
        pick(&mut h, "Value 2", "River");
        let text = "scalerank < 5 and featurecla = 'River'";
        assert_eq!(filter(&h).as_deref(), Some(text), "{what}");
        assert!(rule(&h).filter.error().is_none(), "{what}");

        // The button of a condition removes it.
        let first = format!("{REMOVE_CONDITION} 1");
        scroll_to_control(&mut h, &first);
        tap(&mut h, &first);
        let text = "featurecla = 'River'";
        assert_eq!(filter(&h).as_deref(), Some(text), "{what}");

        // Code takes any filter, and warns about a wrong one.
        scroll_to_control(&mut h, VIEWS[1]);
        tap(&mut h, VIEWS[1]);
        type_text(&mut h, EXPRESSION, "scalerank < 5 and");
        let message = rule(&h).filter.error().unwrap().short();
        assert!(h.query_by_label(&message).is_some(), "{what}");
        type_text(&mut h, EXPRESSION, "scalerank < 5");
        assert!(h.query_by_label(&message).is_none(), "{what}");

        scroll_to_control(&mut h, MIN_SCALE);
        type_scrolled_keys(&mut h, MIN_SCALE, "50000");
        assert_eq!(rule(&h).min_scale, Some(50_000.0), "{what}");
        scroll_to_control(&mut h, MAX_SCALE);
        type_scrolled_keys(&mut h, MAX_SCALE, "1000");
        assert_eq!(rule(&h).max_scale, Some(1000.0), "{what}");
        assert!(h.query_by_label(BAD_RANGE).is_some(), "{what}");
        scroll_to_control(&mut h, MAX_SCALE);
        type_scrolled_keys(&mut h, MAX_SCALE, "1000000");
        assert_eq!(rule(&h).max_scale, Some(1_000_000.0), "{what}");
        assert!(h.query_by_label(BAD_RANGE).is_none(), "{what}");
        assert!(rule(&h).in_scale(100_000.0), "{what}");
        assert!(!rule(&h).in_scale(10_000.0), "{what}");
    }
}

/// Taps the control `menu`, and then the row `choice` of its menu.
fn pick(h: &mut Pad, menu: &str, choice: &str) {
    use vale_app::ui::pad::filter::{SEARCH_FIELDS, SEARCH_VALUES};
    scroll_to_control(h, menu);
    tap(h, menu);
    // A long menu has a search field.
    for search in [SEARCH_FIELDS, SEARCH_VALUES] {
        if control(h, choice).is_none() && control(h, search).is_some() {
            type_text(h, search, choice);
        }
    }
    assert!(control(h, choice).is_some(), "no `{choice}` in `{menu}`");
    tap(h, choice);
}

/// Adds condition `n` of a filter, on field `field`.
fn add_condition(h: &mut Pad, n: usize, field: &str) {
    use vale_app::ui::pad::filter::ADD_CONDITION;
    scroll_to_control(h, ADD_CONDITION);
    tap(h, ADD_CONDITION);
    pick(h, &format!("Field {n}"), field);
}

/// Sets the filter of the first rule of layer `id`.
fn set_filter(h: &mut Pad, id: vale_store::LayerId, text: &str) {
    let entry = h.state_mut().doc.entry_mut(id).unwrap();
    entry.style.style.rules[0].filter = vale_style::expr::Filter::new(text);
    h.run_steps(2);
}

#[test]
fn match_any_writes_or() {
    use vale_app::ui::pad::filter::{ANY, MATCH};
    let mut h = pad(BOARDS[0]);
    let rivers = vector(&h, "Rivers");
    open_rule(&mut h, "Rivers", "Default");
    add_condition(&mut h, 1, "scalerank");
    add_condition(&mut h, 2, "featurecla");
    let text = filter_text(&h, rivers).unwrap();
    assert!(text.contains(" and "), "{text}");
    pick(&mut h, MATCH, ANY);
    let text = filter_text(&h, rivers).unwrap();
    assert!(text.starts_with("scalerank = 0 or featurecla = "), "{text}");
    screenshot::save(&mut h, "pad-filter-conditions");
}

#[test]
fn comparison_menu_follows_field_type() {
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        open_rule(&mut h, "Rivers", "Default");
        add_condition(&mut h, 1, "scalerank");
        scroll_to_control(&mut h, "Comparison 1");
        tap(&mut h, "Comparison 1");
        assert!(control(&h, "is greater than").is_some(), "{what}");
        assert!(control(&h, "is between").is_some(), "{what}");
        assert!(control(&h, "contains").is_none(), "{what}");
        tap(&mut h, "Comparison 1");
        assert!(control(&h, "is greater than").is_none(), "{what}");

        pick(&mut h, "Field 1", "featurecla");
        scroll_to_control(&mut h, "Comparison 1");
        tap(&mut h, "Comparison 1");
        assert!(control(&h, "contains").is_some(), "{what}");
        assert!(control(&h, "starts with").is_some(), "{what}");
        assert!(control(&h, "is greater than").is_none(), "{what}");
    }
}

#[test]
fn text_value_menu_lists_layer_values() {
    use vale_app::ui::pad::filter::{SEARCH_VALUES, use_value};
    let mut h = pad(BOARDS[0]);
    let rivers = vector(&h, "Rivers");
    open_rule(&mut h, "Rivers", "Default");
    add_condition(&mut h, 1, "featurecla");
    scroll_to_control(&mut h, "Value 1");
    tap(&mut h, "Value 1");
    assert!(control(&h, "River").is_some());
    assert!(control(&h, SEARCH_VALUES).is_some());
    // The search keeps the values that hold the text, and offers the text.
    type_text(&mut h, SEARCH_VALUES, "lake c");
    assert!(control(&h, "River").is_none());
    let typed = use_value("lake c");
    assert!(control(&h, &typed).is_some());
    tap(&mut h, &typed);
    let text = filter_text(&h, rivers);
    assert_eq!(text.as_deref(), Some("featurecla = 'lake c'"));
}

#[test]
fn add_group_nests_conditions() {
    use vale_app::ui::pad::filter::{ADD_CONDITION, ADD_GROUP, ANY_OF, REMOVE_CONDITION};
    let mut h = pad(BOARDS[0]);
    let rivers = vector(&h, "Rivers");
    open_rule(&mut h, "Rivers", "Default");
    add_condition(&mut h, 1, "scalerank");
    scroll_to_control(&mut h, ADD_GROUP);
    tap(&mut h, ADD_GROUP);
    assert!(h.query_by_label(ANY_OF).is_some() || control(&h, "Match 2").is_some());
    let in_group = format!("{ADD_CONDITION} 2");
    scroll_to_control(&mut h, &in_group);
    tap(&mut h, &in_group);
    pick(&mut h, "Field 2.2", "featurecla");
    let text = filter_text(&h, rivers).unwrap();
    assert!(text.starts_with("scalerank = 0 and ("), "{text}");
    assert!(text.contains(" or featurecla = "), "{text}");
    screenshot::save(&mut h, "pad-filter-group");
    let inner = format!("{REMOVE_CONDITION} 2.1");
    scroll_to_control(&mut h, &inner);
    tap(&mut h, &inner);
    let text = filter_text(&h, rivers).unwrap();
    assert!(
        text.starts_with("scalerank = 0 and featurecla = "),
        "{text}"
    );
}

#[test]
fn filter_shows_match_count() {
    use vale_app::ui::expr_help::count_matches;
    use vale_app::ui::pad::filter::matches_text;
    let mut h = pad(BOARDS[0]);
    let rivers = vector(&h, "Rivers");
    let layer = h.state().doc.layer(rivers).unwrap();
    let first = layer.features[0].attributes["scalerank"].label().unwrap();
    let text = format!("scalerank = {first}");
    let n = count_matches(layer, &text).unwrap();
    let m = layer.features.len();
    assert!(n > 0 && n <= m);
    set_filter(&mut h, rivers, &text);
    open_rule(&mut h, "Rivers", "Default");
    scroll_to(&mut h, &matches_text(n, m));
    assert!(h.query_by_label(&matches_text(n, m)).is_some());
}

#[test]
fn code_only_filter_opens_in_code() {
    use vale_app::ui::pad::filter::{ADD_CONDITION, CODE_ROW, VIEWS};
    use vale_app::ui::pad::rules::EXPRESSION;
    let mut h = pad(BOARDS[0]);
    let rivers = vector(&h, "Rivers");
    set_filter(&mut h, rivers, "upper(name) = 'X'");
    open_rule(&mut h, "Rivers", "Default");
    scroll_to_control(&mut h, EXPRESSION);
    assert!(control(&h, EXPRESSION).is_some());
    assert!(control(&h, ADD_CONDITION).is_none());
    // In Blocks, the text that is not a condition is a row that opens Code.
    tap(&mut h, VIEWS[0]);
    set_filter(&mut h, rivers, "scalerank < 5 and upper(name) = 'X'");
    let row = format!("{CODE_ROW} 2");
    scroll_to_control(&mut h, &row);
    tap(&mut h, &row);
    assert!(control(&h, EXPRESSION).is_some());
}

/// The text of the filter of the first rule of a layer.
fn filter_text(h: &Pad, id: vale_store::LayerId) -> Option<String> {
    let rule = rules(h, id)[0].clone();
    rule.filter.source().map(|s| s.text().to_owned())
}

#[test]
fn the_line_of_an_expression_field_shows_the_result_or_the_error() {
    use vale_app::ui::expr_help::{MATCHES, NO_MATCH};
    use vale_app::ui::pad::filter::VIEWS;
    use vale_app::ui::pad::rules::EXPRESSION;
    let mut h = pad(BOARDS[0]);
    let rivers = vector(&h, "Rivers");
    let layer = h.state().doc.layer(rivers).unwrap();
    let rank = |i: usize| layer.features[i].attributes["scalerank"].clone();
    let first = rank(0);
    let other = (1..layer.features.len())
        .find(|&i| rank(i) != first)
        .unwrap();
    let other = FeatureRef {
        layer: rivers,
        uuid: layer.features[other].uuid(),
    };
    let first = first.label().unwrap();
    open_rule(&mut h, "Rivers", "Default");
    scroll_to_control(&mut h, VIEWS[1]);
    tap(&mut h, VIEWS[1]);
    scroll_to_control(&mut h, EXPRESSION);

    // With no selection, the line shows the result for the first feature.
    type_text(&mut h, EXPRESSION, &format!("scalerank = {first}"));
    assert!(h.query_by_label(MATCHES).is_some());
    // The selected feature of the layer comes first.
    h.state_mut().feature_selection = vec![other];
    h.run_steps(2);
    assert!(h.query_by_label(NO_MATCH).is_some());
    assert!(h.query_by_label(MATCHES).is_none());

    type_text(&mut h, EXPRESSION, "shout(name)");
    assert!(h.query_by_label("Unknown function 'shout'.").is_some());
    assert!(h.query_by_label(NO_MATCH).is_none());
}

#[test]
fn a_rule_page_sets_the_stroke_and_the_fill() {
    use vale_app::ui::pad::rules::{FILL, FILL_COLOR, STROKE_COLOR, STROKE_WIDTH};
    use vale_style::Color;
    for size in [BOARDS[1], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        let land = vector(&h, "Land");
        let rule = |h: &Pad| rules(h, land)[0].clone();
        open_rule(&mut h, "Land", "Default");

        scroll_to_control(&mut h, STROKE_COLOR);
        tap(&mut h, STROKE_COLOR);
        let hex = format!("{STROKE_COLOR} hex");
        scroll_to_control(&mut h, &hex);
        type_scrolled_keys(&mut h, &hex, "C04020");
        let stroke = rule(&h).stroke().unwrap();
        assert_eq!(stroke.color, Color::rgb(0xc0, 0x40, 0x20), "{what}");

        // The width is in twentieths of a point.
        scrolled_step(&mut h, STROKE_WIDTH, true);
        let width = rule(&h).stroke().unwrap().width;
        assert!(width > stroke.width, "{what}: {width}");
        assert!(
            (width * 20.0 - (width * 20.0).round()).abs() < 1e-9,
            "{what}: {width}"
        );

        scroll_to_control(&mut h, FILL_COLOR);
        tap(&mut h, FILL_COLOR);
        // One picker is open at a time.
        assert!(control(&h, &hex).is_none(), "{what}");
        let hex = format!("{FILL_COLOR} hex");
        scroll_to_control(&mut h, &hex);
        type_scrolled_keys(&mut h, &hex, "E0F0A0");
        assert_eq!(
            rule(&h).fill(),
            Some(Color::rgb(0xe0, 0xf0, 0xa0)),
            "{what}"
        );

        scroll_to_control(&mut h, FILL);
        tap(&mut h, FILL);
        assert_eq!(rule(&h).fill(), None, "{what}");
        assert!(control(&h, FILL_COLOR).is_none(), "{what}");
        tap(&mut h, FILL);
        assert!(rule(&h).fill().is_some(), "{what}");
    }
}

#[test]
fn the_opacity_strip_sets_the_alpha_of_a_fill() {
    use vale_app::ui::pad::rules::FILL_COLOR;
    let mut h = pad(BOARDS[1]);
    let land = vector(&h, "Land");
    let fill = |h: &Pad| rules(h, land)[0].fill().unwrap();
    open_rule(&mut h, "Land", "Default");
    scroll_to_control(&mut h, FILL_COLOR);
    tap(&mut h, FILL_COLOR);
    let before = fill(&h);
    assert_eq!(before.a, 255);
    let strip = format!("{FILL_COLOR} opacity");
    scroll_to_control(&mut h, &strip);
    let rect = control(&h, &strip).unwrap();
    finger_tap(
        &mut h,
        pos2(rect.left() + rect.width() / 4.0, rect.center().y),
    );
    let after = fill(&h);
    assert!((i32::from(after.a) - 64).abs() <= 2, "{after:?}");
    assert_eq!((after.r, after.g, after.b), (before.r, before.g, before.b));
    // The hue strip keeps the alpha.
    let hue = control(&h, &format!("{FILL_COLOR} hue")).unwrap();
    finger_tap(&mut h, pos2(hue.left() + hue.width() / 3.0, hue.center().y));
    let hued = fill(&h);
    assert_ne!((hued.r, hued.g, hued.b), (after.r, after.g, after.b));
    assert_eq!(hued.a, after.a);
    screenshot::save(&mut h, "pad-color-opacity");
}

#[test]
fn the_hex_field_of_a_color_with_alpha_takes_eight_digits() {
    let _clipboard = one_clipboard_test();
    use vale_app::ui::pad::rules::STROKE_COLOR;
    use vale_app::ui::pad::widgets::COPY;
    use vale_style::Color;
    let mut h = pad(BOARDS[1]);
    let land = vector(&h, "Land");
    let stroke = |h: &Pad| rules(h, land)[0].stroke().unwrap().color;
    open_rule(&mut h, "Land", "Default");
    scroll_to_control(&mut h, STROKE_COLOR);
    tap(&mut h, STROKE_COLOR);
    let hex = format!("{STROKE_COLOR} hex");
    let shown = |h: &mut Pad| {
        scroll_to_control(h, &hex);
        long_press(h, &hex);
        tap(h, COPY);
        vale_app::clipboard::get()
    };
    type_scrolled_keys(&mut h, &hex, "C0402080");
    assert_eq!(stroke(&h), Color::rgba(0xc0, 0x40, 0x20, 0x80));
    // The field shows the code with its alpha.
    assert_eq!(shown(&mut h).as_deref(), Some("C0402080"));
    // Six digits give an opaque color, and the field shows six digits.
    type_scrolled_keys(&mut h, &hex, "102030");
    assert_eq!(stroke(&h), Color::rgb(0x10, 0x20, 0x30));
    assert_eq!(shown(&mut h).as_deref(), Some("102030"));
}

#[test]
fn the_width_stepper_of_a_rule_goes_through_the_series_and_takes_a_typed_width() {
    use vale_app::ui::pad::rules::STROKE_WIDTH;
    let mut h = pad(BOARDS[1]);
    let rivers = vector(&h, "Rivers");
    let width = |h: &Pad| rules(h, rivers)[0].stroke().unwrap().width;
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
    open_rule(&mut h, "Rivers", "Default");
    set_number(&mut h, STROKE_WIDTH, "1.5");
    assert!(near(width(&h), 1.5), "{}", width(&h));
    // The buttons go through 1, 2, 5, 10, and so on.
    scrolled_step(&mut h, STROKE_WIDTH, true);
    assert!(near(width(&h), 2.0), "{}", width(&h));
    scrolled_step(&mut h, STROKE_WIDTH, true);
    assert!(near(width(&h), 5.0), "{}", width(&h));
    scrolled_step(&mut h, STROKE_WIDTH, false);
    assert!(near(width(&h), 2.0), "{}", width(&h));
    // A typed width takes the nearest twentieth of a point inside the range.
    set_number(&mut h, STROKE_WIDTH, "3.33");
    assert!(near(width(&h), 3.35), "{}", width(&h));
    set_number(&mut h, STROKE_WIDTH, "50");
    assert!(near(width(&h), 20.0), "{}", width(&h));
}

#[test]
fn a_width_drag_and_a_color_drag_redraw_and_are_one_undo_step_each() {
    use vale_app::ui::pad::rules::{STROKE_COLOR, STROKE_WIDTH};
    let size = BOARDS[1];
    let dir = tempfile::tempdir().unwrap();
    let mut h = saved_pad(size, dir.path());
    let rivers = vector(&h, "Rivers");
    let stroke = |h: &Pad| rules(h, rivers)[0].stroke().unwrap();
    let before = stroke(&h);
    open_rule(&mut h, "Rivers", "Default");
    save_frames(&mut h);
    let saved = saves(&h);

    // A tap on the plus button of the width changes the style that the map
    // draws, and it saves once.
    scrolled_step(&mut h, STROKE_WIDTH, true);
    save_frames(&mut h);
    let width = stroke(&h).width;
    assert!(width > before.width, "{width}");
    assert_eq!(saves(&h), saved + 1);

    // Each step of a drag changes the style that the map draws, and the
    // drag saves once.
    let held = |h: &mut Pad, name: &str, from: f32, to: f32, value: &dyn Fn(&Pad) -> f64| {
        let rect = control(h, name).unwrap();
        let y = rect.center().y;
        let (from, to) = (pos2(rect.left() + from, y), pos2(rect.left() + to, y));
        touch(h, egui::TouchPhase::Start, from, None);
        step_and_save(h, true);
        let mut values = vec![value(h)];
        for i in 1..=6 {
            let at = from.lerp(to, i as f32 / 6.0);
            touch(h, egui::TouchPhase::Move, at, None);
            step_and_save(h, true);
            values.push(value(h));
        }
        touch(h, egui::TouchPhase::End, to, None);
        save_frames(h);
        values.dedup();
        values
    };
    scroll_to_control(&mut h, STROKE_COLOR);
    tap(&mut h, STROKE_COLOR);
    let shade = format!("{STROKE_COLOR} shade");
    scroll_to_control(&mut h, &shade);
    save_frames(&mut h);
    let saved = saves(&h);
    let rect = control(&h, &shade).unwrap();
    let reds = held(&mut h, &shade, 10.0, rect.width() - 10.0, &|h| {
        f64::from(stroke(h).color.r) + 256.0 * f64::from(stroke(h).color.g)
    });
    assert!(reds.len() >= 4, "{reds:?}");
    assert_eq!(saves(&h), saved + 1);

    tap(&mut h, "Undo");
    assert_eq!(stroke(&h).width, width);
    tap(&mut h, "Undo");
    assert_eq!(stroke(&h), before);
}

#[test]
fn a_rule_page_sets_the_line_type_the_dashes_and_the_taper() {
    use vale_app::ui::pad::rules::{
        BRUSH, DASH, DASHED, GAP, LINE, SOLID, STROKE_WIDTH, TAPER, TAPER_START,
    };
    use vale_style::StrokeKind;
    for size in [BOARDS[1], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        let rivers = vector(&h, "Rivers");
        let kind = |h: &Pad| rules(h, rivers)[0].stroke().unwrap().kind;
        open_rule(&mut h, "Rivers", "Default");
        assert_eq!(kind(&h), StrokeKind::Solid, "{what}");
        assert!(control(&h, DASH).is_none(), "{what}");

        scroll_to_control(&mut h, DASHED);
        tap(&mut h, DASHED);
        assert_eq!(kind(&h), StrokeKind::dashed(), "{what}");
        set_number(&mut h, DASH, "12.34");
        set_number(&mut h, GAP, "6.52");
        let StrokeKind::Dashed { dash, gap } = kind(&h) else {
            panic!("{what}: {:?}", kind(&h));
        };
        assert!(
            dash > StrokeKind::DASH && gap > StrokeKind::GAP,
            "{what}: {dash} {gap}"
        );
        assert!(dash <= 20.0 && gap <= 20.0, "{what}: {dash} {gap}");
        let twentieths = |v: f64| (v * 20.0 - (v * 20.0).round()).abs() < 1e-9;
        assert!(twentieths(dash) && twentieths(gap), "{what}: {dash} {gap}");

        scroll_to_control(&mut h, TAPER);
        tap(&mut h, TAPER);
        assert_eq!(kind(&h), StrokeKind::taper(), "{what}");
        assert!(control(&h, DASH).is_none(), "{what}");
        drag_right(&mut h, TAPER_START);
        let StrokeKind::Taper { start } = kind(&h) else {
            panic!("{what}: {:?}", kind(&h));
        };
        assert!(
            start > StrokeKind::TAPER_START && start <= 1.0,
            "{what}: {start}"
        );
        check_controls(&h, &format!("{what} with a taper"));
        screenshot::save(&mut h, &format!("pad-line-{}", size.0));

        scroll_to_control(&mut h, BRUSH);
        tap(&mut h, BRUSH);
        assert_eq!(kind(&h), StrokeKind::Brush, "{what}");
        assert!(control(&h, TAPER_START).is_none(), "{what}");
        tap(&mut h, SOLID);
        assert_eq!(kind(&h), StrokeKind::Solid, "{what}");
    }

    // A polygon outline has no taper.
    let mut h = pad(BOARDS[1]);
    open_rule(&mut h, "Land", "Default");
    scroll_to_control(&mut h, DASHED);
    assert!(control(&h, DASHED).is_some());
    assert!(control(&h, TAPER).is_none());

    // A marker has no line type.
    let mut h = pad(BOARDS[1]);
    open_rule(&mut h, "Places", "Default");
    let width = format!("{STROKE_WIDTH} value");
    scroll_to_control(&mut h, &width);
    assert!(control(&h, &width).is_some());
    assert!(control(&h, LINE).is_none());
}

/// The test symbols that the symbol tests add to the project.
const TEST_SYMBOLS: [&str; 5] = ["city", "pine", "castle", "tree", "mountain"];

/// Adds the test symbols to the project of the harness.
fn add_test_symbols(h: &mut Pad) {
    common::add_symbols(&mut h.state_mut().doc, &TEST_SYMBOLS);
    h.run_steps(2);
}

/// Makes sure that each tile of a symbol and the "Add symbol…" button are at
/// least the touch size.
fn check_tiles(h: &Pad, what: &str) {
    use vale_app::ui::pad::geometry::TOUCH;
    use vale_app::ui::pad::rules::ADD_SYMBOL;
    for name in TEST_SYMBOLS.iter().chain([&ADD_SYMBOL]) {
        if let Some(tile) = control(h, name) {
            assert!(
                tile.width() >= TOUCH && tile.height() >= TOUCH,
                "{what}: {name} {tile:?}"
            );
        }
    }
}

#[test]
fn a_finger_sets_the_shape_and_the_hatch_of_a_marker() {
    use vale_app::ui::pad::furniture::LINE_COLOR;
    use vale_app::ui::pad::rules::{COLOR, FILL, MARKER_SHAPE, SPACING};
    use vale_style::MarkerShape;
    for size in [BOARDS[1], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        let places = vector(&h, "Places");
        let rule = |h: &Pad| rules(h, places)[0].clone();
        open_rule(&mut h, "Places", "Default");
        assert_eq!(rule(&h).marker_shape(), Some(MarkerShape::Circle), "{what}");

        scroll_to_control(&mut h, MARKER_SHAPE);
        tap(&mut h, MARKER_SHAPE);
        assert_eq!(h.state().pad.page, Some(Page::MarkerShape(0)), "{what}");
        check_controls(&h, &format!("{what} on the shape page"));
        tap(&mut h, "Diamond");
        assert_eq!(
            rule(&h).marker_shape(),
            Some(MarkerShape::Diamond),
            "{what}"
        );
        tap(&mut h, "Back to Default");
        assert_eq!(h.state().pad.page, Some(Page::Rule(0)), "{what}");

        scroll_to_control(&mut h, FILL);
        tap(&mut h, FILL);
        assert_eq!(h.state().pad.page, Some(Page::MarkerFill(0)), "{what}");
        assert!(control(&h, COLOR).is_some(), "{what}");
        assert!(control(&h, SPACING).is_none(), "{what}");
        tap(&mut h, "Cross-hatch");
        let hatch = rule(&h)
            .marker_hatch()
            .unwrap_or_else(|| panic!("{what}: no hatch"));
        assert!(hatch.cross, "{what}");
        assert!(rule(&h).fill().is_some(), "{what}");
        assert!(control(&h, COLOR).is_none(), "{what}");
        assert!(control(&h, LINE_COLOR).is_some(), "{what}");
        check_controls(&h, &format!("{what} on the fill page"));
        let before = hatch.spacing;
        scrolled_step(&mut h, SPACING, true);
        let after = rule(&h).marker_hatch().unwrap().spacing;
        assert!(after > before, "{what}: {before} {after}");

        tap(&mut h, "None");
        assert!(rule(&h).fill().is_none() && rule(&h).marker_hatch().is_none());
        assert!(control(&h, LINE_COLOR).is_none(), "{what}");
        tap(&mut h, "Solid");
        assert!(rule(&h).fill().is_some() && rule(&h).marker_hatch().is_none());
    }
}

#[test]
fn a_rule_page_sets_a_point_symbol() {
    use vale_app::ui::pad::rules::{
        FILL, NO_FIELD, ROTATION, SHAPE, SIZE, SIZE_FIELD, STROKE, SVG_SYMBOL, SYMBOL, TINT,
        TINT_COLOR,
    };
    use vale_style::Symbolizer;
    for size in [BOARDS[1], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        add_test_symbols(&mut h);
        let places = vector(&h, "Places");
        let symbol = |h: &Pad| rules(h, places)[0].symbol().cloned();
        open_rule(&mut h, "Places", "Default");
        assert!(symbol(&h).is_none(), "{what}");
        assert!(control(&h, SYMBOL).is_none(), "{what}");

        // "SVG symbol" opens the symbol page, and the rule changes after a pick.
        scroll_to_control(&mut h, SVG_SYMBOL);
        tap(&mut h, SVG_SYMBOL);
        assert_eq!(h.state().pad.page, Some(Page::Symbols(0)), "{what}");
        assert!(symbol(&h).is_none(), "{what}");
        check_tiles(&h, &what);
        check_controls(&h, &format!("{what} with the symbols"));
        tap(&mut h, "pine");
        assert_eq!(h.state().pad.page, Some(Page::Rule(0)), "{what}");
        let s = symbol(&h).unwrap_or_else(|| panic!("{what}: no symbol"));
        assert_eq!(s.symbol, "pine", "{what}");
        assert_eq!(s.size.fallback, 12.0, "{what}");
        assert!(s.tint.is_none(), "{what}");
        // The symbol takes the place of the circle, with its stroke and its fill.
        let r = &rules(&h, places)[0];
        assert!(
            !r.symbolizers
                .iter()
                .any(|s| matches!(s, Symbolizer::Marker { .. })),
            "{what}"
        );
        assert!(control(&h, STROKE).is_none() && control(&h, FILL).is_none());

        // The Symbol row opens the page again, and a tile picks another symbol.
        scroll_to_control(&mut h, SYMBOL);
        tap(&mut h, SYMBOL);
        assert_eq!(h.state().pad.page, Some(Page::Symbols(0)), "{what}");
        screenshot::save(&mut h, &format!("pad-symbols-{}", size.0));
        tap(&mut h, "castle");
        assert_eq!(h.state().pad.page, Some(Page::Rule(0)), "{what}");
        assert_eq!(symbol(&h).unwrap().symbol, "castle", "{what}");

        set_number(&mut h, SIZE, "40.4");
        let s = symbol(&h).unwrap();
        let points = s.size.fallback;
        assert!((points - 40.4).abs() < 1e-9, "{what}: {points}");

        // A tap at the right of the dial turns the symbol to the east.
        scroll_to_control(&mut h, ROTATION);
        // The dial is round, so the whole dial shows when it is as high as
        // it is wide.
        for _ in 0..4 {
            let dial = control(&h, ROTATION).unwrap();
            let hidden = dial.width() - dial.height();
            if hidden < 1.0 {
                break;
            }
            let card = panel(&h, Panel::Layers).unwrap();
            let from = pos2(card.left() + 6.0, card.top() + 112.0);
            drag(&mut h, from, from + vec2(0.0, hidden + 20.0), None);
        }
        let dial = control(&h, ROTATION).unwrap();
        assert_eq!(dial.width(), dial.height(), "{what}");
        let at = pos2(dial.right() - 12.0, dial.center().y);
        touch(&mut h, egui::TouchPhase::Start, at, None);
        h.step();
        touch(&mut h, egui::TouchPhase::End, at, None);
        h.run_steps(3);
        assert_eq!(symbol(&h).unwrap().rotation.fallback, 90.0, "{what}");

        scroll_to_control(&mut h, SIZE_FIELD);
        tap(&mut h, SIZE_FIELD);
        scroll_to_control(&mut h, "pop_max");
        check_controls(&h, &format!("{what} with the fields"));
        tap(&mut h, "pop_max");
        let s = symbol(&h).unwrap();
        assert_eq!(s.size.source().map(|s| s.text()), Some("pop_max"), "{what}");
        assert_eq!(s.size.fallback, points, "{what}");
        assert!(s.rotation.is_constant(), "{what}");
        assert!(control(&h, "pop_max").is_none(), "{what}");
        scroll_to_control(&mut h, SIZE_FIELD);
        tap(&mut h, SIZE_FIELD);
        scroll_to_control(&mut h, NO_FIELD);
        tap(&mut h, NO_FIELD);
        assert!(symbol(&h).unwrap().size.is_constant(), "{what}");

        // The tint switch turns the tint on and off.
        scroll_to_control(&mut h, TINT);
        assert!(control(&h, TINT_COLOR).is_none(), "{what}");
        tap(&mut h, TINT);
        assert!(symbol(&h).unwrap().tint.is_some(), "{what}");
        assert!(control(&h, TINT_COLOR).is_some(), "{what}");
        tap(&mut h, TINT);
        assert!(symbol(&h).unwrap().tint.is_none(), "{what}");
        check_controls(&h, &what);
        scroll_to_control(&mut h, SYMBOL);
        screenshot::save(&mut h, &format!("pad-symbol-{}", size.0));

        scroll_to_control(&mut h, SHAPE);
        tap(&mut h, SHAPE);
        assert!(symbol(&h).is_none(), "{what}");
        assert!(rules(&h, places)[0].fill().is_some(), "{what}");
    }
}

#[test]
fn the_symbol_page_of_an_empty_library_adds_an_svg_file() {
    use vale_app::ui::pad::geometry::TOUCH;
    use vale_app::ui::pad::rules::{ADD_SYMBOL, NO_SYMBOLS, SVG_SYMBOL, SYMBOL};
    let dir = tempfile::tempdir().unwrap();
    for size in [BOARDS[1], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        let places = vector(&h, "Places");
        let symbol = |h: &Pad| rules(h, places)[0].symbol().cloned();
        open_rule(&mut h, "Places", "Default");
        scroll_to_control(&mut h, SVG_SYMBOL);
        tap(&mut h, SVG_SYMBOL);
        assert_eq!(h.state().pad.page, Some(Page::Symbols(0)), "{what}");
        assert!(h.query_by_label(NO_SYMBOLS).is_some(), "{what}");
        let button = control(&h, ADD_SYMBOL).unwrap_or_else(|| panic!("{what}: no button"));
        assert!(button.height() >= TOUCH, "{what}: {button:?}");
        let card = panel(&h, Panel::Layers).unwrap();
        assert!(
            button.left() >= card.left() + 8.0 && button.right() <= card.right() - 8.0,
            "{what}: {button:?} {card:?}"
        );
        check_controls(&h, &format!("{what} with no symbols"));
        screenshot::save(&mut h, &format!("pad-symbols-empty-{}", size.0));
        tap(&mut h, ADD_SYMBOL);
        assert!(
            h.state().actions.contains(&Action::AddSymbolDialog),
            "{what}"
        );
        assert!(symbol(&h).is_none(), "{what}");

        // The file that the user picks shows as a tile, with a note.
        let file = dir.path().join("tree.svg");
        std::fs::write(&file, common::svg("tree")).unwrap();
        h.state_mut().run_action(Action::AddSymbol(file));
        h.run_steps(3);
        assert!(h.query_by_label("Added symbol tree").is_some(), "{what}");
        assert!(h.query_by_label(NO_SYMBOLS).is_none(), "{what}");
        assert!(control(&h, "tree").is_some(), "{what}");
        assert!(symbol(&h).is_none(), "{what}");

        // The back button keeps the circle.
        tap(&mut h, "Back to Default");
        assert_eq!(h.state().pad.page, Some(Page::Rule(0)), "{what}");
        assert!(symbol(&h).is_none(), "{what}");
        assert!(control(&h, SYMBOL).is_none(), "{what}");
    }
}

#[test]
fn a_circle_size_step_is_one_undo_step() {
    use vale_app::ui::pad::rules::{CIRCLE_SIZES, SIZE};
    use vale_style::Symbolizer;
    let dir = tempfile::tempdir().unwrap();
    let mut h = saved_pad(BOARDS[1], dir.path());
    let places = vector(&h, "Places");
    let radius = |h: &Pad| {
        rules(h, places)[0]
            .symbolizers
            .iter()
            .find_map(|s| match s {
                Symbolizer::Marker { radius, .. } => Some(radius.fallback),
                _ => None,
            })
    };
    let before = radius(&h).unwrap();
    open_rule(&mut h, "Places", "Default");
    save_frames(&mut h);
    let saved = saves(&h);

    scrolled_step(&mut h, SIZE, true);
    save_frames(&mut h);
    let after = radius(&h).unwrap();
    assert!(after > before, "{after}");
    assert!(2.0 * after <= CIRCLE_SIZES.1, "{after}");
    assert_eq!(saves(&h), saved + 1);

    tap(&mut h, "Undo");
    assert_eq!(radius(&h), Some(before));

    // The stepper goes down through 1, 2, and 5 to the smallest size.
    for _ in 0..20 {
        if 2.0 * radius(&h).unwrap() <= CIRCLE_SIZES.0 + 1e-9 {
            break;
        }
        scrolled_step(&mut h, SIZE, false);
    }
    let smallest = 2.0 * radius(&h).unwrap();
    assert!((smallest - CIRCLE_SIZES.0).abs() < 1e-9, "{smallest}");
    assert!(CIRCLE_SIZES.0 <= 0.1);
}

#[test]
fn a_rule_page_adds_a_scatter() {
    use vale_app::ui::pad::rules::{
        ROTATION_VARIATION, SCATTER, SIZE, SIZE_VARIATION, SPACING, SYMBOL, TINT,
    };
    for size in [BOARDS[1], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        add_test_symbols(&mut h);
        let land = vector(&h, "Land");
        let scatter = |h: &Pad| rules(h, land)[0].scatter().cloned();
        open_rule(&mut h, "Land", "Default");
        scroll_to_control(&mut h, SCATTER);
        assert!(control(&h, SPACING).is_none(), "{what}");

        // The switch opens the symbol page, and the back button adds no scatter.
        tap(&mut h, SCATTER);
        assert_eq!(h.state().pad.page, Some(Page::Symbols(0)), "{what}");
        assert!(scatter(&h).is_none(), "{what}");
        tap(&mut h, "Back to Default");
        assert_eq!(h.state().pad.page, Some(Page::Rule(0)), "{what}");
        assert!(scatter(&h).is_none(), "{what}");
        scroll_to_control(&mut h, SCATTER);
        assert!(control(&h, SPACING).is_none(), "{what}");

        // A pick adds the scatter.
        tap(&mut h, SCATTER);
        check_tiles(&h, &what);
        tap(&mut h, "tree");
        assert_eq!(h.state().pad.page, Some(Page::Rule(0)), "{what}");
        let s = scatter(&h).unwrap_or_else(|| panic!("{what}: no scatter"));
        assert_eq!(s.symbol, "tree", "{what}");
        assert_eq!((s.size.fallback, s.spacing.fallback), (8.0, 10.0), "{what}");
        assert!(s.tint.is_none(), "{what}");

        scroll_to_control(&mut h, SYMBOL);
        tap(&mut h, SYMBOL);
        tap(&mut h, "pine");
        assert_eq!(scatter(&h).unwrap().symbol, "pine", "{what}");

        set_number(&mut h, SPACING, "50");
        let s = scatter(&h).unwrap();
        let spacing = s.spacing.fallback;
        assert!(spacing > 10.0 && spacing <= 100.0, "{what}: {spacing}");
        assert!(spacing >= s.size.fallback, "{what}");

        // A larger size pushes the spacing up with it.
        set_number(&mut h, SIZE, "30");
        let s = scatter(&h).unwrap();
        assert!(s.size.fallback > 8.0 && s.size.fallback <= 48.0, "{what}");
        assert!(s.spacing.fallback >= s.size.fallback, "{what}");

        drag_right(&mut h, SIZE_VARIATION);
        drag_right(&mut h, ROTATION_VARIATION);
        let s = scatter(&h).unwrap();
        assert!(s.size_jitter > 0.2 && s.size_jitter <= 0.5, "{what}");
        assert!(
            s.rotation_jitter > 0.0 && s.rotation_jitter <= 180.0,
            "{what}"
        );
        scroll_to_control(&mut h, TINT);
        check_controls(&h, &what);
        screenshot::save(&mut h, &format!("pad-scatter-{}", size.0));

        scroll_to_control(&mut h, SCATTER);
        tap(&mut h, SCATTER);
        assert!(scatter(&h).is_none(), "{what}");
        assert!(rules(&h, land)[0].fill().is_some(), "{what}");
    }
}

#[test]
fn a_spacing_step_is_one_undo_step() {
    use vale_app::ui::pad::rules::{SCATTER, SPACING};
    let dir = tempfile::tempdir().unwrap();
    let mut h = saved_pad(BOARDS[1], dir.path());
    add_test_symbols(&mut h);
    let land = vector(&h, "Land");
    let spacing = |h: &Pad| rules(h, land)[0].scatter().map(|s| s.spacing.fallback);
    open_rule(&mut h, "Land", "Default");
    scroll_to_control(&mut h, SCATTER);
    tap(&mut h, SCATTER);
    tap(&mut h, "tree");
    save_frames(&mut h);
    let saved = saves(&h);
    scrolled_step(&mut h, SPACING, true);
    save_frames(&mut h);
    assert_eq!(spacing(&h), Some(11.0));
    assert_eq!(saves(&h), saved + 1);

    // One undo takes back the step, and the next takes back the scatter.
    tap(&mut h, "Undo");
    assert_eq!(spacing(&h), Some(10.0));
    tap(&mut h, "Undo");
    assert_eq!(spacing(&h), None);
}

#[test]
fn a_start_width_drag_is_one_undo_step() {
    use vale_app::ui::pad::rules::{TAPER, TAPER_START};
    use vale_style::StrokeKind;
    let dir = tempfile::tempdir().unwrap();
    let mut h = saved_pad(BOARDS[1], dir.path());
    let rivers = vector(&h, "Rivers");
    let kind = |h: &Pad| rules(h, rivers)[0].stroke().unwrap().kind;
    open_rule(&mut h, "Rivers", "Default");
    scroll_to_control(&mut h, TAPER);
    tap(&mut h, TAPER);
    scroll_to_control(&mut h, TAPER_START);
    save_frames(&mut h);
    let saved = saves(&h);
    let rect = control(&h, TAPER_START).unwrap();
    let y = rect.center().y;
    let (from, to) = (pos2(rect.left() + 100.0, y), pos2(rect.left() + 200.0, y));
    touch(&mut h, egui::TouchPhase::Start, from, None);
    step_and_save(&mut h, true);
    let mut starts = Vec::new();
    for i in 1..=6 {
        touch(
            &mut h,
            egui::TouchPhase::Move,
            from.lerp(to, i as f32 / 6.0),
            None,
        );
        step_and_save(&mut h, true);
        if let StrokeKind::Taper { start } = kind(&h) {
            starts.push(start);
        }
    }
    touch(&mut h, egui::TouchPhase::End, to, None);
    save_frames(&mut h);
    starts.dedup();
    assert!(starts.len() >= 4, "{starts:?}");
    assert!(starts.is_sorted(), "{starts:?}");
    assert_eq!(saves(&h), saved + 1);

    // One undo takes back the whole drag, and the next takes back the taper.
    tap(&mut h, "Undo");
    assert_eq!(kind(&h), StrokeKind::taper());
    tap(&mut h, "Undo");
    assert_eq!(kind(&h), StrokeKind::Solid);
}

#[test]
fn the_style_editor_opens_on_the_globe_and_the_flat_view_and_fits_a_finger() {
    use vale_app::ui::pad::rules::{ADD_RULE, FILL_COLOR, MIN_SCALE, STROKE, STROKE_COLOR};
    for size in BOARDS {
        for view in WORLD_VIEWS {
            let what = format!("{size:?} {view:?}");
            let mut h = pad(size);
            h.state_mut().globe.world_view = view;
            h.run_steps(2);
            open_panel(&mut h, "Layers");
            tap(&mut h, "Open Land");
            scroll_to_control(&mut h, ADD_RULE);
            assert!(control(&h, "Default").is_some(), "{what}");
            check_controls(&h, &format!("{what} with the Style section"));
            tap(&mut h, "Default");
            for name in [STROKE, STROKE_COLOR, FILL_COLOR, "Back to Land"] {
                scroll_to_control(&mut h, name);
                assert!(control(&h, name).is_some(), "{what}: {name}");
            }
            check_controls(&h, &format!("{what} on the page of a rule"));
            scroll_to_control(&mut h, FILL_COLOR);
            tap(&mut h, FILL_COLOR);
            check_controls(&h, &format!("{what} with the color picker"));
            scroll_to_control(&mut h, MIN_SCALE);
            tap(&mut h, MIN_SCALE);
            scroll_to_control(&mut h, &format!("{MIN_SCALE} 5"));
            check_controls(&h, &format!("{what} with the keypad"));
        }
    }
}

/// Taps a control of the form of a new raster layer, after drags on the
/// margin at the left of the rows that scroll the control into the card.
fn form_tap(h: &mut Pad, name: &str) {
    for _ in 0..8 {
        if control(h, name).is_some() {
            break;
        }
        let close = control(h, &format!("Close {}", raster_form::TITLE)).unwrap();
        let width = h.state().pad.rects.unwrap().screen.width();
        let from = pos2(width - close.right() + 4.0, close.bottom() + 260.0);
        drag(h, from, from - vec2(0.0, 200.0), None);
    }
    tap(h, name);
}

/// Writes a 2:1 greyscale PNG with three bands of levels from west to east:
/// `low`, the level halfway, and `high`. `wide`: 16-bit, or else 8-bit.
fn band_image(name: &str, low: u16, high: u16, wide: bool) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/app/test-raster-file");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let (width, height) = (96u32, 48u32);
    let levels = [low, low + (high - low) / 2, high];
    let level = |x: u32| levels[(x * 3 / width) as usize];
    if wide {
        let pixels: Vec<u16> = (0..width * height).map(|i| level(i % width)).collect();
        let image = image::ImageBuffer::<image::Luma<u16>, _>::from_raw(width, height, pixels);
        image.unwrap().save(&path).unwrap();
    } else {
        let pixels: Vec<u8> = (0..width * height)
            .map(|i| level(i % width) as u8)
            .collect();
        let image = image::ImageBuffer::<image::Luma<u8>, _>::from_raw(width, height, pixels);
        image.unwrap().save(&path).unwrap();
    }
    path
}

/// All texels of the heightmap of a raster layer.
fn raster_texels(h: &Pad, id: RasterId) -> Vec<u16> {
    let map = h.state().globe.raster_map(id).unwrap();
    let n = map.face_size();
    let mut out = Vec::new();
    for face in 0..vale_terrain::FACES {
        let full = vale_terrain::TexelRect {
            x0: 0,
            y0: 0,
            x1: n,
            y1: n,
        };
        map.read_rect(face, full, &mut out);
    }
    out
}

/// Opens the form with a small face size, picks a preset, and reads an
/// image as the source.
fn form_with_image(h: &mut Pad, preset: &str, path: std::path::PathBuf) {
    h.state_mut().form_face_size = Some(64);
    open_raster_form(h);
    tap(h, preset);
    tap(h, raster_form::SOURCES[1]);
    // A form with no image opens the file picker at once.
    let actions = std::mem::take(&mut h.state_mut().actions);
    assert_eq!(actions, [Action::RasterImageDialog]);
    h.state_mut().run_action(Action::RasterImage(path));
    h.run_steps(2);
}

#[test]
fn the_form_reads_the_darkest_and_the_brightest_level_of_an_image() {
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = empty_pad(size);
        open_panel(&mut h, "Layers");
        form_with_image(
            &mut h,
            "Custom",
            band_image("levels16.png", 1000, 60000, true),
        );
        let form = h.state().raster_form.clone().unwrap();
        let probe = form.probe().unwrap();
        assert_eq!((probe.levels, probe.image.bits), ((1000, 60000), 16));
        check_controls(&h, &format!("{what} with the levels of an image"));
        for name in [
            raster_form::IMAGE,
            raster_form::DARKEST_VALUE,
            raster_form::BRIGHTEST_VALUE,
        ] {
            assert!(control(&h, name).is_some(), "{what}: {name}");
        }
        // The tap on the image row opens the file picker again.
        tap(&mut h, raster_form::IMAGE);
        assert_eq!(h.state().actions, [Action::RasterImageDialog]);
        h.state_mut().actions.clear();

        // An 8-bit image shows its levels in 8-bit values.
        h.state_mut().run_action(Action::RasterImage(band_image(
            "levels8.png",
            10,
            200,
            false,
        )));
        h.run_steps(2);
        let form = h.state().raster_form.clone().unwrap();
        let probe = form.probe().unwrap();
        assert_eq!((probe.levels, probe.image.bits), ((10 * 257, 200 * 257), 8));
        assert_eq!(raster_form::level_text(probe.levels.0, 8), "10 of 255");
        assert_eq!(raster_form::level_text(1000, 16), "1,000 of 65,535");
    }
}

#[test]
fn an_image_makes_a_layer_of_each_kind_with_its_levels_at_the_ends_of_the_range() {
    let mut h = empty_pad(BOARDS[0]);
    open_panel(&mut h, "Layers");
    let path = band_image("kinds.png", 1000, 60000, true);
    for (index, preset) in PRESETS.iter().enumerate() {
        form_with_image(&mut h, preset.name, path.clone());
        let form = h.state().raster_form.clone().unwrap();
        let (min, max) = preset.range;
        assert_eq!(
            (form.darkest.as_str(), form.brightest.as_str()),
            (&*min.to_string(), &*max.to_string())
        );
        form_tap(&mut h, raster_form::ADD);
        h.run_steps(2);
        assert!(h.state().raster_form.is_none(), "{}", preset.name);
        let raster = h.state().doc.project.rasters()[index].clone();
        assert_eq!(
            (raster.unit.as_str(), raster.value_min, raster.value_max),
            (preset.unit, min, max)
        );
        let texels = raster_texels(&h, raster.id);
        // The darkest level is the minimum, and the brightest level is the
        // maximum.
        assert_eq!(texels.iter().min(), Some(&0), "{}", preset.name);
        assert_eq!(texels.iter().max(), Some(&65535), "{}", preset.name);
        assert!(texels.contains(&32768), "{}", preset.name);
        assert_eq!(h.state().pad.layer, Some(LayerRef::Raster(raster.id)));
    }
}

#[test]
fn the_mapped_values_scale_the_levels_of_the_image() {
    let mut h = empty_pad(BOARDS[3]);
    open_panel(&mut h, "Layers");
    form_with_image(
        &mut h,
        "Precipitation",
        band_image("scale.png", 1000, 61000, true),
    );
    let form = h.state_mut().raster_form.as_mut().unwrap();
    form.darkest = "0".into();
    form.brightest = "3,000".into();
    h.run_steps(1);
    form_tap(&mut h, raster_form::ADD);
    h.run_steps(2);
    let raster = h.state().doc.project.rasters()[0].clone();
    assert_eq!((raster.value_min, raster.value_max), (0.0, 4000.0));
    let texels = raster_texels(&h, raster.id);
    // 0 mm, 1,500 mm, and 3,000 mm in a range of 0 to 4,000 mm.
    let level = |mm: f64| (mm / 4000.0 * 65535.0).round() as u16;
    assert_eq!(texels.iter().min(), Some(&level(0.0)));
    assert_eq!(texels.iter().max(), Some(&level(3000.0)));
    assert!(texels.contains(&level(1500.0)));

    // A value past the range gets the end of the range.
    form_with_image(
        &mut h,
        "Precipitation",
        band_image("scale.png", 1000, 61000, true),
    );
    h.state_mut().raster_form.as_mut().unwrap().brightest = "5000".into();
    h.run_steps(1);
    form_tap(&mut h, raster_form::ADD);
    h.run_steps(2);
    let raster = h.state().doc.project.rasters()[1].clone();
    let texels = raster_texels(&h, raster.id);
    assert_eq!(texels.iter().max(), Some(&65535));
    assert!(texels.contains(&level(2500.0)));
}

#[test]
fn a_file_that_is_not_an_image_makes_no_layer() {
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = empty_pad(size);
        open_panel(&mut h, "Layers");
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/app/test-raster-file");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notes.png");
        std::fs::write(&path, "these bytes are not an image").unwrap();
        form_with_image(&mut h, "Custom", path);
        let note = h.state().raster_form.as_ref().unwrap().note().unwrap();
        assert_eq!(note, "notes is not a PNG or TIFF image", "{what}");
        assert!(control(&h, raster_form::DARKEST_VALUE).is_none(), "{what}");
        // The add button is off, and the form stays open.
        assert!(
            h.get_by_label(raster_form::ADD)
                .accesskit_node()
                .is_disabled(),
            "{what}"
        );
        assert!(h.state().raster_form.is_some(), "{what}");
        assert!(h.state().doc.project.rasters().is_empty(), "{what}");

        // The empty source makes a layer again.
        tap(&mut h, raster_form::SOURCES[0]);
        assert!(h.state().actions.is_empty(), "{what}");
        form_tap(&mut h, raster_form::ADD);
        assert_eq!(h.state().doc.project.rasters().len(), 1, "{what}");
    }
}

#[test]
fn the_menu_of_the_new_layers_asks_for_the_file_of_an_image_layer() {
    for size in BOARDS {
        let what = format!("{size:?}");
        let mut h = pad(size);
        open_panel(&mut h, "Layers");
        tap(&mut h, ADD_LAYER);
        check_controls(&h, &format!("{what} with the menu of the new layers"));
        let row = control(&h, NEW_IMAGE).unwrap_or_else(|| panic!("{what}: no row"));
        let menu = h.state().pad.add_rect.unwrap();
        assert!(menu.contains_rect(row), "{what}: {row:?}");
        assert!(h.state().actions.is_empty());
        tap(&mut h, NEW_IMAGE);
        assert_eq!(h.state().actions, [Action::ImageLayerDialog], "{what}");
        assert!(!h.state().pad.add, "{what}");
    }
}

#[test]
fn the_menu_of_the_new_layers_imports_a_geojson_file() {
    for size in BOARDS {
        let what = format!("{size:?}");
        let mut h = pad(size);
        open_panel(&mut h, "Layers");
        tap(&mut h, ADD_LAYER);
        check_controls(&h, &format!("{what} with the menu of the new layers"));
        let row = control(&h, NEW_GEOJSON).unwrap_or_else(|| panic!("{what}: no row"));
        let menu = h.state().pad.add_rect.unwrap();
        assert!(menu.contains_rect(row), "{what}: {row:?}");
        tap(&mut h, NEW_GEOJSON);
        assert_eq!(h.state().actions, [Action::GeoJsonDialog], "{what}");
        assert!(!h.state().pad.add, "{what}");
    }
}

#[test]
fn the_menu_of_the_new_layers_puts_the_rows_that_read_a_file_under_import() {
    let mut h = empty_pad(BOARDS[0]);
    open_panel(&mut h, "Layers");
    tap(&mut h, ADD_LAYER);
    let top = |name: &str| control(&h, name).unwrap_or_else(|| panic!("{name}")).top();
    let heading = |name: &str| h.get_by_label(name).rect().top();
    let new = heading(vale_app::ui::pad::NEW_SECTION);
    let import = heading(vale_app::ui::pad::IMPORT_SECTION);
    for name in ["Lines", "Points", "Polygons", NEW_RASTER] {
        assert!(new < top(name) && top(name) < import, "{name}");
    }
    for name in [NEW_IMAGE, NEW_GEOJSON, NEW_HEIGHTMAP] {
        assert!(import < top(name), "{name}");
    }
    screenshot::save(&mut h, "pad-add-layer");
}

#[test]
fn a_geojson_file_adds_a_vector_layer_for_each_kind_at_its_longitude_and_latitude() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("coast.geojson");
    let json = r#"{"type":"FeatureCollection","features":[
      {"type":"Feature","properties":{"name":"Port"},"geometry":{"type":"Point","coordinates":[-120.5,45.25]}},
      {"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[0,0],[10,5]]}}
    ]}"#;
    std::fs::write(&file, json).unwrap();
    let mut h = pad(BOARDS[0]);
    open_panel(&mut h, "Layers");
    let before = h.state().doc.project.layers().len();
    h.state_mut().run_action(Action::ImportGeoJson(file));
    h.run_steps(2);
    let project = &h.state().doc.project;
    assert_eq!(project.layers().len(), before + 2);
    // The panel of the top new layer opens.
    let top = h.state().pad.layer;
    assert_eq!(h.state().selected_layer, top);
    let Some(LayerRef::Vector(id)) = top else {
        panic!("{top:?}");
    };
    let points = project.layer(id).unwrap();
    assert_eq!(points.kind, LayerKind::Vector(GeometryKind::Point));
    let geometry = &points.features[0].geometry;
    assert_eq!(
        *geometry,
        vale_store::Geometry::Points(vec![[-120.5, 45.25]])
    );
}

#[test]
fn the_layers_panel_tells_of_a_geojson_file_that_fails() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("broken.geojson");
    std::fs::write(&file, "{ not json").unwrap();
    let mut h = pad(BOARDS[0]);
    open_panel(&mut h, "Layers");
    let before = h.state().doc.project.layers().len();
    h.state_mut().run_action(Action::ImportGeoJson(file));
    h.run_steps(2);
    assert_eq!(h.state().doc.project.layers().len(), before);
    assert!(
        h.query_by_label_contains("Cannot import broken.geojson")
            .is_some()
    );
}

/// The straight RGBA8 pixel of level 0 of an image of a project with no
/// file, at a pixel of the image.
fn image_pixel(h: &Pad, id: vale_store::ImageId, x: u32, y: u32) -> [u8; 4] {
    let tile = vale_store::IMAGE_TILE_SIZE;
    let key = (id, 0, x / tile, y / tile);
    let png = &h.state().image_tiles[&key];
    let (w, _, rgba) = vale_store::decode_tile(png).unwrap();
    let at = (((y % tile) * w + x % tile) * 4) as usize;
    rgba[at..at + 4].try_into().unwrap()
}

#[test]
fn an_image_layer_comes_from_a_png_a_tiff_or_a_psd_with_alpha() {
    let dir = tempfile::tempdir().unwrap();
    let picture = images::half_red(512, 256);
    let files = [
        images::write_png(&dir.path().join("plates.png"), &picture),
        images::write_tiff(&dir.path().join("plates.tif"), &picture),
        images::write_psd(&dir.path().join("plates.psd"), &picture),
    ];
    let mut h = pad(BOARDS[0]);
    open_panel(&mut h, "Layers");
    for (at, file) in files.into_iter().enumerate() {
        let what = file.display().to_string();
        h.state_mut().run_action(Action::ImportImageLayer(file));
        h.run_steps(2);
        let images = h.state().doc.project.images();
        assert_eq!(images.len(), at + 1, "{what}");
        let image = &images[at];
        let id = image.id;
        assert_eq!((image.width, image.height), (512, 256), "{what}");
        let layer = h.state().doc.project.image_layer(id).unwrap();
        assert_eq!(layer.name, "plates", "{what}");
        let style = vale_style::LayerStyle::from_json(layer.style.as_deref().unwrap()).unwrap();
        assert!(style.image_symbolizer().is_some(), "{what}");
        assert_eq!(image_pixel(&h, id, 100, 50)[3], 0, "{what}");
        assert_eq!(image_pixel(&h, id, 400, 50), [255, 0, 0, 255], "{what}");
        // The panel of the new layer opens with the result.
        assert_eq!(h.state().pad.layer, Some(LayerRef::Image(id)), "{what}");
        let note = h.query_by_label_contains("Imported plates.");
        assert!(note.is_some(), "{what}");
    }
}

#[test]
fn the_panel_of_an_image_layer_works_with_a_finger() {
    use vale_store::BlendMode;
    let dir = tempfile::tempdir().unwrap();
    let file = images::write_png(&dir.path().join("plates.png"), &images::half_red(512, 256));
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        h.state_mut()
            .run_action(Action::ImportImageLayer(file.clone()));
        let id = h.state().doc.project.images()[0].id;
        h.state_mut().pad.layer = None;
        open_panel(&mut h, "Layers");
        tap(&mut h, "Open plates");
        assert_eq!(h.state().pad.layer, Some(LayerRef::Image(id)), "{what}");
        assert_eq!(h.state().selected_layer, Some(LayerRef::Image(id)));
        check_controls(&h, &format!("{what} with the panel of an image layer"));
        let card = panel(&h, Panel::Layers).unwrap();
        for name in [LAYER_NAME, OPACITY, raster::BLEND] {
            let rect = control(&h, name).unwrap_or_else(|| panic!("{what}: no {name}"));
            assert!(card.contains_rect(rect), "{what}: {name} is {rect:?}");
        }
        let texts: Vec<String> = painted_texts(&h).into_iter().map(|t| t.0).collect();
        assert!(
            texts.iter().any(|t| t == "512 × 256 px"),
            "{what}: {texts:?}"
        );
        // An image layer has no color stops and no values.
        assert!(control(&h, raster::COLOR_STOPS).is_none(), "{what}");
        assert!(!texts.iter().any(|t| t == "Values"), "{what}: {texts:?}");
        // The brush paints no image.
        assert!(!h.state().globe.paintable, "{what}");

        // A drag to the left lowers the opacity in whole percents.
        let slider = control(&h, OPACITY).unwrap();
        drag(
            &mut h,
            slider.center(),
            slider.center() - vec2(40.0, 0.0),
            None,
        );
        let opacity = h.state().doc.project.image(id).unwrap().opacity;
        assert!(opacity > 0.0 && opacity < 1.0, "{what}: {opacity}");
        assert_eq!((opacity * 100.0).round() / 100.0, opacity, "{what}");

        tap(&mut h, raster::BLEND);
        assert_eq!(h.state().pad.page, Some(Page::Blend), "{what}");
        check_controls(&h, &format!("{what} with the blend page of an image layer"));
        tap(&mut h, "Blend mode Multiply");
        let image = h.state().doc.project.image(id).unwrap();
        assert_eq!(image.blend, BlendMode::Multiply, "{what}");
        let layer = h.state().doc.project.image_layer(id).unwrap();
        let style = vale_style::LayerStyle::from_json(layer.style.as_deref().unwrap()).unwrap();
        let look = style.image_symbolizer().unwrap();
        assert_eq!((look.opacity, look.blend), (opacity, BlendMode::Multiply));
        tap(&mut h, "Back to plates");
        assert_eq!(h.state().pad.page, None, "{what}");
    }
}

#[test]
fn the_layers_panel_shows_why_an_image_layer_did_not_import() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.png");
    std::fs::write(&file, b"not an image").unwrap();
    let mut h = pad(BOARDS[0]);
    open_panel(&mut h, "Layers");
    h.state_mut().run_action(Action::ImportImageLayer(file));
    h.run_steps(2);
    assert!(h.state().doc.project.images().is_empty());
    assert!(h.query_by_label_contains("notes").is_some());
}

/// The label text of layer `id`.
fn label_text(h: &Pad, id: vale_store::LayerId) -> String {
    h.state().doc.entry(id).unwrap().style.label.text.clone()
}

/// Opens the Label text page of Places, and gives the token field the
/// focus with a tap at its bottom right, away from the chips.
fn open_label_text(h: &mut Pad) -> vale_store::LayerId {
    use vale_app::ui::pad::label_text::LABEL_TEXT;
    let places = vector(h, "Places");
    open_panel(h, "Layers");
    tap(h, "Open Places");
    scroll_to_control(h, LABEL_TEXT);
    tap(h, LABEL_TEXT);
    assert_eq!(h.state().pad.page, Some(Page::LabelText));
    check_controls(h, "the Label text page");
    let at = control(h, LABEL_TEXT).unwrap().right_bottom() - vec2(16.0, 8.0);
    touch(h, egui::TouchPhase::Start, at, None);
    h.step();
    touch(h, egui::TouchPhase::End, at, None);
    h.run_steps(3);
    places
}

/// Taps the control `name` of the bar above the keyboard, and scrolls the
/// bar to it first.
fn tap_bar(h: &mut Pad, name: &str) {
    use vale_app::ui::new_feature::keyboard_top;
    use vale_app::ui::pad::label_text::BAR;
    for i in 0..16 {
        if control(h, name).is_some_and(|r| r.width() >= 40.0) {
            break;
        }
        let screen = h.state().pad.rects.unwrap().screen;
        let y = keyboard_top(screen) - BAR / 2.0;
        // Forward first, and then back.
        let by = vec2(screen.width() * 0.5, 0.0);
        let (from, to) = match i < 8 {
            true => (
                pos2(screen.right() - 30.0, y),
                pos2(screen.right() - 30.0, y) - by,
            ),
            false => (
                pos2(screen.left() + 30.0, y),
                pos2(screen.left() + 30.0, y) + by,
            ),
        };
        drag(h, from, to, None);
    }
    tap(h, name);
}

#[test]
fn bar_inserts_field_chip() {
    use vale_app::ui::pad::label_text::{GROUP, LINE_BREAK, insert_name};
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        let places = open_label_text(&mut h);
        assert_eq!(label_text(&h, places), "name", "{what}");
        assert!(control(&h, GROUP).is_some(), "{what}");
        assert!(control(&h, LINE_BREAK).is_some(), "{what}");
        tap_bar(&mut h, &insert_name("adm0name"));
        assert_eq!(label_text(&h, places), "concat(name, adm0name)", "{what}");
        screenshot::save(&mut h, &format!("pad-label-text-bar-{}", size.0));
        tap(&mut h, "Back to Places");
        assert_eq!(h.state().pad.page, None, "{what}");
    }
}

#[test]
fn typed_text_stays_text() {
    let mut h = pad(BOARDS[0]);
    let places = open_label_text(&mut h);
    h.event(egui::Event::Text(" km".into()));
    h.run_steps(2);
    assert_eq!(label_text(&h, places), "concat(name, ' km')");
    // A word that is the name of a field stays text.
    h.event(egui::Event::Text(" name".into()));
    h.run_steps(2);
    assert_eq!(label_text(&h, places), "concat(name, ' km name')");
}

#[test]
fn group_writes_optional_text() {
    use vale_app::ui::pad::label_text::{GROUP, LINE_BREAK, insert_name};
    let mut h = pad(BOARDS[0]);
    let places = open_label_text(&mut h);
    tap_bar(&mut h, LINE_BREAK);
    assert_eq!(label_text(&h, places), "concat(name, '\n')");
    h.event(egui::Event::Text("Pop ".into()));
    h.run_steps(2);
    tap_bar(&mut h, &insert_name("pop_max"));
    tap_bar(&mut h, GROUP);
    assert_eq!(
        label_text(&h, places),
        "concat(name, '\n', 'Pop ' || pop_max)"
    );
    screenshot::save(&mut h, "pad-label-text-group");
}

#[test]
fn label_preview_has_no_marker() {
    fn visit(shape: &egui::Shape, out: &mut Vec<egui::Shape>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| visit(s, out)),
            s => out.push(s.clone()),
        }
    }
    let mut h = pad(BOARDS[0]);
    open_label_text(&mut h);
    let area = h.get_by_label("Vatican City").rect();
    let mut shapes = Vec::new();
    for clipped in &h.output().shapes {
        visit(&clipped.shape, &mut shapes);
    }
    let inside = |s: &egui::Shape| area.intersects(s.visual_bounding_rect());
    let circles = (shapes.iter()).filter(|s| inside(s) && matches!(s, egui::Shape::Circle(_)));
    assert_eq!(circles.count(), 0);
    // The label is a picture in the look of the labels.
    let picture = (shapes.iter()).filter(|s| inside(s)).any(|s| match s {
        egui::Shape::Mesh(m) => m.texture_id != egui::TextureId::default(),
        _ => false,
    });
    assert!(picture);
    screenshot::save(&mut h, "pad-label-preview");
}

#[test]
fn manual_label_text_boxes() {
    use vale_app::ui::pad::label_text::LABEL_TEXT;
    let text = "concat(upper(name), '\n', 'Pop ' || round(pop_max / 1000, 1) || ' k', ' ', \
                'Rank ' || scalerank, ' ', max(abs(pop_min - pop_max), 2 * scalerank))";
    for size in [BOARDS[0], BOARDS[3]] {
        let mut h = pad(size);
        let places = vector(&h, "Places");
        h.state_mut()
            .doc
            .entry_mut(places)
            .unwrap()
            .style
            .label
            .text = text.into();
        open_panel(&mut h, "Layers");
        tap(&mut h, "Open Places");
        scroll_to_control(&mut h, LABEL_TEXT);
        tap(&mut h, LABEL_TEXT);
        h.run_steps(2);
        screenshot::save(&mut h, &format!("pad-label-text-boxes-{}", size.0));
    }
}

#[test]
fn manual_value_boxes() {
    use vale_app::ui::pad::classify::EXPRESSION;
    let text = "max(abs(pop_min - pop_max), round(scalerank / 2, 1), floor(abs(pop_other))) ";
    for size in [BOARDS[0], BOARDS[3]] {
        let mut h = pad(size);
        open_classify(&mut h, "Places");
        scroll_to_control(&mut h, EXPRESSION);
        type_text(&mut h, EXPRESSION, text);
        screenshot::save(&mut h, &format!("pad-value-boxes-focus-{}", size.0));
        // A tap on the title of the field takes the focus away.
        let at = control(&h, EXPRESSION).unwrap().left_top() - vec2(0.0, 24.0);
        touch(&mut h, egui::TouchPhase::Start, at, None);
        h.step();
        touch(&mut h, egui::TouchPhase::End, at, None);
        h.run_steps(3);
        screenshot::save(&mut h, &format!("pad-value-boxes-{}", size.0));
    }
}

#[test]
fn blocks_and_code_edit_one_text() {
    use vale_app::ui::pad::label_text::{BLOCKS, CODE, LABEL_TEXT};
    let mut h = pad(BOARDS[0]);
    let places = vector(&h, "Places");
    // Text in a form that Blocks would write in another way.
    let odd = "concat(name,'  x')";
    h.state_mut()
        .doc
        .entry_mut(places)
        .unwrap()
        .style
        .label
        .text = odd.into();
    open_label_text(&mut h);
    // Blocks do not write text that the user does not edit.
    h.key_press(egui::Key::ArrowLeft);
    h.run_steps(2);
    assert_eq!(label_text(&h, places), odd);
    tap(&mut h, CODE);
    let code = format!("{LABEL_TEXT} code");
    assert!(control(&h, &code).is_some());
    type_text(&mut h, &code, "upper(name)");
    assert_eq!(label_text(&h, places), "upper(name)");
    tap(&mut h, BLOCKS);
    assert!(control(&h, &code).is_none());
    tap(&mut h, LABEL_TEXT);
    h.event(egui::Event::Text("!".into()));
    h.run_steps(2);
    assert_eq!(label_text(&h, places), "concat(upper(name), '!')");
}

/// Opens the Label text page of Places with label text `text`, and taps
/// the first chip of the field, which opens its popover.
fn open_chip(h: &mut Pad, text: &str) -> vale_store::LayerId {
    use vale_app::ui::pad::label_text::LABEL_TEXT;
    let places = vector(h, "Places");
    h.state_mut()
        .doc
        .entry_mut(places)
        .unwrap()
        .style
        .label
        .text = text.into();
    open_label_text(h);
    let field = control(h, LABEL_TEXT).unwrap();
    let pos = field.min + vec2(12.0 + 24.0, 12.0 + 16.0);
    touch(h, egui::TouchPhase::Start, pos, None);
    h.step();
    touch(h, egui::TouchPhase::End, pos, None);
    h.run_steps(3);
    places
}

/// Opens the Label text page of Places with label text `text`, and gives
/// the field the focus.
fn open_label(h: &mut Pad, text: &str) -> vale_store::LayerId {
    let places = vector(h, "Places");
    h.state_mut()
        .doc
        .entry_mut(places)
        .unwrap()
        .style
        .label
        .text = text.into();
    open_label_text(h);
    places
}

/// The layout of the label text field with text `text`, on the screen.
fn label_layout(h: &Pad, text: &str) -> vale_app::ui::token_field::Layout {
    use vale_app::ui::expr_help::ExprHelp;
    use vale_app::ui::pad::label_text::LABEL_TEXT;
    use vale_app::ui::token_field::{Metrics, layout, template};
    use vale_style::expr::editor::Template;
    let rect = control(h, LABEL_TEXT).unwrap();
    let m = Metrics::pad();
    let field = template::field_of(&Template::from_text(text), &ExprHelp::empty());
    let mut measure = |s: &str, font: &egui::FontId| {
        h.ctx
            .fonts_mut(|f| s.chars().map(|c| f.glyph_width(font, c)).sum::<f32>())
    };
    let mut lay = layout(&field.nodes, &m, rect.width() - 2.0 * m.pad.x, &mut measure);
    lay.translate((rect.min + m.pad).to_vec2());
    lay
}

/// The middle of the caret at `index` of the list at `path`.
fn stop(lay: &vale_app::ui::token_field::Layout, path: &[usize], index: usize) -> Pos2 {
    use vale_app::ui::token_field::Caret;
    let caret = Caret::new(path.to_vec(), index);
    lay.caret_rect(&caret).unwrap().center()
}

/// Holds a finger still at `from` for a long press, drags it to `to`, and
/// lifts it. `shot` names a picture of the drag before the lift.
fn long_press_drag(h: &mut Pad, from: Pos2, to: Pos2, shot: Option<&str>) {
    touch(h, egui::TouchPhase::Start, from, None);
    h.step();
    h.step();
    for i in 1..=6 {
        let at = from.lerp(to, i as f32 / 6.0);
        touch(h, egui::TouchPhase::Move, at, None);
        h.step();
    }
    if let Some(name) = shot {
        screenshot::save(h, name);
    }
    touch(h, egui::TouchPhase::End, to, None);
    h.run_steps(3);
}

#[test]
fn long_press_drags_chip_to_new_place() {
    use vale_app::ui::pad::chip_popover::DECIMAL_PLACES;
    let mut h = pad(BOARDS[0]);
    let text = "concat(name, ' ', round(pop_max, 1))";
    let places = open_label(&mut h, text);
    let lay = label_layout(&h, text);
    let pop = lay.rect_of(&[2]).unwrap().center();
    long_press_drag(&mut h, pop, stop(&lay, &[], 0), Some("pad-label-text-drag"));
    assert_eq!(
        label_text(&h, places),
        "concat(round(pop_max, 1), name, ' ')"
    );
    // The long press does not open the popover, and the keyboard stays.
    assert!(control(&h, &format!("Increase {DECIMAL_PLACES}")).is_none());
    assert!(h.output().platform_output.ime.is_some());
}

#[test]
fn drag_into_group_moves_chip_inside() {
    let mut h = pad(BOARDS[0]);
    let text = "concat(name, 'Pop ' || pop_max)";
    let places = open_label(&mut h, text);
    let lay = label_layout(&h, text);
    let name = lay.rect_of(&[0]).unwrap().center();
    long_press_drag(&mut h, name, stop(&lay, &[1], 0), None);
    assert_eq!(label_text(&h, places), "name || 'Pop ' || pop_max");
}

#[test]
fn drag_moves_a_group_by_its_frame() {
    let mut h = pad(BOARDS[0]);
    let text = "concat(name, 'Pop ' || pop_max)";
    let places = open_label(&mut h, text);
    let lay = label_layout(&h, text);
    let frame = lay.rect_of(&[1]).unwrap();
    let edge = pos2(frame.right() - 2.0, frame.center().y);
    long_press_drag(&mut h, edge, stop(&lay, &[], 0), None);
    assert_eq!(label_text(&h, places), "concat('Pop ' || pop_max, name)");
}

#[test]
fn drag_cancels_outside_field() {
    use vale_app::ui::pad::label_text::LABEL_TEXT;
    let mut h = pad(BOARDS[0]);
    let text = "concat(name, ' ', pop_max)";
    let places = open_label(&mut h, text);
    let lay = label_layout(&h, text);
    let pop = lay.rect_of(&[2]).unwrap().center();
    let below = control(&h, LABEL_TEXT).unwrap().center_bottom() + vec2(0.0, 80.0);
    long_press_drag(&mut h, pop, below, None);
    assert_eq!(label_text(&h, places), text);
}

#[test]
fn tap_still_opens_popover() {
    use vale_app::ui::pad::chip_popover::DECIMAL_PLACES;
    let mut h = pad(BOARDS[0]);
    let text = "concat(name, ' ', pop_max)";
    let places = open_label(&mut h, text);
    let lay = label_layout(&h, text);
    let pop = lay.rect_of(&[2]).unwrap().center();
    touch(&mut h, egui::TouchPhase::Start, pop, None);
    h.step();
    touch(&mut h, egui::TouchPhase::End, pop, None);
    h.run_steps(3);
    assert!(control(&h, &format!("Increase {DECIMAL_PLACES}")).is_some());
    assert_eq!(label_text(&h, places), text);
}

/// Scrolls the open popover until its control `name` shows, and taps it.
fn tap_in_popover(h: &mut Pad, name: &str) {
    use vale_app::ui::pad::popover;
    for _ in 0..10 {
        let card = popover::rect(&h.ctx).expect("a popover");
        if control(h, name).is_some_and(|r| card.contains_rect(r)) {
            break;
        }
        let from = card.center_bottom() - vec2(0.0, 30.0);
        drag(h, from, from - vec2(0.0, 100.0), None);
    }
    tap(h, name);
}

#[test]
fn copy_field_and_paste_keep_chip_format() {
    use vale_app::ui::pad::chip_popover::{COPY_FIELD, CUT_FIELD};
    use vale_app::ui::pad::widgets::PASTE;
    let _clipboard = one_clipboard_test();
    let mut h = pad(BOARDS[0]);
    let text = "concat(round(pop_max, 1), ' ', name)";
    let places = open_label(&mut h, text);
    let lay = label_layout(&h, text);
    let pop = lay.rect_of(&[0]).unwrap().center();
    touch(&mut h, egui::TouchPhase::Start, pop, None);
    h.step();
    touch(&mut h, egui::TouchPhase::End, pop, None);
    h.run_steps(3);
    tap_in_popover(&mut h, COPY_FIELD);
    assert_eq!(
        vale_app::clipboard::get().as_deref(),
        Some("round(pop_max, 1)")
    );
    // A long press in empty space at the end opens the edit menu.
    let end = stop(&lay, &[], 3) + vec2(60.0, 0.0);
    touch(&mut h, egui::TouchPhase::Start, end, None);
    h.run_steps(3);
    touch(&mut h, egui::TouchPhase::End, end, None);
    h.run_steps(3);
    tap(&mut h, PASTE);
    assert_eq!(
        label_text(&h, places),
        "concat(round(pop_max, 1), ' ', name, round(pop_max, 1))"
    );
    // Cut field takes the chip away, and the clipboard keeps its text.
    touch(&mut h, egui::TouchPhase::Start, pop, None);
    h.step();
    touch(&mut h, egui::TouchPhase::End, pop, None);
    h.run_steps(3);
    tap(&mut h, CUT_FIELD);
    assert_eq!(
        label_text(&h, places),
        "concat(' ', name, round(pop_max, 1))"
    );
    assert_eq!(
        vale_app::clipboard::get().as_deref(),
        Some("round(pop_max, 1)")
    );
}

#[test]
fn tap_on_expression_chip_opens_edit_menu() {
    use vale_app::ui::pad::widgets::CUT;
    let _clipboard = one_clipboard_test();
    let mut h = pad(BOARDS[0]);
    let text = "concat(name, pop_max * 2, pop_max)";
    let places = open_label(&mut h, text);
    let lay = label_layout(&h, text);
    let brk = lay.rect_of(&[1]).unwrap().center();
    touch(&mut h, egui::TouchPhase::Start, brk, None);
    h.step();
    touch(&mut h, egui::TouchPhase::End, brk, None);
    h.run_steps(3);
    tap(&mut h, CUT);
    assert_eq!(label_text(&h, places), "concat(name, pop_max)");
}

#[test]
fn decimal_places_write_round() {
    use vale_app::ui::pad::chip_popover::DECIMAL_PLACES;
    let mut h = pad(BOARDS[0]);
    let places = open_chip(&mut h, "pop_max");
    let plus = format!("Increase {DECIMAL_PLACES}");
    tap(&mut h, &plus);
    assert_eq!(label_text(&h, places), "round(pop_max)");
    tap(&mut h, &plus);
    assert_eq!(label_text(&h, places), "round(pop_max, 1)");
    screenshot::save(&mut h, "pad-label-text-popover");
    tap(&mut h, &format!("Decrease {DECIMAL_PLACES}"));
    tap(&mut h, &format!("Decrease {DECIMAL_PLACES}"));
    assert_eq!(label_text(&h, places), "pop_max");
}

#[test]
fn label_field_and_popover_stay_above_bar() {
    use vale_app::ui::pad::chip_popover::DECIMAL_PLACES;
    use vale_app::ui::pad::label_text::{LABEL_TEXT, bar_top};
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        open_chip(&mut h, "pop_max");
        let floor = bar_top(&h.ctx);
        let field = control(&h, LABEL_TEXT).unwrap();
        assert!(field.bottom() <= floor, "{what}: {field:?} {floor}");
        let plus = control(&h, &format!("Increase {DECIMAL_PLACES}"));
        assert!(plus.unwrap().bottom() <= floor, "{what}");
    }
}

#[test]
fn thousands_separator_writes_format_number() {
    use vale_app::ui::pad::chip_popover::THOUSANDS;
    let mut h = pad(BOARDS[0]);
    let places = open_chip(&mut h, "pop_max");
    tap(&mut h, THOUSANDS);
    assert_eq!(label_text(&h, places), "format_number(pop_max, 0)");
}

#[test]
fn case_writes_upper() {
    use vale_app::ui::pad::chip_popover::CASES;
    let mut h = pad(BOARDS[0]);
    let places = open_chip(&mut h, "concat(name, ' x')");
    tap(&mut h, CASES[1]);
    assert_eq!(label_text(&h, places), "concat(upper(name), ' x')");
}

#[test]
fn when_empty_uses_another_field() {
    use vale_app::ui::pad::chip_popover::{WHEN_EMPTY, use_field};
    let mut h = pad(BOARDS[0]);
    let places = open_chip(&mut h, "name");
    tap(&mut h, WHEN_EMPTY);
    tap_in_popover(&mut h, &use_field("adm0name"));
    assert_eq!(label_text(&h, places), "coalesce(name, adm0name)");
}

#[test]
fn remove_field_deletes_chip() {
    use vale_app::ui::pad::chip_popover::REMOVE_FIELD;
    let mut h = pad(BOARDS[0]);
    let places = open_chip(&mut h, "concat(name, ' x')");
    tap_in_popover(&mut h, REMOVE_FIELD);
    assert_eq!(label_text(&h, places), "' x'");
    assert!(control(&h, REMOVE_FIELD).is_none());
}

#[test]
fn the_font_row_asks_for_the_font_picker() {
    use vale_app::ui::pad::fonts::FONT;
    let font = |h: &Pad, id| h.state().doc.entry(id).unwrap().style.label.font.clone();
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        let places = vector(&h, "Places");
        open_panel(&mut h, "Layers");
        tap(&mut h, "Open Places");
        scroll_to_control(&mut h, FONT);
        check_controls(&h, &what);
        tap(&mut h, FONT);
        assert!(
            h.state()
                .actions
                .contains(&Action::PickFont(FontTarget::Layer(places))),
            "{what}"
        );
        assert_eq!(h.state().pad.page, None, "{what}");

        // The picker gives a family.
        let pick = |h: &mut Pad, family: &str| {
            h.state_mut()
                .run_action(Action::SetFont(FontTarget::Layer(places), family.into()));
            h.run_steps(3);
        };
        pick(&mut h, "Noto Sans");
        assert_eq!(font(&h, places), None, "{what}");
        // A family that the device does not have gives a warning.
        pick(&mut h, "Lora");
        assert_eq!(font(&h, places).as_deref(), Some("Lora"), "{what}");
        scroll_to_control(&mut h, FONT);
        let warning = "Font Lora is missing. Labels use Noto Sans.";
        assert!(h.query_by_label(warning).is_some(), "{what}");
        screenshot::save(&mut h, &format!("pad-font-row-{}", size.0));
    }
}

/// The screens of the Measure tool, and their safe areas: the bands at the
/// top, the right, the bottom, and the left.
fn measure_screens() -> Vec<((f32, f32), [f32; 4])> {
    let mut screens: Vec<_> = BOARDS.iter().map(|&size| (size, [0.0; 4])).collect();
    screens.push(((1133.0, 744.0), [0.0; 4]));
    screens.extend(IPHONES.map(|((w, h), safe)| ((w as f32, h as f32), safe)));
    screens
}

/// The layout of a world with a height layer, on a screen with a safe area.
fn safe_pad(size: (f32, f32), [top, right, bottom, left]: [f32; 4]) -> Pad {
    let mut h = pad(size);
    let insets = egui::epaint::MarginF32 {
        left,
        right,
        top,
        bottom,
    };
    h.input_mut().safe_area_insets = Some(egui::SafeAreaInsets(insets));
    h.run_steps(3);
    h
}

fn toggled(h: &Pad, name: &str) -> Option<bool> {
    let node = h.query_by_label(name)?;
    Some(node.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True))
}

fn disabled(h: &Pad, name: &str) -> bool {
    h.get_by_label(name).accesskit_node().is_disabled()
}

/// Makes sure that each control is inside the safe area.
fn check_safe(h: &Pad, size: (f32, f32), [top, right, bottom, left]: [f32; 4], what: &str) {
    let top = top.max(BANDS.0);
    let bottom = size.1 - bottom.max(BANDS.1);
    for c in &h.state().pad.controls {
        let r = c.rect;
        let inside = r.top() >= top
            && r.bottom() <= bottom
            && r.left() >= left
            && r.right() <= size.0 - right;
        assert!(inside, "{what}: {} is {r:?}", c.name);
    }
}

/// Scrolls the tool strip down, and then up, until a cell shows in full.
/// Scrolls the tool column of a short screen to its end, where the
/// controls of the tool are.
fn scroll_column_to_end(h: &mut Pad) {
    for _ in 0..4 {
        let rects = h.state().pad.rects.unwrap();
        if rects.row || rects.column_content <= rects.tools.height() + 0.5 {
            return;
        }
        // The column holds its cells to the right of the band of the
        // system.
        let column = rects.tools;
        let from = pos2(column.right() - 28.0, column.bottom() - 10.0);
        drag(h, from, pos2(from.x, column.top() + 10.0), None);
    }
}

fn scroll_strip_to(h: &mut Pad, name: &str) {
    for i in 0..10 {
        let rects = h.state().pad.rects.unwrap();
        let strip = rects.tools;
        if control(h, name).is_some_and(|r| strip.contains_rect(r)) {
            return;
        }
        let d = if i < 5 { -120.0 } else { 120.0 };
        // A row scrolls from side to side.
        let along = if rects.row {
            vec2(d, 0.0)
        } else {
            vec2(0.0, d)
        };
        // The strip at the left edge holds its cells to the right of the
        // band of the system.
        let middle = pos2(strip.right() - strip.height().min(30.0), strip.center().y);
        let middle = if rects.row { strip.center() } else { middle };
        let from = middle - along / 2.0;
        drag(h, from, from + along, None);
    }
}

#[test]
fn the_measure_cell_of_the_tool_strip_opens_the_tool_with_any_layer_on_each_screen() {
    use vale_store::GeometryKind;
    for (size, safe) in measure_screens() {
        let mut h = safe_pad(size, safe);
        let height = h.state().doc.project.rasters()[0].id;
        let mut layers = vec![None, Some(LayerRef::Raster(height))];
        for kind in [
            GeometryKind::Line,
            GeometryKind::Point,
            GeometryKind::Polygon,
        ] {
            let id = h.state_mut().doc.add_layer(kind);
            layers.push(Some(LayerRef::Vector(id)));
        }
        for layer in layers {
            let what = format!("{size:?} with {layer:?}");
            h.state_mut().select(layer);
            h.run_steps(2);
            assert!(!h.state().globe.measuring(), "{what}");
            scroll_strip_to(&mut h, MEASURE);
            let strip = h.state().pad.rects.unwrap().tools;
            let cell = control(&h, MEASURE).unwrap_or_else(|| panic!("{what}"));
            assert!(strip.contains_rect(cell), "{what}");
            assert!(cell.width() >= 44.0 && cell.height() >= 44.0, "{what}");
            // The cell comes after Move.
            if let Some(pan) = control(&h, "Pan") {
                let row = h.state().pad.rects.unwrap().row;
                let after = if row {
                    pan.right() <= cell.left()
                } else {
                    pan.bottom() <= cell.top()
                };
                assert!(after, "{what}");
            }
            assert!(!disabled(&h, MEASURE), "{what}");
            quick_tap(&mut h, MEASURE);
            assert!(h.state().globe.measuring(), "{what}");
            assert_eq!(toggled(&h, MEASURE), Some(true), "{what}");
            // No other cell of the tool strip is active.
            for name in ["Pan", SCULPT, TERRAIN, VECTOR] {
                assert_ne!(toggled(&h, name), Some(true), "{what}: {name}");
            }
            // A disabled cell is not in the list of the controls, so the
            // path gets three points.
            let center = pos2(size.0 / 2.0, size.1 / 2.0);
            for offset in [vec2(-50.0, -40.0), vec2(50.0, -40.0), vec2(0.0, 50.0)] {
                finger_tap(&mut h, center + offset);
            }
            assert_eq!(h.state().measure.points.len(), 3, "{what}");
            scroll_column_to_end(&mut h);
            let card = context_card(&h);
            for name in [SNAP, CLEAR_PATH] {
                let rect = control(&h, name).unwrap_or_else(|| panic!("{what}: {name}"));
                assert!(card.contains_rect(rect), "{what}");
            }
            for name in ["Close path", "Remove point"] {
                assert!(control(&h, name).is_none(), "{what}: {name}");
            }
            check_controls(&h, &format!("{what} with Measure"));
            check_safe(&h, size, safe, &what);
            let screen = Rect::from_min_size(Pos2::ZERO, size.into());
            for card in cards(&h) {
                assert!(screen.contains_rect(card), "{what}: {card:?}");
            }
            // Move leaves the tool and clears the path.
            scroll_strip_to(&mut h, "Pan");
            quick_tap(&mut h, "Pan");
            assert!(!h.state().globe.measuring(), "{what}");
            assert_eq!(h.state().globe.tool, Tool::Navigate, "{what}");
            assert!(h.state().measure.is_empty(), "{what}");
        }
    }
}

#[test]
fn the_cells_of_the_measure_card_act_on_the_path() {
    let screens = [
        (BOARDS[0], [0.0; 4]),
        ((393.0, 852.0), [59.0, 0.0, 34.0, 0.0]),
        ((852.0, 393.0), [0.0, 59.0, 21.0, 59.0]),
    ];
    for (size, safe) in screens {
        let what = format!("{size:?}");
        let mut h = safe_pad(size, safe);
        h.state_mut().select(None);
        h.run_steps(2);
        scroll_strip_to(&mut h, MEASURE);
        quick_tap(&mut h, MEASURE);
        // With no points, the readout is the readout of the place.
        let value = h.state().readout_value();
        let place = readout_text(h.state().cursor_lonlat, value.as_deref());
        assert_eq!(readout_line(h.state()), place, "{what}");
        assert!(disabled(&h, CLEAR_PATH), "{what}");
        let center = pos2(size.0 / 2.0, size.1 / 2.0);
        for offset in [vec2(-50.0, -40.0), vec2(50.0, -40.0), vec2(0.0, 50.0)] {
            finger_tap(&mut h, center + offset);
        }
        assert_eq!(h.state().measure.points.len(), 3, "{what}");
        let text = readout_line(h.state());
        assert!(text.starts_with("Length "), "{what}: {text}");
        assert_eq!(Some(text), h.state().measure_readout(), "{what}");

        // A tap on the first point closes the path.
        finger_tap(&mut h, center + vec2(-50.0, -40.0));
        assert!(h.state().measure.closed, "{what}");
        let text = readout_line(h.state());
        assert!(text.starts_with("Perimeter "), "{what}: {text}");
        // Undo reopens the path and removes a point, and redo puts them
        // back.
        tap(&mut h, "Undo");
        assert!(!h.state().measure.closed, "{what}");
        tap(&mut h, "Undo");
        assert_eq!(h.state().measure.points.len(), 2, "{what}");
        tap(&mut h, "Redo");
        tap(&mut h, "Redo");
        assert!(h.state().measure.closed, "{what}");
        assert!(disabled(&h, "Redo"), "{what}");
        scroll_strip_to(&mut h, CLEAR_PATH);
        tap(&mut h, CLEAR_PATH);
        assert!(h.state().measure.is_empty(), "{what}");
        assert!(disabled(&h, CLEAR_PATH), "{what}");
        tap(&mut h, "Undo");
        assert!(h.state().measure.closed, "{what}");

        let snap = h.state().globe.snap.on;
        scroll_strip_to(&mut h, SNAP);
        tap(&mut h, SNAP);
        assert_eq!(h.state().globe.snap.on, !snap, "{what}");
        tap(&mut h, SNAP);

        // A cell of the tool strip leaves the tool and clears the path.
        let height = h.state().doc.project.rasters()[0].id;
        h.state_mut().select(Some(LayerRef::Raster(height)));
        finger_tap(&mut h, center);
        assert!(!h.state().measure.is_empty(), "{what}");
        scroll_strip_to(&mut h, SCULPT);
        quick_tap(&mut h, SCULPT);
        assert!(!h.state().globe.measuring(), "{what}");
        assert!(h.state().measure.is_empty(), "{what}");
    }
}

#[test]
fn a_crossing_path_shows_the_perimeter_and_no_area_or_hint() {
    let mut h = safe_pad(BOARDS[0], [0.0; 4]);
    h.state_mut().select(None);
    h.state_mut().globe.snap.on = false;
    quick_tap(&mut h, MEASURE);
    let center = pos2(597.0, 417.0);
    let bow = [
        vec2(-60.0, -60.0),
        vec2(60.0, 60.0),
        vec2(60.0, -60.0),
        vec2(-60.0, 60.0),
    ];
    for offset in bow.into_iter().chain([bow[0]]) {
        finger_tap(&mut h, center + offset);
    }
    assert!(h.state().measure.closed);
    let text = readout_line(h.state());
    assert!(text.starts_with("Perimeter "), "{text}");
    assert!(!text.contains("Area") && !text.contains('·'), "{text}");
    assert!(h.query_by_label_contains("crosses").is_none());
}

#[test]
fn the_brush_button_of_the_measure_card_opens_the_snapping_settings() {
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        let points = h.state_mut().doc.add_layer(vale_store::GeometryKind::Point);
        h.state_mut().select(Some(LayerRef::Vector(points)));
        h.run_steps(2);
        scroll_strip_to(&mut h, MEASURE);
        quick_tap(&mut h, MEASURE);
        tap(&mut h, "Brush settings");
        assert!(h.state().pad.is_open(Panel::Brush), "{what}");
        for name in [SNAPPING, SNAP_VERTICES, SNAP_EDGES, SNAP_DISTANCE] {
            assert!(control(&h, name).is_some(), "{what}: {name}");
        }
        assert!(control(&h, "Brush size").is_none(), "{what}");
        check_controls(&h, &format!("{what} with the snapping settings"));
    }
}

/// Two cells of a strip are in order: in a column, or in a row.
fn in_order(a: Rect, b: Rect) -> bool {
    a.bottom() <= b.top() || a.right() <= b.left()
}

#[test]
fn the_measure_cell_and_card_fit_in_the_atlas_on_each_screen() {
    use vale_app::ui::atlas::AtlasTool;
    for (size, safe) in measure_screens() {
        let what = format!("Atlas {size:?}");
        let mut h = safe_pad(size, safe);
        h.state_mut().workspace = Workspace::Atlas;
        vale_app::ui::atlas::add_frame(h.state_mut());
        h.run_steps(3);
        lower_sheet(&mut h);
        let strip = h.state().pad.rects.unwrap().tools;
        let names = ["Frame tool", "Select", "Vertex", "Move", MEASURE];
        for name in names {
            scroll_strip_to(&mut h, name);
            let cell = control(&h, name).unwrap_or_else(|| panic!("{what}: {name}"));
            assert!(strip.contains_rect(cell), "{what}: {name}");
        }
        // On a short screen, the strip scrolls, and only some cells show.
        let cells: Vec<_> = names.iter().filter_map(|name| control(&h, name)).collect();
        assert!(cells.len() > 1, "{what}");
        for pair in cells.windows(2) {
            assert!(in_order(pair[0], pair[1]), "{what}");
        }
        quick_tap(&mut h, MEASURE);
        assert!(h.state().measuring(), "{what}");
        assert_eq!(h.state().atlas.tool, AtlasTool::Measure, "{what}");
        assert_eq!(toggled(&h, MEASURE), Some(true), "{what}");
        for name in ["Frame tool", "Select", "Vertex", "Move"] {
            assert_ne!(toggled(&h, name), Some(true), "{what}: {name}");
        }
        assert!(!readout_line(h.state()).starts_with("Length"), "{what}");
        // On a short screen, the card scrolls, and the Brush button is
        // below its end. The tool column scrolls to its end.
        scroll_column_to_end(&mut h);
        let card = context_card(&h);
        let rects = h.state().pad.rects.unwrap();
        let all = rects.card_cells == 3 || !rects.row;
        let names: &[&str] = if all {
            &[SNAP, "Brush settings"]
        } else {
            &[SNAP]
        };
        for name in names {
            let rect = control(&h, name).unwrap_or_else(|| panic!("{what}: {name}"));
            assert!(card.contains_rect(rect), "{what}: {name}");
        }
        assert!(disabled(&h, CLEAR_PATH), "{what}");
        check_safe(&h, size, safe, &what);
        let screen = Rect::from_min_size(Pos2::ZERO, size.into());
        let r = h.state().pad.rects.unwrap();
        let mut cards = vec![r.workspace, r.actions, card];
        // The tool column holds the cells of the tool.
        if r.row {
            cards.push(r.tools);
        } else {
            assert!(r.tools.contains_rect(card), "{what}");
        }
        for (i, a) in cards.iter().enumerate() {
            assert!(screen.contains_rect(*a), "{what}: {a:?}");
            for b in &cards[i + 1..] {
                assert!(!a.intersects(*b), "{what}: {a:?} {b:?}");
            }
        }
        for c in &h.state().pad.controls {
            let r = c.rect;
            assert!(
                r.width() >= 44.0 && r.height() >= 44.0,
                "{what}: {}",
                c.name
            );
        }
        if all {
            tap(&mut h, "Brush settings");
            assert!(h.state().pad.is_open(Panel::Brush), "{what}");
            assert!(control(&h, SNAPPING).is_some(), "{what}");
            check_safe(
                &h,
                size,
                safe,
                &format!("{what} with the snapping settings"),
            );
        }
        // Frame leaves the tool.
        lower_sheet(&mut h);
        scroll_strip_to(&mut h, "Frame tool");
        quick_tap(&mut h, "Frame tool");
        assert_eq!(h.state().atlas.tool, AtlasTool::Frame, "{what}");
    }
}

#[test]
fn the_vertex_cell_and_card_fit_in_the_atlas_on_each_screen() {
    use vale_app::ui::atlas::AtlasTool;
    use vale_app::ui::pad::atlas::{SNAP_TO_LAYOUT, VERTEX};
    use vale_app::ui::pad::{DELETE_VERTEX, DESELECT};
    for (size, safe) in measure_screens() {
        let what = format!("Atlas {size:?}");
        let mut h = safe_pad(size, safe);
        h.state_mut().workspace = Workspace::Atlas;
        vale_app::ui::atlas::add_frame(h.state_mut());
        h.run_steps(3);
        lower_sheet(&mut h);
        scroll_strip_to(&mut h, VERTEX);
        quick_tap(&mut h, VERTEX);
        assert_eq!(h.state().atlas.tool, AtlasTool::Vertex, "{what}");
        assert_eq!(toggled(&h, VERTEX), Some(true), "{what}");
        h.state_mut().atlas.vertices = vec![(0, 0)];
        h.run_steps(2);
        lower_sheet(&mut h);
        scroll_column_to_end(&mut h);
        let names = [SNAP_TO_LAYOUT, DELETE_VERTEX, DESELECT];
        let cells = names.map(|name| control(&h, name).unwrap_or_else(|| panic!("{what}: {name}")));
        for pair in cells.windows(2) {
            assert!(in_order(pair[0], pair[1]), "{what}");
        }
        let card = cells[0].union(cells[2]);
        let r = h.state().pad.rects.unwrap();
        let mut cards = vec![r.workspace, r.actions];
        if r.row {
            cards.push(r.tools);
        } else {
            assert!(r.tools.contains_rect(card), "{what}");
        }
        cards.extend(h.state().atlas.panel_rect);
        for other in cards {
            assert!(!card.intersects(other), "{what}: {card:?} {other:?}");
        }
        for c in &h.state().pad.controls {
            let r = c.rect;
            assert!(
                r.width() >= 44.0 && r.height() >= 44.0,
                "{what}: {}",
                c.name
            );
        }
        check_safe(&h, size, safe, &what);
        tap(&mut h, DELETE_VERTEX);
        assert!(h.state().atlas.vertices.is_empty(), "{what}");
        assert!(
            matches!(
                h.state().atlas.frame().and_then(|id| h.state().doc.project.atlas.frame(id)),
                Some(f) if f.shape.rings()[0].len() == 3
            ),
            "{what}"
        );
    }
}

#[test]
fn the_frame_strip_fits_in_the_atlas_on_each_screen() {
    use vale_app::ui::atlas::{AtlasSelection, AtlasTool};
    use vale_app::ui::pad::DESELECT;
    use vale_app::ui::pad::atlas::SNAP_TO_LAYOUT;
    use vale_app::ui::select::DELETE;
    for (size, safe) in measure_screens() {
        let what = format!("Atlas {size:?}");
        let mut h = safe_pad(size, safe);
        h.state_mut().workspace = Workspace::Atlas;
        vale_app::ui::atlas::add_frame(h.state_mut());
        h.run_steps(3);
        lower_sheet(&mut h);
        assert_eq!(h.state().atlas.tool, AtlasTool::Frame, "{what}");
        scroll_column_to_end(&mut h);
        let names = [SNAP_TO_LAYOUT, DELETE, DESELECT];
        let cells = names.map(|name| control(&h, name).unwrap_or_else(|| panic!("{what}: {name}")));
        for pair in cells.windows(2) {
            assert!(in_order(pair[0], pair[1]), "{what}");
        }
        let card = cells[0].union(cells[2]);
        let r = h.state().pad.rects.unwrap();
        let mut cards = vec![r.workspace, r.actions];
        if r.row {
            cards.push(r.tools);
        } else {
            assert!(r.tools.contains_rect(card), "{what}");
        }
        cards.extend(h.state().atlas.panel_rect);
        for other in cards {
            assert!(!card.intersects(other), "{what}: {card:?} {other:?}");
        }
        for c in &h.state().pad.controls {
            let r = c.rect;
            assert!(
                r.width() >= 44.0 && r.height() >= 44.0,
                "{what}: {}",
                c.name
            );
        }
        check_safe(&h, size, safe, &what);
        tap(&mut h, DELETE);
        assert_eq!(h.state().atlas.selection, AtlasSelection::None, "{what}");
        assert!(control(&h, DELETE).is_none(), "{what}");
        assert!(control(&h, DESELECT).is_none(), "{what}");
    }
}

/// The features of `select_pad`.
struct Picks {
    point: FeatureRef,
    /// A second point in the point layer, above and left of `point`.
    second: FeatureRef,
    line: FeatureRef,
    polygon: FeatureRef,
}

/// A world with a project file, one new layer of each geometry kind, and
/// the Select tool in the point layer. The other vector layers are hidden.
/// The point is at the middle of the canvas, the line is above it, and the
/// polygon is below it.
fn select_pad(view: WorldView, dir: &std::path::Path) -> (Pad, Picks) {
    use vale_store::{Feature, Geometry};
    let mut h = saved_pad(BOARDS[0], dir);
    let state = h.state_mut();
    let shown: Vec<_> = state.doc.project.layers().iter().map(|l| l.id).collect();
    for id in shown {
        state.doc.project.set_layer_visible(id, false);
    }
    let mut add = |id: Option<vale_store::LayerId>, kind, geometry| {
        let id = id.unwrap_or_else(|| state.doc.add_layer(kind));
        let feature = Feature::new(geometry, Default::default());
        let uuid = feature.uuid();
        assert!(state.doc.project.add_feature(id, feature));
        FeatureRef { layer: id, uuid }
    };
    let point = add(
        None,
        GeometryKind::Point,
        Geometry::Points(vec![[0.0, 0.0]]),
    );
    let second = add(
        Some(point.layer),
        GeometryKind::Point,
        Geometry::Points(vec![[-2.0, 2.0]]),
    );
    let line = add(
        None,
        GeometryKind::Line,
        Geometry::Lines(vec![vec![[-8.0, 10.0], [8.0, 10.0]]]),
    );
    let square = vec![[-10.0, -25.0], [10.0, -25.0], [10.0, -12.0], [-10.0, -12.0]];
    let polygon = add(
        None,
        GeometryKind::Polygon,
        Geometry::Polygons(vec![vec![square]]),
    );
    state.select(Some(LayerRef::Vector(point.layer)));
    state.globe.world_view = view;
    state.save_waiting().unwrap();
    h.run_steps(2);
    quick_tap(&mut h, select::TOOL);
    assert_eq!(h.state().globe.tool, Tool::Select);
    let center = h.state().globe.rect.center();
    let at = screen_at(&h, 0.0, 0.0);
    h.state_mut().globe.move_place(at, center);
    h.run_steps(2);
    let picks = Picks {
        point,
        second,
        line,
        polygon,
    };
    (h, picks)
}

/// The screen position of a place.
fn screen_at(h: &Pad, lon: f64, lat: f64) -> Pos2 {
    let dir = vale_app::globe::math::lonlat_to_dir(lon, lat);
    let pos = h.state().globe.project(dir);
    pos.unwrap_or_else(|| panic!("{lon}, {lat} does not show"))
}

fn finger_tap_at(h: &mut Pad, pos: Pos2) {
    touch(h, egui::TouchPhase::Start, pos, None);
    h.step();
    touch(h, egui::TouchPhase::End, pos, None);
    h.run_steps(3);
}

fn select_layer(h: &mut Pad, id: vale_store::LayerId) {
    h.state_mut().select(Some(LayerRef::Vector(id)));
    h.run_steps(2);
}

fn selection(h: &Pad) -> Vec<FeatureRef> {
    h.state().feature_selection.clone()
}

fn geometry_of(h: &Pad, f: FeatureRef) -> vale_store::Geometry {
    let layer = h.state().doc.project.layer(f.layer).unwrap();
    let feature = layer.features.iter().find(|x| x.uuid() == f.uuid).unwrap();
    feature.geometry.clone()
}

fn inside_polygon(g: &vale_store::Geometry, lon: f64, lat: f64) -> bool {
    use vale_app::globe::{math::lonlat_to_dir, vector};
    let vale_store::Geometry::Polygons(polygons) = g else {
        panic!("not a polygon");
    };
    polygons.iter().any(|p| {
        let rings: Vec<_> = (p.iter().enumerate())
            .map(|(i, r)| vector::ring(r, i > 0))
            .collect();
        vector::contains(&rings, lonlat_to_dir(lon, lat))
    })
}

/// A pen stroke through places.
fn pen_way(h: &mut Pad, way: &[[f64; 2]]) {
    let path: Vec<Pos2> = way.iter().map(|p| screen_at(h, p[0], p[1])).collect();
    pen_path(h, &path);
}

#[test]
fn a_stroke_from_an_outline_reshapes_the_polygon_in_one_undo_step() {
    use vale_app::ui::pad::RESHAPE;
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    select_layer(&mut h, picks.polygon.layer);
    quick_tap(&mut h, VECTOR);
    assert_eq!(h.state().globe.tool, Tool::Polygon);
    assert!(!h.state().globe.line.reshape);
    tap(&mut h, RESHAPE);
    assert!(h.state().globe.line.reshape);
    assert_eq!(toggled(&h, RESHAPE), Some(true));
    check_controls(&h, "the polygon tool with Reshape");
    screenshot::save(&mut h, "pad-reshape-switch");
    h.state_mut().globe.snap.on = false;
    // The styles of the new layers go in the file first.
    step_and_save(&mut h, false);
    h.state_mut().save_waiting().unwrap();
    let before = geometry_of(&h, picks.polygon);
    let saved = saves(&h);

    // A bump out of the south side.
    pen_way(
        &mut h,
        &[[-4.0, -25.0], [-4.0, -29.0], [4.0, -29.0], [4.0, -25.0]],
    );
    for _ in 0..3 {
        step_and_save(&mut h, false);
    }
    assert_eq!(line_features(&h, picks.polygon.layer).len(), 1);
    let after = geometry_of(&h, picks.polygon);
    assert!(inside_polygon(&after, 0.0, -27.0));
    assert!(inside_polygon(&after, 0.0, -18.0));
    assert!(!inside_polygon(&before, 0.0, -27.0));
    assert_eq!(saves(&h), saved + 1);

    tap(&mut h, "Undo");
    assert_eq!(geometry_of(&h, picks.polygon), before);
    tap(&mut h, "Redo");
    assert_eq!(geometry_of(&h, picks.polygon), after);
}

#[test]
fn the_flyout_of_the_mode_cell_chooses_the_mode_of_the_polygon_tool_on_each_board() {
    use vale_app::globe::combine::PolygonMode;
    use vale_app::ui::pad::{POLYGON_MODE, POLYGON_MODES, RESHAPE};
    for size in BOARDS {
        let what = format!("{size:?}");
        let dir = tempfile::tempdir().unwrap();
        let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
        h.set_size(egui::vec2(size.0, size.1));
        h.run_steps(2);
        select_layer(&mut h, picks.polygon.layer);
        quick_tap(&mut h, VECTOR);
        assert_eq!(toggled(&h, POLYGON_MODE), Some(false), "{what}");
        let mode = |h: &Pad| h.state().globe.line.polygon_mode;
        let open = |h: &Pad| h.state().pad.flyouts.open == Some(Flyout::PolygonMode);
        let cell = |h: &Pad| control(h, POLYGON_MODE).unwrap().center();
        let row = |h: &Pad, name: &str| {
            control(h, name)
                .unwrap_or_else(|| panic!("{what}: no {name}"))
                .center()
        };

        // A finger opens the flyout and chooses a mode.
        let at = cell(&h);
        finger_tap_at(&mut h, at);
        assert!(open(&h), "{what}");
        let flyout = h.state().pad.flyouts.rect.unwrap();
        for (name, ..) in POLYGON_MODES {
            assert!(
                flyout.contains_rect(control(&h, name).unwrap()),
                "{what}: {name}"
            );
        }
        check_controls(&h, &format!("{what}: the flyout of the polygon modes"));
        if size == BOARDS[0] {
            screenshot::save(&mut h, "pad-polygon-modes");
        }
        let at = row(&h, "Add");
        finger_tap_at(&mut h, at);
        assert_eq!(mode(&h), PolygonMode::Add, "{what}");
        assert!(!open(&h), "{what}");
        assert_eq!(toggled(&h, POLYGON_MODE), Some(true), "{what}");

        // The pen.
        let at = cell(&h);
        pen_path(&mut h, &[at]);
        assert!(open(&h), "{what}");
        let at = row(&h, "Subtract");
        pen_path(&mut h, &[at]);
        assert_eq!(mode(&h), PolygonMode::Subtract, "{what}");
        // Reshape works in the New mode only.
        assert_eq!(toggled(&h, RESHAPE), Some(false), "{what}");

        // The mouse.
        click(&mut h, POLYGON_MODE);
        assert!(open(&h), "{what}");
        click(&mut h, "New");
        assert_eq!(mode(&h), PolygonMode::New, "{what}");

        // A second tap on the cell and a tap outside close the flyout.
        quick_tap(&mut h, POLYGON_MODE);
        assert!(open(&h), "{what}");
        quick_tap(&mut h, POLYGON_MODE);
        assert!(!open(&h), "{what}");
        quick_tap(&mut h, POLYGON_MODE);
        assert!(open(&h), "{what}");
        let away = h.state().globe.rect.center();
        finger_tap_at(&mut h, away);
        assert!(!open(&h), "{what}");
        assert_eq!(mode(&h), PolygonMode::New, "{what}");
    }
}

#[test]
fn the_context_strip_shows_only_the_controls_of_the_tool() {
    use vale_app::ui::pad::{POLYGON_MODE, RESHAPE};
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    let all = [
        SMOOTHING,
        SNAP,
        RESHAPE,
        TERRAIN_FIT,
        POLYGON_MODE,
        "Brush settings",
        "Size",
        "Flow",
    ];
    let check = |h: &Pad, shown: &[&str], what: &str| {
        for name in all {
            let want = shown.contains(&name);
            assert_eq!(control(h, name).is_some(), want, "{what}: {name}");
        }
    };
    select_layer(&mut h, picks.polygon.layer);
    assert_eq!(h.state().globe.tool, Tool::Select);
    check(&h, &[], "the Select tool");
    quick_tap(&mut h, VECTOR);
    let polygon = [SMOOTHING, SNAP, RESHAPE, POLYGON_MODE, "Brush settings"];
    check(&h, &polygon, "the polygon tool");
    // The list of the modes stays open.
    quick_tap(&mut h, POLYGON_MODE);
    h.run_steps(5);
    assert_eq!(h.state().pad.flyouts.open, Some(Flyout::PolygonMode));
    quick_tap(&mut h, POLYGON_MODE);
    select_layer(&mut h, picks.line.layer);
    assert!(h.state().globe.lining());
    let line = [SMOOTHING, SNAP, RESHAPE, TERRAIN_FIT, "Brush settings"];
    check(&h, &line, "the line tool");
    select_layer(&mut h, picks.point.layer);
    quick_tap(&mut h, VECTOR);
    assert!(h.state().globe.placing());
    check(&h, &[SNAP, "Brush settings"], "the point tool");
    quick_tap(&mut h, "Pan");
    check(&h, &[], "the Move tool");
}

#[test]
fn the_selection_shows_in_each_tool() {
    use vale_app::ui::pad::MEASURE;
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    select_layer(&mut h, picks.polygon.layer);
    h.state_mut().feature_selection = vec![picks.polygon];
    h.run_steps(2);
    // The wide line in the accent color on the outline.
    let highlight = |h: &Pad| {
        painted_shapes(h).iter().any(|s| {
            matches!(s, egui::Shape::Path(p)
                if p.stroke.width == 3.0
                    && p.stroke.color == egui::epaint::ColorMode::Solid(theme::ACCENT))
        })
    };
    assert!(highlight(&h), "the Select tool");
    for name in [VECTOR, "Pan", MEASURE] {
        quick_tap(&mut h, name);
        assert_eq!(h.state().feature_selection, vec![picks.polygon], "{name}");
        assert!(highlight(&h), "{name}");
    }
}

#[test]
fn the_context_strip_has_deselect_in_each_tool_while_features_are_selected() {
    use vale_app::ui::pad::{DESELECT, MEASURE};
    for size in BOARDS {
        let what = format!("{size:?}");
        let dir = tempfile::tempdir().unwrap();
        let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
        h.set_size(egui::vec2(size.0, size.1));
        select_layer(&mut h, picks.polygon.layer);
        // With no selection, there is no Deselect, and Move has no context
        // strip.
        assert!(control(&h, DESELECT).is_none(), "{what}");
        quick_tap(&mut h, "Pan");
        let r = h.state().pad.rects.unwrap();
        let tools_only = 12.0 + TOOLS as f32 * 44.0 + 0.5;
        assert!(
            r.brush.is_none() && (r.row || r.column_content <= tools_only),
            "{what}"
        );
        for name in [select::TOOL, "Pan", VECTOR, MEASURE] {
            quick_tap(&mut h, name);
            h.state_mut().feature_selection = vec![picks.polygon];
            h.run_steps(2);
            assert!(control(&h, DESELECT).is_some(), "{what}: {name}");
            check_controls(&h, &format!("{what}: Deselect in {name}"));
        }
        quick_tap(&mut h, VECTOR);
        tap(&mut h, DESELECT);
        assert!(h.state().feature_selection.is_empty(), "{what}");
        assert!(control(&h, DESELECT).is_none(), "{what}");
        assert_eq!(h.state().globe.tool, Tool::Polygon, "{what}");
    }
}

#[test]
fn a_hold_on_the_mode_cell_opens_the_flyout() {
    use vale_app::ui::pad::POLYGON_MODE;
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    select_layer(&mut h, picks.polygon.layer);
    quick_tap(&mut h, VECTOR);
    hold_cell(&mut h, POLYGON_MODE);
    // The end of the hold is not a tap, so the flyout stays open.
    assert_eq!(h.state().pad.flyouts.open, Some(Flyout::PolygonMode));
}

#[test]
fn add_grows_only_the_polygon_that_the_stroke_reaches_first() {
    use vale_app::globe::combine::PolygonMode;
    use vale_store::{Feature, Geometry};
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    let id = picks.polygon.layer;
    // A second polygon east of the square.
    let east = vec![[20.0, -25.0], [30.0, -25.0], [30.0, -12.0], [20.0, -12.0]];
    let feature = Feature::new(Geometry::Polygons(vec![vec![east]]), Default::default());
    let east = FeatureRef {
        layer: id,
        uuid: feature.uuid(),
    };
    assert!(h.state_mut().doc.project.add_feature(id, feature));
    select_layer(&mut h, id);
    quick_tap(&mut h, VECTOR);
    h.state_mut().globe.line.polygon_mode = PolygonMode::Add;
    h.state_mut().globe.snap.on = false;
    let before = geometry_of(&h, east);
    // From the gap, west into the square, and then east into the other
    // polygon.
    pen_way(
        &mut h,
        &[
            [15.0, -20.0],
            [5.0, -20.0],
            [5.0, -17.0],
            [25.0, -17.0],
            [25.0, -20.0],
        ],
    );
    h.run_steps(3);
    assert!(inside_polygon(&geometry_of(&h, picks.polygon), 15.0, -18.5));
    assert_eq!(geometry_of(&h, east), before);
    assert_eq!(line_features(&h, id).len(), 2);
}

#[test]
fn add_and_subtract_change_polygons_in_one_undo_step_each() {
    use vale_app::globe::combine::PolygonMode;
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    select_layer(&mut h, picks.polygon.layer);
    quick_tap(&mut h, VECTOR);
    h.state_mut().globe.line.polygon_mode = PolygonMode::Add;
    h.state_mut().globe.snap.on = false;
    step_and_save(&mut h, false);
    h.state_mut().save_waiting().unwrap();
    let before = geometry_of(&h, picks.polygon);
    let stroke = |h: &mut Pad, way: &[[f64; 2]]| {
        let saved = saves(h);
        pen_way(h, way);
        for _ in 0..3 {
            step_and_save(h, false);
        }
        assert_eq!(saves(h), saved + 1);
        assert_eq!(line_features(h, picks.polygon.layer).len(), 1);
        geometry_of(h, picks.polygon)
    };
    let undo_redo = |h: &mut Pad, before: &vale_store::Geometry, after: &vale_store::Geometry| {
        tap(h, "Undo");
        assert_eq!(&geometry_of(h, picks.polygon), before);
        tap(h, "Redo");
        assert_eq!(&geometry_of(h, picks.polygon), after);
    };

    // Add, from the east into the square and back out. The stroke starts
    // away from the polygon.
    let added = stroke(
        &mut h,
        &[[16.0, -18.0], [5.0, -18.0], [5.0, -14.0], [16.0, -14.0]],
    );
    assert!(inside_polygon(&added, 13.0, -16.0));
    assert!(inside_polygon(&added, -5.0, -20.0));
    undo_redo(&mut h, &before, &added);

    // Subtract, through the polygon.
    h.state_mut().globe.line.polygon_mode = PolygonMode::Subtract;
    let cut = stroke(
        &mut h,
        &[[-2.0, -28.0], [2.0, -28.0], [2.0, -9.0], [-2.0, -9.0]],
    );
    let vale_store::Geometry::Polygons(parts) = &cut else {
        panic!("not a polygon");
    };
    assert_eq!(parts.len(), 2);
    assert!(!inside_polygon(&cut, 0.0, -18.0));
    assert!(inside_polygon(&cut, -6.0, -18.0));
    undo_redo(&mut h, &added, &cut);

    // Subtract inside the polygon cuts a hole.
    let holed = stroke(
        &mut h,
        &[[-8.0, -22.0], [-4.0, -22.0], [-4.0, -16.0], [-8.0, -16.0]],
    );
    let vale_store::Geometry::Polygons(parts) = &holed else {
        panic!("not a polygon");
    };
    assert_eq!(parts.iter().map(|p| p.len()).sum::<usize>(), 3);
    assert!(!inside_polygon(&holed, -6.0, -19.0));
    assert!(inside_polygon(&holed, -9.0, -19.0));
    undo_redo(&mut h, &cut, &holed);
}

#[test]
fn with_reshape_a_stroke_from_a_line_reshapes_it_and_other_strokes_draw_lines() {
    use vale_app::ui::pad::RESHAPE;
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    select_layer(&mut h, picks.line.layer);
    quick_tap(&mut h, VECTOR);
    tap(&mut h, RESHAPE);
    h.state_mut().globe.snap.on = false;
    pen_way(
        &mut h,
        &[[-4.0, 10.0], [-4.0, 14.0], [4.0, 14.0], [4.0, 10.0]],
    );
    step_and_save(&mut h, false);
    assert_eq!(line_features(&h, picks.line.layer).len(), 1);
    let vale_store::Geometry::Lines(parts) = geometry_of(&h, picks.line) else {
        panic!("not a line");
    };
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0][0], [-8.0, 10.0]);
    assert_eq!(parts[0][parts[0].len() - 1], [8.0, 10.0]);
    assert!(parts[0].iter().any(|p| p[1] > 13.5));
    let reshaped = geometry_of(&h, picks.line);

    // A stroke away from the line draws a new line.
    pen_way(&mut h, &[[-4.0, 20.0], [4.0, 20.0]]);
    step_and_save(&mut h, false);
    assert_eq!(line_features(&h, picks.line.layer).len(), 2);

    // With Reshape off, a stroke from the line draws a new line too.
    tap(&mut h, RESHAPE);
    assert!(!h.state().globe.line.reshape);
    pen_way(&mut h, &[[0.0, 10.0], [0.0, 17.0]]);
    step_and_save(&mut h, false);
    assert_eq!(line_features(&h, picks.line.layer).len(), 3);
    assert_eq!(geometry_of(&h, picks.line), reshaped);
}

#[test]
fn a_finger_tap_selects_a_small_point_a_line_and_a_polygon_in_each_view() {
    for view in WORLD_VIEWS {
        let dir = tempfile::tempdir().unwrap();
        let (mut h, picks) = select_pad(view, dir.path());
        let point = screen_at(&h, 0.0, 0.0);
        // A finger reaches farther than a pen.
        let off = point + vec2(18.0, 0.0);
        pen_tap_at(&mut h, off);
        assert_eq!(selection(&h), [], "{view:?}");
        finger_tap_at(&mut h, off);
        assert_eq!(selection(&h), [picks.point], "{view:?}");
        // A tap selects in the selected layer only.
        let line = screen_at(&h, 2.0, 10.0) - vec2(0.0, 10.0);
        finger_tap_at(&mut h, line);
        assert_eq!(selection(&h), [], "{view:?}");
        select_layer(&mut h, picks.line.layer);
        finger_tap_at(&mut h, line);
        assert_eq!(selection(&h), [picks.line], "{view:?}");
        // Another layer clears the selection.
        select_layer(&mut h, picks.polygon.layer);
        assert_eq!(selection(&h), [], "{view:?}");
        let inside = screen_at(&h, -2.0, -18.0);
        finger_tap_at(&mut h, inside);
        assert_eq!(selection(&h), [picks.polygon], "{view:?}");
        // A tap on empty ground clears the selection.
        let empty = screen_at(&h, 30.0, 0.0);
        finger_tap_at(&mut h, empty);
        assert_eq!(selection(&h), [], "{view:?}");
    }
}

#[test]
fn a_pen_drag_makes_a_box_and_a_finger_drag_moves_the_view() {
    for view in WORLD_VIEWS {
        let dir = tempfile::tempdir().unwrap();
        let (mut h, picks) = select_pad(view, dir.path());
        // The box holds the two points and crosses the line between its
        // vertices.
        let (from, to) = (screen_at(&h, -3.0, 14.0), screen_at(&h, 3.0, -3.0));
        drag(&mut h, from, to, Some(0.5));
        let mut found = selection(&h);
        found.sort_by_key(|f| f.uuid);
        let mut points = [picks.point, picks.second];
        points.sort_by_key(|f| f.uuid);
        assert_eq!(found, points, "{view:?}");
        select_layer(&mut h, picks.line.layer);
        drag(&mut h, from, to, Some(0.5));
        assert_eq!(selection(&h), [picks.line], "{view:?}");
        let before = screen_at(&h, 0.0, 0.0);
        drag(&mut h, from, from + vec2(80.0, 0.0), None);
        assert_ne!(screen_at(&h, 0.0, 0.0), before, "{view:?}");
        assert_eq!(selection(&h), [picks.line], "{view:?}");
    }
}

#[test]
fn a_pen_drag_on_a_selected_point_moves_it_in_one_undo_step() {
    use vale_store::Geometry;
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    let point = screen_at(&h, 0.0, 0.0);
    finger_tap_at(&mut h, point);
    assert_eq!(selection(&h), [picks.point]);
    let saved = saves(&h);
    let to = point + vec2(60.0, 30.0);
    drag(&mut h, point, to, Some(0.5));
    assert_eq!(saves(&h), saved + 1);
    let features = line_features(&h, picks.point.layer);
    assert_eq!(features.len(), 2);
    assert_eq!(features[0].uuid(), picks.point.uuid);
    let Geometry::Points(points) = &features[0].geometry else {
        panic!("not a point");
    };
    let p = points[0];
    let moved = screen_at(&h, p[0], p[1]);
    assert!(moved.distance(to) < 0.5, "{moved:?} {to:?}");
    // The move made no box, so the selection stays.
    assert_eq!(selection(&h), [picks.point]);
    h.state_mut().undo();
    h.run_steps(2);
    let features = line_features(&h, picks.point.layer);
    assert_eq!(features[0].geometry, Geometry::Points(vec![[0.0, 0.0]]));
}

#[test]
fn a_selection_opens_no_card_over_the_canvas() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    let at = screen_at(&h, 0.0, 0.0);
    finger_tap_at(&mut h, at);
    assert_eq!(selection(&h), [picks.point]);
    assert!(h.state().pad.panels.is_empty());
    assert!(h.state().selection_form.is_none());
    assert!(control(&h, "name").is_none());
    assert!(control(&h, SHOW_IN_TABLE).is_none());
    // The one control of the selection is in the card below the strip.
    let card = context_card(&h);
    assert!(card.contains_rect(control(&h, select::DELETE).unwrap()));
}

#[test]
fn the_select_card_has_delete_selection_and_it_is_off_with_no_selection() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    assert!(disabled(&h, select::DELETE));
    let (from, to) = (screen_at(&h, -3.0, 14.0), screen_at(&h, 3.0, -3.0));
    drag(&mut h, from, to, Some(0.5));
    assert_eq!(selection(&h).len(), 2);
    assert!(!disabled(&h, select::DELETE));
    check_controls(&h, "the card of the Select tool");
    let saved = saves(&h);
    tap(&mut h, select::DELETE);
    assert_eq!(saves(&h), saved + 1);
    assert!(line_features(&h, picks.point.layer).is_empty());
    assert_eq!(line_features(&h, picks.line.layer).len(), 1);
    assert_eq!(selection(&h), []);
    assert!(disabled(&h, select::DELETE));
    h.state_mut().undo();
    h.run_steps(2);
    assert_eq!(line_features(&h, picks.point.layer).len(), 2);
    // The other tools have no Delete selection.
    quick_tap(&mut h, "Pan");
    assert!(h.query_by_label(select::DELETE).is_none());
}

#[test]
fn merge_joins_two_provinces_into_the_feature_that_you_choose_in_one_undo_step() {
    use std::collections::HashSet;
    use vale_store::{Feature, Field, FieldType, Geometry, Value};
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    let id = picks.polygon.layer;
    let small = {
        let state = h.state_mut();
        let ring = vec![[10.0, -25.0], [16.0, -25.0], [16.0, -12.0], [10.0, -12.0]];
        let feature = Feature::new(Geometry::Polygons(vec![vec![ring]]), Default::default());
        let uuid = feature.uuid();
        let project = &mut state.doc.project;
        assert!(project.add_feature(id, feature));
        assert!(project.add_field(id, Field::new("name", FieldType::Text)));
        for (u, name) in [(picks.polygon.uuid, "Big"), (uuid, "Small")] {
            let one = HashSet::from([u]);
            let value = Value::Text(name.into());
            assert_eq!(project.set_attribute_of(id, &one, "name", &value), 1);
        }
        state.doc.entry_mut(id).unwrap().style.label.text = "'Isle ' || name".into();
        state.save_waiting().unwrap();
        uuid
    };
    select_layer(&mut h, id);
    assert!(disabled(&h, select::MERGE));
    h.state_mut().set_feature_selection(vec![FeatureRef {
        layer: id,
        uuid: small,
    }]);
    h.run_steps(2);
    assert!(disabled(&h, select::MERGE));
    let (from, to) = (screen_at(&h, -14.0, -8.0), screen_at(&h, 20.0, -28.0));
    drag(&mut h, from, to, Some(0.5));
    assert_eq!(selection(&h).len(), 2);
    assert!(!disabled(&h, select::MERGE));
    tap(&mut h, select::MERGE);
    // The list names each feature by the label text, the largest first.
    let big_row = control(&h, "Isle Big").unwrap();
    let small_row = control(&h, "Isle Small").unwrap();
    assert!(big_row.top() < small_row.top());
    let card = context_card(&h);
    assert!(big_row.left() > card.right());
    check_controls(&h, "the list of Merge");
    screenshot::save(&mut h, "pad-merge");
    let saved = saves(&h);
    tap(&mut h, "Isle Small");
    assert_eq!(saves(&h), saved + 1);
    assert!(!h.state().merging);
    let features = line_features(&h, id);
    assert_eq!(features.len(), 1);
    assert_eq!(features[0].uuid(), small);
    assert_eq!(
        features[0].attributes.get("name"),
        Some(&Value::Text("Small".into()))
    );
    let Geometry::Polygons(p) = &features[0].geometry else {
        panic!("not a polygon");
    };
    // The border between the two goes away.
    assert_eq!((p.len(), p[0].len()), (1, 1), "{p:?}");
    assert_eq!(
        selection(&h),
        [FeatureRef {
            layer: id,
            uuid: small
        }]
    );
    h.state_mut().undo();
    h.run_steps(2);
    assert_eq!(line_features(&h, id).len(), 2);
}

#[test]
fn a_touch_outside_the_list_of_merge_closes_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    set_ranks(&mut h, &picks, [1.0, 2.0]);
    h.state_mut()
        .set_feature_selection(vec![picks.point, picks.second]);
    h.run_steps(2);
    tap(&mut h, select::MERGE);
    assert!(h.state().merging);
    assert!(control(&h, "Feature 1").is_some());
    let at = h.state().globe.rect.right_bottom() - vec2(40.0, 40.0);
    finger_tap_at(&mut h, at);
    assert!(!h.state().merging);
    assert!(control(&h, "Feature 1").is_none());
    assert_eq!(line_features(&h, picks.point.layer).len(), 2);
}

#[test]
fn merge_does_not_ask_when_the_features_have_the_same_attributes() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    set_ranks(&mut h, &picks, [3.0, 3.0]);
    h.state_mut()
        .set_feature_selection(vec![picks.second, picks.point]);
    h.run_steps(2);
    let saved = saves(&h);
    tap(&mut h, select::MERGE);
    assert!(!h.state().merging);
    assert!(control(&h, select::MERGE_TITLE).is_none());
    assert_eq!(saves(&h), saved + 1);
    let features = line_features(&h, picks.point.layer);
    assert_eq!(features.len(), 1);
    // The two points are of one size, so the first in the draw order stays.
    assert_eq!(features[0].uuid(), picks.point.uuid);
    assert_eq!(
        features[0].attributes.get("rank"),
        Some(&vale_store::Value::Number(3.0))
    );
    h.state_mut().undo();
    h.run_steps(2);
    assert_eq!(line_features(&h, picks.point.layer).len(), 2);
}

/// Gives the two points of `select_pad` a field "rank" with two values.
fn set_ranks(h: &mut Pad, picks: &Picks, ranks: [f64; 2]) {
    use std::collections::HashSet;
    use vale_store::{Field, FieldType, Value};
    let id = picks.point.layer;
    let state = h.state_mut();
    let project = &mut state.doc.project;
    assert!(project.add_field(id, Field::new("rank", FieldType::Real)));
    for (f, rank) in [picks.point, picks.second].into_iter().zip(ranks) {
        let one = HashSet::from([f.uuid]);
        let value = Value::Number(rank);
        assert_eq!(project.set_attribute_of(id, &one, "rank", &value), 1);
    }
    state.save_waiting().unwrap();
    h.run_steps(2);
}

#[test]
fn the_attributes_button_opens_and_closes_the_inspector_and_it_stays_open_in_each_tool() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    let open = |h: &Pad| panel(h, Panel::Attributes).is_some();
    open_panel(&mut h, select::ATTRIBUTES);
    assert!(open(&h));
    assert!(h.state().pad.is_open(Panel::Attributes));
    check_controls(&h, "the Attributes panel");
    // Select.
    let at = screen_at(&h, 0.0, 0.0);
    finger_tap_at(&mut h, at);
    assert!(open(&h));
    assert_eq!(
        h.state().selection_form.as_ref().unwrap().features,
        [picks.point]
    );
    // Draw: the line tool draws a line.
    select_layer(&mut h, picks.line.layer);
    quick_tap(&mut h, VECTOR);
    assert_eq!(h.state().globe.tool, Tool::Line);
    let lines = line_features(&h, picks.line.layer).len();
    let (from, to) = (screen_at(&h, -6.0, -4.0), screen_at(&h, 6.0, -4.0));
    drag(&mut h, from, to, Some(0.5));
    h.run_steps(4);
    assert_eq!(line_features(&h, picks.line.layer).len(), lines + 1);
    assert!(open(&h));
    // Paint: the brush paints the height layer.
    let raster = h.state().doc.project.rasters()[0].id;
    let layers = h.state().doc.project.layers();
    let height = layers.iter().find(|l| l.kind == LayerKind::Raster(raster));
    let height = height.unwrap().id;
    h.state_mut().doc.project.set_layer_visible(height, true);
    h.state_mut().select(Some(LayerRef::Raster(raster)));
    h.run_steps(2);
    quick_tap(&mut h, SCULPT);
    assert_eq!(h.state().globe.tool, Tool::Brush);
    drag(&mut h, from, to, Some(0.5));
    assert!(open(&h));
    open_panel(&mut h, select::ATTRIBUTES);
    assert!(!open(&h));
}

#[test]
fn with_one_feature_the_inspector_edits_each_attribute_in_one_undo_step() {
    use vale_store::{Field, FieldType, Value};
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    let id = picks.point.layer;
    let field = Field::new("people", FieldType::Integer);
    assert!(h.state_mut().doc.project.add_field(id, field));
    h.state_mut().save_waiting().unwrap();
    open_panel(&mut h, select::ATTRIBUTES);
    let at = screen_at(&h, 0.0, 0.0);
    finger_tap_at(&mut h, at);
    // A new point layer has a name field.
    let form = h.state().selection_form.as_ref().expect("no form");
    assert_eq!(form.features, [picks.point]);
    let names: Vec<_> = form.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["name", "people"]);
    let saved = saves(&h);
    tap(&mut h, "name");
    h.event(egui::Event::Text("Avalon".into()));
    h.run_steps(2);
    // The change is one undo step when the field loses the focus.
    assert_eq!(saves(&h), saved);
    tap(&mut h, "people");
    assert_eq!(saves(&h), saved + 1);
    let attributes = |h: &Pad| line_features(h, id)[0].attributes.clone();
    assert_eq!(attributes(&h)["name"], Value::Text("Avalon".into()));
    // A value that is not of the field type has the warning color, and it
    // does not go in the feature.
    h.event(egui::Event::Text("many".into()));
    h.run_steps(2);
    let warned = painted_shapes(&h).iter().any(|shape| match shape {
        egui::Shape::Text(text) => {
            text.galley.text() == "many"
                && text
                    .galley
                    .job
                    .sections
                    .iter()
                    .all(|s| s.format.color == theme::WARN)
        }
        _ => false,
    });
    assert!(warned);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(saves(&h), saved + 1);
    assert_eq!(attributes(&h)["people"], Value::Null);
    h.state_mut().undo();
    h.run_steps(2);
    assert_eq!(attributes(&h)["name"], Value::Null);
    let form = h.state().selection_form.as_ref().unwrap();
    assert_eq!(form.fields[0].text, "");
}

/// Adds point features far from the picks, with these names.
fn add_named_points(h: &mut Pad, id: vale_store::LayerId, names: &[&str]) {
    use vale_store::{Feature, Geometry, Value};
    for (i, name) in names.iter().enumerate() {
        let at = [40.0 + i as f64, -40.0];
        let attributes = [("name".to_owned(), Value::Text((*name).into()))];
        let feature = Feature::new(Geometry::Points(vec![at]), attributes.into());
        assert!(h.state_mut().doc.project.add_feature(id, feature));
    }
    h.state_mut().save_waiting().unwrap();
    h.run_steps(2);
}

#[test]
fn a_field_lists_the_values_of_the_layer_and_a_tap_on_a_value_is_one_undo_step() {
    use vale_store::Value;
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    let id = picks.point.layer;
    add_named_points(&mut h, id, &["small", "large", "Medium", "small"]);
    open_panel(&mut h, select::ATTRIBUTES);
    let at = screen_at(&h, 0.0, 0.0);
    finger_tap_at(&mut h, at);
    let form = h.state().selection_form.as_ref().expect("no form");
    assert_eq!(form.choices[0], ["large", "Medium", "small"]);
    let button = vale_app::ui::pad::widgets::choices_name("name");
    assert!(control(&h, &button).is_some());
    assert!(control(&h, "name: small").is_none());
    // The menu opens below the field.
    tap(&mut h, &button);
    let field = control(&h, "name").unwrap();
    let rows = ["name: large", "name: Medium", "name: small"].map(|r| control(&h, r).unwrap());
    assert!(rows.windows(2).all(|w| w[0].top() < w[1].top()));
    assert!(rows[0].top() >= field.bottom());
    check_controls(&h, "the list of values");
    assert_eq!(toggled(&h, "name: Medium"), Some(false));
    let saved = saves(&h);
    tap(&mut h, "name: Medium");
    let name = |h: &Pad| {
        let features = line_features(h, id);
        let feature = features.iter().find(|f| f.uuid() == picks.point.uuid);
        feature.unwrap().attributes["name"].clone()
    };
    assert_eq!(name(&h), Value::Text("Medium".into()));
    assert_eq!(saves(&h), saved + 1);
    assert!(control(&h, "name: small").is_none());
    // The mark shows the current value.
    tap(&mut h, &button);
    assert_eq!(toggled(&h, "name: Medium"), Some(true));
    assert_eq!(toggled(&h, "name: small"), Some(false));
    screenshot::save(&mut h, "pad-field-values");
    // A tap outside the menu closes it.
    tap(&mut h, "name");
    assert!(control(&h, "name: small").is_none());
    assert!(h.state().selection_form.as_ref().unwrap().open.is_none());
    h.state_mut().undo();
    h.run_steps(2);
    assert_eq!(name(&h), Value::Null);
}

#[test]
fn a_field_with_no_values_or_with_too_many_values_has_no_list() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    open_panel(&mut h, select::ATTRIBUTES);
    let at = screen_at(&h, 0.0, 0.0);
    finger_tap_at(&mut h, at);
    let button = vale_app::ui::pad::widgets::choices_name("name");
    assert!(control(&h, "name").is_some());
    assert!(control(&h, &button).is_none());
    let names: Vec<String> = (0..=select::CHOICES).map(|i| format!("Town {i}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    add_named_points(&mut h, picks.point.layer, &names);
    finger_tap_at(&mut h, at);
    assert!(h.state().selection_form.as_ref().unwrap().choices[0].is_empty());
    assert!(control(&h, &button).is_none());
}

#[test]
fn the_inspector_shows_the_count_of_a_selection_and_an_empty_state() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _) = select_pad(WorldView::Globe, dir.path());
    open_panel(&mut h, select::ATTRIBUTES);
    assert!(h.query_by_label(select::NO_SELECTION).is_some());
    let (from, to) = (screen_at(&h, -3.0, 14.0), screen_at(&h, 3.0, -3.0));
    drag(&mut h, from, to, Some(0.5));
    assert_eq!(selection(&h).len(), 2);
    assert!(h.query_by_label(&select::title(2)).is_some());
    assert!(h.query_by_label(select::NO_SELECTION).is_none());
    assert!(control(&h, "name").is_some());
    tap(&mut h, DESELECT);
    assert!(h.query_by_label(select::NO_SELECTION).is_some());
}

#[test]
fn with_many_features_an_edit_of_a_field_sets_it_in_each_feature_in_one_undo_step() {
    use vale_store::{Field, FieldType, Value};
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    let id = picks.point.layer;
    let project = &mut h.state_mut().doc.project;
    assert!(project.add_field(id, Field::new("size", FieldType::Text)));
    for (f, name) in [(picks.point, "Avalon"), (picks.second, "Brook")] {
        let values = [
            ("name".to_owned(), Value::Text(name.into())),
            ("size".to_owned(), Value::Text("small".into())),
        ];
        assert!(project.set_attributes(id, f.uuid, values));
    }
    h.state_mut().save_waiting().unwrap();
    open_panel(&mut h, select::ATTRIBUTES);
    let (from, to) = (screen_at(&h, -3.0, 14.0), screen_at(&h, 3.0, -3.0));
    drag(&mut h, from, to, Some(0.5));
    assert_eq!(selection(&h).len(), 2);
    // A field with different values is empty and shows "Mixed".
    let form = h.state().selection_form.as_ref().expect("no form");
    assert_eq!(form.features.len(), 2);
    assert_eq!(
        (form.fields[0].text.as_str(), form.fields[0].mixed),
        ("", true)
    );
    assert_eq!(
        (form.fields[1].text.as_str(), form.fields[1].mixed),
        ("small", false)
    );
    let mixed = painted_shapes(&h).iter().any(|shape| match shape {
        egui::Shape::Text(text) => text.galley.text() == select::MIXED,
        _ => false,
    });
    assert!(mixed);
    screenshot::save(&mut h, "pad-mixed-field");
    let values = |h: &Pad, name: &str| -> Vec<Value> {
        let features = line_features(h, id);
        [picks.point, picks.second]
            .iter()
            .map(|f| features.iter().find(|x| x.uuid() == f.uuid).unwrap())
            .map(|x| x.attributes[name].clone())
            .collect()
    };
    // A mixed field that loses the focus with no edit changes nothing.
    let saved = saves(&h);
    tap(&mut h, "name");
    tap(&mut h, "size");
    assert_eq!(saves(&h), saved);
    let names = [Value::Text("Avalon".into()), Value::Text("Brook".into())];
    assert_eq!(values(&h, "name"), names);
    tap(&mut h, "name");
    h.event(egui::Event::Text("Camelot".into()));
    h.run_steps(2);
    tap(&mut h, "size");
    assert_eq!(saves(&h), saved + 1);
    assert_eq!(values(&h, "name"), vec![Value::Text("Camelot".into()); 2]);
    assert_eq!(values(&h, "size"), vec![Value::Text("small".into()); 2]);
    h.state_mut().undo();
    h.run_steps(2);
    assert_eq!(values(&h, "name"), names);
}

#[test]
fn after_an_edit_a_tap_on_another_feature_puts_the_focus_in_the_same_field() {
    use vale_store::Value;
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    let id = picks.point.layer;
    open_panel(&mut h, select::ATTRIBUTES);
    let at = screen_at(&h, 0.0, 0.0);
    pen_tap_at(&mut h, at);
    tap(&mut h, "name");
    h.event(egui::Event::Text("Avalon".into()));
    h.run_steps(2);
    let name = |h: &Pad, f: FeatureRef| {
        let features = line_features(h, id);
        let feature = features.iter().find(|x| x.uuid() == f.uuid).unwrap();
        feature.attributes["name"].clone()
    };
    for (f, text) in [(picks.second, "Lyonesse"), (picks.point, "Ys")] {
        let [lon, lat] = if f == picks.point {
            [0.0, 0.0]
        } else {
            [-2.0, 2.0]
        };
        let at = screen_at(&h, lon, lat);
        pen_tap_at(&mut h, at);
        assert_eq!(selection(&h), [f]);
        assert!(h.ctx.memory(|m| m.focused()).is_some(), "{text}");
        h.event(egui::Event::Text(text.into()));
        h.run_steps(2);
    }
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(name(&h, picks.second), Value::Text("Lyonesse".into()));
    assert_eq!(name(&h, picks.point), Value::Text("AvalonYs".into()));
    // An empty selection ends the run of edits.
    tap(&mut h, DESELECT);
    let at = screen_at(&h, -2.0, 2.0);
    pen_tap_at(&mut h, at);
    assert!(no_keyboard(&h));
}

#[test]
fn below_600_points_the_inspector_is_a_sheet() {
    for size in BOARDS {
        let dir = tempfile::tempdir().unwrap();
        let (mut h, _) = select_pad(WorldView::Globe, dir.path());
        h.set_size(vec2(size.0, size.1));
        h.run_steps(3);
        open_panel(&mut h, select::ATTRIBUTES);
        let rects = h.state().pad.rects.unwrap();
        let rect = panel(&h, Panel::Attributes).expect("no panel");
        if size.0 < 600.0 {
            let sheet = rects.sheet.expect("no sheet");
            assert_eq!(rect, sheet, "{size:?}");
        } else {
            assert!(rects.sheet.is_none(), "{size:?}");
            assert_eq!(rect.right(), rects.screen.right(), "{size:?}");
        }
        check_controls(&h, &format!("{size:?}: Attributes"));
    }
}

#[test]
#[ignore = "gpu"]
fn manual_the_number_controls_of_the_brush_panel() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Brush settings");
    choose_mode(&mut h, "Raise");
    tap(&mut h, "Brush ceiling");
    screenshot::save(&mut h, "numbers-stepper");
    // A drag that moves away from the track is slower.
    let from = control(&h, "Brush size").unwrap().center();
    touch(&mut h, egui::TouchPhase::Start, from, None);
    h.step();
    for at in [vec2(20.0, 0.0), vec2(40.0, 0.0), vec2(60.0, 200.0)] {
        touch(&mut h, egui::TouchPhase::Move, from + at, None);
        h.step();
    }
    screenshot::save(&mut h, "numbers-drag");
    touch(
        &mut h,
        egui::TouchPhase::End,
        from + vec2(60.0, 200.0),
        None,
    );
    h.run_steps(3);
    tap(&mut h, "Brush size value");
    screenshot::save(&mut h, "numbers-keypad");
    tap(&mut h, "Brush size value");
    // The bubble of the card goes past the open Brush panel.
    let from = control(&h, "Size").unwrap().center();
    touch(&mut h, egui::TouchPhase::Start, from, None);
    h.step();
    for at in [vec2(0.0, -20.0), vec2(0.0, -40.0)] {
        touch(&mut h, egui::TouchPhase::Move, from + at, None);
        h.step();
    }
    screenshot::save(&mut h, "numbers-card-drag");
    touch(&mut h, egui::TouchPhase::End, from + vec2(0.0, -40.0), None);
    h.run_steps(3);
}

#[test]
fn the_vector_cell_lists_the_tools_of_the_kind_of_the_selected_layer() {
    use vale_app::ui::pad::{POINT, SPLIT, VERTEX, vector_tools};
    let names = |kind| {
        vector_tools(kind)
            .iter()
            .map(|t| t.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(GeometryKind::Line), [LINE, VERTEX, SPLIT]);
    assert_eq!(names(GeometryKind::Polygon), [POLYGON, VERTEX, SPLIT]);
    assert_eq!(names(GeometryKind::Point), [POINT]);
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    for (layer, kind) in [
        (picks.line.layer, GeometryKind::Line),
        (picks.polygon.layer, GeometryKind::Polygon),
    ] {
        select_layer(&mut h, layer);
        quick_tap(&mut h, "Pan");
        quick_tap(&mut h, VECTOR);
        assert_eq!(h.state().pad.flyouts.open, None, "{kind:?}");
        // A second tap opens the list, with a row for each tool.
        quick_tap(&mut h, VECTOR);
        assert_eq!(h.state().pad.flyouts.open, Some(Flyout::Vector));
        screenshot::save(&mut h, &format!("pad-vector-{kind:?}"));
        let list = h.state().pad.flyouts.rect.expect("a list");
        for name in names(kind) {
            let row = control(&h, name).unwrap_or_else(|| panic!("{name}"));
            assert!(list.contains(row.center()), "{kind:?}: {name}");
        }
        tap(&mut h, SPLIT);
        assert_eq!(h.state().globe.tool, Tool::Split, "{kind:?}");
        assert_eq!(h.state().pad.flyouts.open, None);
        assert_eq!(h.state().pad.vector_face, Some(SPLIT));
        assert_eq!(toggled(&h, VECTOR), Some(true));
        // A hold opens the list too, and the tool that draws is in it.
        hold_cell(&mut h, VECTOR);
        assert_eq!(h.state().pad.flyouts.open, Some(Flyout::Vector));
        tap(&mut h, names(kind)[0]);
        assert_eq!(h.state().pad.vector_face, Some(names(kind)[0]));
    }
    // A point layer has one tool and no list.
    select_layer(&mut h, picks.point.layer);
    quick_tap(&mut h, VECTOR);
    quick_tap(&mut h, VECTOR);
    assert_eq!(h.state().globe.tool, Tool::Point);
    assert_eq!(h.state().pad.flyouts.open, None);
}

/// Chooses Split in the list of the Vector cell.
fn choose_split(h: &mut Pad) {
    use vale_app::ui::pad::SPLIT;
    hold_cell(h, VECTOR);
    tap(h, SPLIT);
    assert_eq!(h.state().globe.tool, Tool::Split);
}

#[test]
fn split_stays_the_tool_in_a_layer_that_has_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    select_layer(&mut h, picks.line.layer);
    choose_split(&mut h);
    select_layer(&mut h, picks.polygon.layer);
    assert_eq!(h.state().globe.tool, Tool::Split);
    // A point layer has no Split, so its tool that draws is the tool.
    select_layer(&mut h, picks.point.layer);
    assert_eq!(h.state().globe.tool, Tool::Point);
    select_layer(&mut h, picks.line.layer);
    assert_eq!(h.state().globe.tool, Tool::Line);
}

#[test]
fn the_context_strip_of_split_has_smoothing_and_snap() {
    use vale_app::ui::pad::{RESHAPE, SPLIT};
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    select_layer(&mut h, picks.polygon.layer);
    choose_split(&mut h);
    for name in [SMOOTHING, SNAP] {
        assert!(control(&h, name).is_some(), "{name}");
    }
    for name in [RESHAPE, vale_app::ui::pad::POLYGON_MODE] {
        assert!(control(&h, name).is_none(), "{name}");
    }
    check_controls(&h, SPLIT);
}

#[test]
fn split_cuts_each_feature_that_the_stroke_crosses_in_one_undo_step() {
    use vale_store::{Feature, Field, FieldType, Geometry, Value};
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    let id = picks.polygon.layer;
    let state = h.state_mut();
    let project = &mut state.doc.project;
    assert!(project.add_field(id, Field::new("name", FieldType::Text)));
    let named = |name: &str| [("name".to_string(), Value::Text(name.into()))];
    assert!(project.set_attributes(id, picks.polygon.uuid, named("Vale")));
    let east = vec![vec![
        [12.0, -25.0],
        [20.0, -25.0],
        [20.0, -12.0],
        [12.0, -12.0],
    ]];
    let other = Feature::new(Geometry::Polygons(vec![east]), named("Ost").into());
    assert!(project.add_feature(id, other));
    // A third polygon away from the stroke stays.
    let away = vec![vec![
        [-30.0, -25.0],
        [-20.0, -25.0],
        [-20.0, -12.0],
        [-30.0, -12.0],
    ]];
    assert!(project.add_feature(
        id,
        Feature::new(Geometry::Polygons(vec![away]), named("Far").into())
    ));
    state.save_waiting().unwrap();
    select_layer(&mut h, id);
    choose_split(&mut h);
    h.state_mut().globe.snap.on = false;
    // The selection does not limit the split.
    h.state_mut().feature_selection = vec![picks.polygon];
    step_and_save(&mut h, false);
    let before = line_features(&h, id);
    assert_eq!(before.len(), 3);
    let saved = saves(&h);
    // From south to north across the two polygons in the east.
    pen_way(&mut h, &[[0.0, -28.0], [16.0, -18.0], [16.0, -9.0]]);
    for _ in 0..3 {
        step_and_save(&mut h, false);
    }
    assert_eq!(saves(&h), saved + 1);
    let after = line_features(&h, id);
    assert_eq!(after.len(), 5);
    let olds: Vec<_> = before.iter().map(|f| f.uuid()).collect();
    let new: Vec<_> = after.iter().filter(|f| !olds.contains(&f.uuid())).collect();
    assert_eq!(new.len(), 4);
    let names: Vec<_> = new.iter().map(|f| f.attributes["name"].clone()).collect();
    for name in ["Vale", "Ost"] {
        let n = names
            .iter()
            .filter(|v| **v == Value::Text(name.into()))
            .count();
        assert_eq!(n, 2, "{name}");
    }
    assert!(
        after
            .iter()
            .any(|f| f.attributes["name"] == Value::Text("Far".into()))
    );
    // The parts become the selection.
    let selected: Vec<_> = h.state().feature_selection.iter().map(|f| f.uuid).collect();
    let parts: Vec<_> = new.iter().map(|f| f.uuid()).collect();
    assert_eq!(selected, parts);
    // The two parts of the square share the cut.
    let vale: Vec<_> = (new.iter())
        .filter(|f| f.attributes["name"] == Value::Text("Vale".into()))
        .collect();
    let probes = [[-9.0, -24.0], [9.0, -13.0], [5.0, -20.0], [9.0, -24.0]];
    for p in probes {
        let n = vale
            .iter()
            .filter(|f| inside_polygon(&f.geometry, p[0], p[1]))
            .count();
        assert_eq!(n, 1, "{p:?}");
    }
    tap(&mut h, "Undo");
    assert_eq!(line_features(&h, id), before);
    tap(&mut h, "Redo");
    assert_eq!(line_features(&h, id), after);
}

#[test]
fn a_held_finger_adds_a_second_tap_makes_the_key_and_subtract_cuts_in_one_undo_step() {
    use vale_app::ui::select::{ADD_TO_SELECTION, SUBTRACT, Selectable};
    use vale_store::{Feature, Geometry};
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    let country = picks.polygon;
    let state = h.state_mut();
    let water = state.doc.add_layer(GeometryKind::Polygon);
    let lake = vec![vec![
        [-4.0, -21.0],
        [4.0, -21.0],
        [4.0, -16.0],
        [-4.0, -16.0],
    ]];
    let lake = Feature::new(Geometry::Polygons(vec![lake]), Default::default());
    let uuid = lake.uuid();
    assert!(state.doc.project.add_feature(water, lake));
    let lake = FeatureRef { layer: water, uuid };
    state.selectable = Selectable::Layers([country.layer, water].into());
    state.save_waiting().unwrap();
    step_and_save(&mut h, false);
    let on_lake = screen_at(&h, 0.0, -18.0);
    let pos = screen_at(&h, -8.0, -14.0);
    finger_tap_at(&mut h, pos);
    assert_eq!(selection(&h), [country]);
    assert!(disabled(&h, SUBTRACT));
    // A finger holds still, and a tap of the pen adds the lake.
    let held = screen_at(&h, 40.0, 30.0);
    finger(&mut h, 7, egui::TouchPhase::Start, held);
    h.run_steps(40);
    pen_tap_at(&mut h, on_lake);
    finger(&mut h, 7, egui::TouchPhase::End, held);
    h.run_steps(3);
    assert_eq!(selection(&h), [country, lake]);
    assert_eq!(h.state().key_feature(), None);
    // A second tap on the lake makes it the key.
    pen_tap_at(&mut h, on_lake);
    assert_eq!(selection(&h), [country, lake]);
    assert_eq!(h.state().key_feature(), Some(lake));
    assert!(!disabled(&h, SUBTRACT));
    screenshot::save(&mut h, "pad-select-key");
    let before = geometry_of(&h, country);
    let lake_before = geometry_of(&h, lake);
    let saved = saves(&h);
    tap(&mut h, SUBTRACT);
    for _ in 0..3 {
        step_and_save(&mut h, false);
    }
    assert_eq!(saves(&h), saved + 1);
    let after = geometry_of(&h, country);
    assert!(!inside_polygon(&after, 0.0, -18.0));
    assert!(inside_polygon(&after, -8.0, -18.0));
    assert_eq!(geometry_of(&h, lake), lake_before);
    tap(&mut h, "Undo");
    assert_eq!(geometry_of(&h, country), before);
    tap(&mut h, "Redo");
    assert_eq!(geometry_of(&h, country), after);
    // With Add, a tap takes a selected feature out.
    tap(&mut h, ADD_TO_SELECTION);
    assert!(h.state().select_add);
    finger_tap_at(&mut h, on_lake);
    assert_eq!(selection(&h), [country]);
    assert_eq!(h.state().key_feature(), None);
}

#[test]
fn undo_of_subtract_selects_the_key_and_the_cut_polygon_again() {
    use vale_app::ui::select::{SUBTRACT, Selectable};
    use vale_store::{Feature, Geometry};
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    let country = picks.polygon;
    let state = h.state_mut();
    let water = state.doc.add_layer(GeometryKind::Polygon);
    let lake = vec![vec![
        [-4.0, -21.0],
        [4.0, -21.0],
        [4.0, -16.0],
        [-4.0, -16.0],
    ]];
    let lake = Feature::new(Geometry::Polygons(vec![lake]), Default::default());
    let uuid = lake.uuid();
    assert!(state.doc.project.add_feature(water, lake));
    let lake = FeatureRef { layer: water, uuid };
    state.selectable = Selectable::Layers([country.layer, water].into());
    state.set_feature_selection(vec![country, lake]);
    state.key = Some(lake);
    step_and_save(&mut h, false);
    // The styles of the new layer wait for their save.
    h.state_mut().save_waiting().unwrap();
    let before = geometry_of(&h, country);
    tap(&mut h, SUBTRACT);
    step_and_save(&mut h, false);
    assert_ne!(geometry_of(&h, country), before);
    h.state_mut().set_feature_selection(Vec::new());
    step_and_save(&mut h, false);
    tap(&mut h, "Undo");
    step_and_save(&mut h, false);
    assert_eq!(geometry_of(&h, country), before);
    assert_eq!(selection(&h), [country, lake]);
    assert_eq!(h.state().key_feature(), Some(lake));
}

#[test]
fn split_cuts_a_line_into_two_lines_at_the_crossing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    let id = picks.line.layer;
    select_layer(&mut h, id);
    choose_split(&mut h);
    h.state_mut().globe.snap.on = false;
    step_and_save(&mut h, false);
    // A stroke that does not reach the line changes nothing, and the
    // selection stays.
    h.state_mut().feature_selection = vec![picks.line];
    pen_way(&mut h, &[[0.0, 2.0], [0.0, 6.0]]);
    step_and_save(&mut h, false);
    assert_eq!(line_features(&h, id).len(), 1);
    assert_eq!(h.state().feature_selection, vec![picks.line]);
    pen_way(&mut h, &[[0.0, 6.0], [0.0, 14.0]]);
    step_and_save(&mut h, false);
    let lines = line_features(&h, id);
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().all(|f| f.uuid() != picks.line.uuid));
    assert_eq!(h.state().feature_selection.len(), 2);
    let vale_store::Geometry::Lines(a) = &lines[0].geometry else {
        panic!("not a line");
    };
    let end = a[a.len() - 1].last().unwrap();
    // The line is an arc of a great circle, so it bends north of the
    // parallel.
    assert!(
        end[0].abs() < 0.05 && (end[1] - 10.0).abs() < 0.2,
        "{end:?}"
    );
    tap(&mut h, "Undo");
    assert_eq!(line_features(&h, id).len(), 1);
}

/// Chooses Vertex in the list of the Vector cell.
fn choose_vertex(h: &mut Pad) {
    use vale_app::ui::pad::VERTEX;
    hold_cell(h, VECTOR);
    tap(h, VERTEX);
    assert_eq!(h.state().globe.tool, Tool::Vertex);
}

/// `select_pad` with the Vertex tool in the polygon layer, with no
/// snapping, and the square of the layer as the edited feature.
fn vertex_pad(dir: &std::path::Path) -> (Pad, Picks) {
    let (mut h, picks) = select_pad(WorldView::Globe, dir);
    select_layer(&mut h, picks.polygon.layer);
    choose_vertex(&mut h);
    h.state_mut().globe.snap.on = false;
    let inside = screen_at(&h, 0.0, -18.0);
    finger_tap_at(&mut h, inside);
    assert_eq!(selection(&h), [picks.polygon]);
    (h, picks)
}

/// The distinct vertices of the outer ring of the first polygon.
fn outer_ring(g: &vale_store::Geometry) -> Vec<[f64; 2]> {
    let vale_store::Geometry::Polygons(p) = g else {
        panic!("not a polygon");
    };
    let mut ring = p[0][0].clone();
    if ring.len() > 1 && ring.first() == ring.last() {
        ring.pop();
    }
    ring
}

fn near(a: [f64; 2], b: [f64; 2], tolerance: f64) -> bool {
    (a[0] - b[0]).abs() <= tolerance && (a[1] - b[1]).abs() <= tolerance
}

/// A drag of the mouse from one screen point to another.
fn mouse_drag(h: &mut Pad, from: Pos2, to: Pos2) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.event(egui::Event::PointerMoved(from));
    h.event(button(from, true));
    h.step();
    for i in 1..=6 {
        h.event(egui::Event::PointerMoved(from.lerp(to, i as f32 / 6.0)));
        h.step();
    }
    h.event(button(to, false));
    h.run_steps(3);
}

#[test]
fn vertex_stays_the_tool_between_line_and_polygon_layers() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    select_layer(&mut h, picks.line.layer);
    choose_vertex(&mut h);
    assert!(h.state().globe.vertexing());
    select_layer(&mut h, picks.polygon.layer);
    assert_eq!(h.state().globe.tool, Tool::Vertex);
    select_layer(&mut h, picks.point.layer);
    assert_eq!(h.state().globe.tool, Tool::Point);
}

#[test]
fn the_card_of_vertex_has_its_buttons_at_touch_size() {
    use vale_app::ui::pad::{DELETE_VERTEX, MOVE_SHARED, SMOOTH_VERTICES};
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _) = vertex_pad(dir.path());
    // Delete vertex and Smooth vertices are off with no selected vertex.
    assert!(control(&h, DELETE_VERTEX).is_none());
    let corner = screen_at(&h, 10.0, -12.0);
    finger_tap_at(&mut h, corner);
    assert_eq!(h.state().vertex_selection, [[10.0, -12.0]]);
    assert!(control(&h, SMOOTH_VERTICES).is_none());
    for name in [SNAP, MOVE_SHARED, DELETE_VERTEX] {
        assert!(control(&h, name).is_some(), "{name}");
    }
    check_controls(&h, "the Vertex tool");
    screenshot::save(&mut h, "pad-vertex");
    // The switch is on by default, and a tap turns it off.
    assert!(h.state().globe.vertex.shared);
    assert_eq!(toggled(&h, MOVE_SHARED), Some(true));
    tap(&mut h, MOVE_SHARED);
    assert!(!h.state().globe.vertex.shared);
    // The Brush panel has the snap switches and the smoothing.
    tap(&mut h, "Brush settings");
    for name in [SNAPPING, SNAP_VERTICES, SNAP_EDGES, "Line smoothing"] {
        assert!(row_of(&h, name).is_some(), "{name}");
    }
}

#[test]
fn a_finger_a_pen_and_the_mouse_move_a_vertex_in_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = vertex_pad(dir.path());
    let corners: [[f64; 2]; 3] = [[10.0, -12.0], [-10.0, -12.0], [10.0, -25.0]];
    for (i, (corner, pointer)) in corners
        .into_iter()
        .zip(["finger", "pen", "mouse"])
        .enumerate()
    {
        let before = geometry_of(&h, picks.polygon);
        let to = [corner[0] + 3.0 * corner[0].signum(), corner[1] + 2.0];
        let (from, end) = (
            screen_at(&h, corner[0], corner[1]),
            screen_at(&h, to[0], to[1]),
        );
        let saved = saves(&h);
        match i {
            0 => drag(&mut h, from, end, None),
            1 => drag(&mut h, from, end, Some(0.5)),
            _ => mouse_drag(&mut h, from, end),
        }
        assert_eq!(saves(&h), saved + 1, "{pointer}");
        let ring = outer_ring(&geometry_of(&h, picks.polygon));
        assert!(!ring.contains(&corner), "{pointer}: {ring:?}");
        assert!(
            ring.iter().any(|&v| near(v, to, 0.3)),
            "{pointer}: {ring:?}"
        );
        assert_eq!(ring.len(), 4, "{pointer}");
        h.state_mut().undo();
        h.run_steps(2);
        assert_eq!(geometry_of(&h, picks.polygon), before, "{pointer}");
    }
}

#[test]
fn a_finger_drag_away_from_a_handle_moves_the_view() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = vertex_pad(dir.path());
    let before = geometry_of(&h, picks.polygon);
    let (from, to) = (screen_at(&h, 0.0, -18.0), screen_at(&h, 5.0, -10.0));
    drag(&mut h, from, to, None);
    assert_eq!(geometry_of(&h, picks.polygon), before);
    assert!(screen_at(&h, 0.0, -18.0).distance(from) > 20.0);
}

#[test]
fn a_tap_on_an_edge_adds_a_vertex_and_delete_vertex_deletes_it() {
    use vale_app::ui::pad::DELETE_VERTEX;
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = vertex_pad(dir.path());
    for pointer in ["finger", "pen"] {
        let saved = saves(&h);
        let edge = screen_at(&h, 0.0, -25.0);
        match pointer {
            "finger" => finger_tap_at(&mut h, edge),
            _ => pen_tap_at(&mut h, edge),
        }
        assert_eq!(saves(&h), saved + 1, "{pointer}");
        let ring = outer_ring(&geometry_of(&h, picks.polygon));
        assert_eq!(ring.len(), 5, "{pointer}");
        let added = ring.iter().copied().find(|&v| near(v, [0.0, -25.0], 0.5));
        let added = added.unwrap_or_else(|| panic!("{pointer}: {ring:?}"));
        assert_eq!(h.state().vertex_selection, [added]);
        tap(&mut h, DELETE_VERTEX);
        assert_eq!(saves(&h), saved + 2, "{pointer}");
        assert_eq!(outer_ring(&geometry_of(&h, picks.polygon)).len(), 4);
        assert!(h.state().vertex_selection.is_empty());
    }
}

#[test]
fn a_handle_takes_a_touch_of_44_points() {
    use vale_app::ui::vertex::HANDLE_REACH;
    assert_eq!(2.0 * HANDLE_REACH, 44.0);
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _) = vertex_pad(dir.path());
    // Away from the square, on the diagonal from its corner.
    let out = vec2(1.0, -1.0).normalized();
    let corner = screen_at(&h, 10.0, -12.0);
    finger_tap_at(&mut h, corner + out * 21.0);
    assert_eq!(h.state().vertex_selection, [[10.0, -12.0]]);
    finger_tap_at(&mut h, corner + out * 23.0);
    assert!(h.state().vertex_selection.is_empty());
    pen_tap_at(&mut h, corner + out * 21.0);
    assert_eq!(h.state().vertex_selection, [[10.0, -12.0]]);
}

#[test]
fn a_pen_box_selects_vertices_and_delete_vertex_deletes_them() {
    use vale_app::ui::pad::DELETE_VERTEX;
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = vertex_pad(dir.path());
    let corner = screen_at(&h, 10.0, -12.0);
    drag(
        &mut h,
        corner + vec2(40.0, -40.0),
        corner + vec2(-30.0, 30.0),
        Some(0.5),
    );
    assert_eq!(h.state().vertex_selection, [[10.0, -12.0]]);
    let saved = saves(&h);
    tap(&mut h, DELETE_VERTEX);
    assert_eq!(saves(&h), saved + 1);
    let ring = outer_ring(&geometry_of(&h, picks.polygon));
    assert_eq!(ring.len(), 3);
    assert!(!ring.contains(&[10.0, -12.0]));
}

/// Adds a feature to a layer, and gives the app the frames to see it.
fn add_feature(
    h: &mut Pad,
    layer: vale_store::LayerId,
    geometry: vale_store::Geometry,
) -> FeatureRef {
    let feature = vale_store::Feature::new(geometry, Default::default());
    let uuid = feature.uuid();
    let state = h.state_mut();
    assert!(state.doc.project.add_feature(layer, feature));
    state.touch();
    state.save_waiting().unwrap();
    h.run_steps(2);
    FeatureRef { layer, uuid }
}

#[test]
fn a_move_of_a_shared_vertex_moves_both_polygons_with_the_switch_on() {
    use vale_app::ui::pad::MOVE_SHARED;
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = vertex_pad(dir.path());
    let east = vec![[10.0, -25.0], [30.0, -25.0], [30.0, -12.0], [10.0, -12.0]];
    let east = add_feature(
        &mut h,
        picks.polygon.layer,
        vale_store::Geometry::Polygons(vec![vec![east]]),
    );
    let saved = saves(&h);
    let (from, to) = (screen_at(&h, 10.0, -12.0), screen_at(&h, 12.0, -8.0));
    drag(&mut h, from, to, None);
    assert_eq!(saves(&h), saved + 1, "one undo step");
    let (west_g, east_g) = (geometry_of(&h, picks.polygon), geometry_of(&h, east));
    let moved = outer_ring(&west_g)
        .into_iter()
        .find(|&v| near(v, [12.0, -8.0], 0.3))
        .expect("the moved vertex");
    assert!(outer_ring(&east_g).contains(&moved), "the same position");
    assert!(!outer_ring(&east_g).contains(&[10.0, -12.0]));
    // Each side of the new border is in one polygon.
    for (lon, lat, west) in [
        (10.5, -14.0, true),
        (12.0, -14.0, false),
        (11.0, -9.5, true),
    ] {
        assert_eq!(inside_polygon(&west_g, lon, lat), west, "{lon}, {lat}");
        assert_eq!(inside_polygon(&east_g, lon, lat), !west, "{lon}, {lat}");
    }
    // With the switch off, only the edited polygon changes.
    tap(&mut h, MOVE_SHARED);
    let east_before = geometry_of(&h, east);
    let (from, to) = (screen_at(&h, 10.0, -25.0), screen_at(&h, 8.0, -28.0));
    drag(&mut h, from, to, None);
    assert_eq!(geometry_of(&h, east), east_before);
    let ring = outer_ring(&geometry_of(&h, picks.polygon));
    assert!(!ring.contains(&[10.0, -25.0]));
}

#[test]
fn a_vertex_snaps_to_a_vertex_and_an_edge_and_the_switches_apply() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = vertex_pad(dir.path());
    h.state_mut().globe.vertex.shared = false;
    let east = vec![[14.0, -25.0], [30.0, -25.0], [30.0, -12.0], [14.0, -12.0]];
    add_feature(
        &mut h,
        picks.polygon.layer,
        vale_store::Geometry::Polygons(vec![vec![east]]),
    );
    h.state_mut().globe.snap.on = true;
    let corner = [10.0, -12.0];
    let cases = [
        // To the corner of the other square.
        ([14.0, -12.0], true, true, Some([14.0, -12.0])),
        ([14.0, -12.0], false, false, None),
        // To its west edge.
        ([14.0, -18.0], false, true, Some([14.0, -18.0])),
        ([14.0, -18.0], false, false, None),
    ];
    for (target, vertices, edges, want) in cases {
        let snap = &mut h.state_mut().globe.snap;
        (snap.vertices, snap.edges) = (vertices, edges);
        let before = geometry_of(&h, picks.polygon);
        let from = screen_at(&h, corner[0], corner[1]);
        let to = screen_at(&h, target[0], target[1]) + vec2(-4.0, 3.0);
        drag(&mut h, from, to, Some(0.5));
        let ring = outer_ring(&geometry_of(&h, picks.polygon));
        let moved = ring
            .iter()
            .copied()
            .find(|v| !outer_ring(&before).contains(v))
            .unwrap();
        match want {
            Some(w) if vertices => assert_eq!(moved, w),
            Some(w) => {
                assert!((moved[0] - w[0]).abs() < 1e-6, "{moved:?} is on the edge");
                assert!(near(moved, w, 1.0), "{moved:?}");
            }
            None => assert!(
                (moved[0] - target[0]).abs() > 0.05,
                "{moved:?} did not snap"
            ),
        }
        h.state_mut().undo();
        h.run_steps(2);
    }
}

#[test]
fn smooth_vertices_changes_only_the_selected_part_in_one_undo_step() {
    use vale_app::ui::pad::SMOOTH_VERTICES;
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = select_pad(WorldView::Globe, dir.path());
    let zigzag: Vec<[f64; 2]> = (0..=8)
        .map(|i| [-8.0 + 2.0 * i as f64, if i % 2 == 0 { 10.0 } else { 12.0 }])
        .collect();
    let state = h.state_mut();
    let line = vale_store::Geometry::Lines(vec![zigzag.clone()]);
    assert!(
        state
            .doc
            .project
            .set_geometry(picks.line.layer, picks.line.uuid, line)
    );
    state.touch();
    h.run_steps(2);
    select_layer(&mut h, picks.line.layer);
    choose_vertex(&mut h);
    let on_line = screen_at(&h, -7.0, 11.0);
    finger_tap_at(&mut h, on_line);
    assert_eq!(selection(&h), [picks.line]);
    // A box around the vertices from (-2, 12) to (2, 12).
    let (a, b) = (screen_at(&h, -2.0, 12.0), screen_at(&h, 2.0, 12.0));
    // The box starts away from the handles.
    drag(&mut h, a + vec2(-6.0, -40.0), b + vec2(6.0, 4.0), Some(0.5));
    assert_eq!(h.state().vertex_selection.len(), 2);
    let before = geometry_of(&h, picks.line);
    tap(&mut h, SMOOTH_VERTICES);
    let vale_store::Geometry::Lines(parts) = geometry_of(&h, picks.line) else {
        panic!("not a line");
    };
    let out = &parts[0];
    assert_eq!(out[..4], zigzag[..4]);
    assert_eq!(out[out.len() - 3..], zigzag[6..]);
    assert!(!out.contains(&zigzag[4]), "the middle vertex moved");
    // One undo takes the smooth away.
    h.state_mut().undo();
    h.run_steps(2);
    assert_eq!(geometry_of(&h, picks.line), before);
}

#[test]
fn a_drag_of_a_selected_vertex_moves_all_the_selected_vertices() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, picks) = vertex_pad(dir.path());
    let (top, bottom) = ([10.0, -12.0], [10.0, -25.0]);
    for (i, pointer) in ["finger", "pen", "mouse"].into_iter().enumerate() {
        let before = geometry_of(&h, picks.polygon);
        // A box around the two corners at the east side.
        let (a, b) = (
            screen_at(&h, top[0], top[1]),
            screen_at(&h, bottom[0], bottom[1]),
        );
        drag(
            &mut h,
            a + vec2(40.0, -40.0),
            b + vec2(-30.0, 30.0),
            Some(0.5),
        );
        assert_eq!(h.state().vertex_selection.len(), 2, "{pointer}");
        let saved = saves(&h);
        let (from, to) = (a, screen_at(&h, 13.0, -10.0));
        match i {
            0 => drag(&mut h, from, to, None),
            1 => drag(&mut h, from, to, Some(0.5)),
            _ => mouse_drag(&mut h, from, to),
        }
        assert_eq!(saves(&h), saved + 1, "{pointer}: one undo step");
        let ring = outer_ring(&geometry_of(&h, picks.polygon));
        assert_eq!(ring.len(), 4, "{pointer}: {ring:?}");
        assert!(
            !ring.contains(&top) && !ring.contains(&bottom),
            "{pointer}: {ring:?}"
        );
        assert!(ring.contains(&[-10.0, -12.0]) && ring.contains(&[-10.0, -25.0]));
        let moved: Vec<_> = ring.iter().filter(|v| v[0] > 11.0).collect();
        assert_eq!(moved.len(), 2, "{pointer}: {ring:?}");
        assert!(
            moved.iter().any(|&&v| near(v, [13.0, -10.0], 0.3)),
            "{pointer}"
        );
        assert_eq!(h.state().vertex_selection.len(), 2, "{pointer}");
        h.state_mut().undo();
        h.run_steps(2);
        assert_eq!(geometry_of(&h, picks.polygon), before, "{pointer}");
    }
    // A drag of a vertex that is not selected moves it alone.
    let (a, b) = (
        screen_at(&h, top[0], top[1]),
        screen_at(&h, bottom[0], bottom[1]),
    );
    drag(
        &mut h,
        a + vec2(40.0, -40.0),
        b + vec2(-30.0, 30.0),
        Some(0.5),
    );
    assert_eq!(h.state().vertex_selection.len(), 2);
    let (from, to) = (screen_at(&h, -10.0, -12.0), screen_at(&h, -13.0, -10.0));
    drag(&mut h, from, to, Some(0.5));
    let ring = outer_ring(&geometry_of(&h, picks.polygon));
    assert!(ring.contains(&top) && ring.contains(&bottom), "{ring:?}");
    assert!(!ring.contains(&[-10.0, -12.0]));
    assert_eq!(h.state().vertex_selection.len(), 1);
}

#[test]
#[ignore = "gpu"]
fn a_held_ocean_ridge_has_its_buttons_on_the_canvas() {
    use vale_app::globe::math::lonlat_to_dir;
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let mut h = headless::ui_harness(state(), (1194.0, 834.0), 2.0, setup);
    // The GPU does the work of the ridge when a frame renders.
    h.set_render_every_step(true);
    h.state_mut().add_height();
    h.state_mut().globe.map.fill(meters_to_level(-4000.0));
    h.run_steps(4);
    tap(&mut h, "Brush settings");
    choose_mode(&mut h, "Ocean ridge");
    h.state_mut().globe.brush.ridge_crest_m = -2500.0;
    h.state_mut().globe.pen_down();
    for i in 0..=30 {
        let t = f64::from(i);
        let sample = vale_app::globe::brush::Sample {
            dir: Some(lonlat_to_dir(-25.0 + 1.6 * t, (t - 15.0).abs() * 0.8 - 6.0)),
            flow: 0.5,
            radius: 0.08,
        };
        h.state_mut()
            .globe
            .pen_sample(sample, std::time::Instant::now());
        h.step();
    }
    h.state_mut().globe.pen_up();
    for _ in 0..400 {
        h.step();
        if h.state().globe.held_idle() {
            break;
        }
    }
    h.run_steps(4);
    assert!(h.state().globe.holding().is_some());
    screenshot::save(&mut h, "pad-held-ridge");
    // The buttons are on the canvas, beside the open panel.
    let (_, panel) = h.state().pad.panels[0];
    for name in [panels::APPLY, panels::DISCARD] {
        let rect = control(&h, name).unwrap();
        assert!(!panel.intersects(rect), "{name}: {rect:?}");
    }
    check_controls(&h, "a held ocean ridge");
    tap(&mut h, panels::APPLY);
    for _ in 0..400 {
        h.step();
        if !h.state().globe.busy() {
            break;
        }
    }
    assert!(h.state().globe.holding().is_none());
    assert!(h.state().globe.can_undo());
}

#[test]
fn the_smallest_lake_slider_sets_the_size_of_the_smallest_lake() {
    for size in BOARDS {
        let mut h = painted_pad(size);
        open_height(&mut h);
        scroll_to_control(&mut h, raster::SMALLEST_LAKE);
        slide_to_end(&mut h, raster::SMALLEST_LAKE, true);
        assert_eq!(h.state().globe.preview.lake_km2, 1_000_000.0, "{size:?}");
        slide_to_end(&mut h, raster::SMALLEST_LAKE, false);
        assert_eq!(h.state().globe.preview.lake_km2, 100.0, "{size:?}");
    }
}

#[test]
fn the_mask_page_sets_and_clears_the_mask_of_a_layer() {
    use vale_app::ui::pad::rules::{MASK, MASK_MAX, MASK_OFFSET, NO_MASK};
    use vale_style::Mask;
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let dir = tempfile::tempdir().unwrap();
        let mut h = saved_pad(size, dir.path());
        let rivers = vector(&h, "Rivers");
        let height = vector(&h, "Height");
        let land = vector(&h, "Land");
        let mask = |h: &Pad| h.state().doc.entry(rivers).unwrap().style.style.mask;
        open_panel(&mut h, "Layers");
        tap(&mut h, "Open Rivers");
        scroll_to_control(&mut h, MASK);
        tap(&mut h, MASK);
        assert_eq!(h.state().pad.page, Some(Page::Mask), "{what}");

        // A tap on the height layer sets its land.
        tap(&mut h, "Height");
        save_frames(&mut h);
        assert_eq!(mask(&h), Some(Mask::land(height)), "{what}");
        type_scrolled_keys(&mut h, MASK_MAX, "500");
        save_frames(&mut h);
        let shore = Mask::Raster {
            layer: height,
            min: Some(0.0),
            max: Some(500.0),
            offset: 0.0,
        };
        assert_eq!(mask(&h), Some(shore), "{what}");
        set_number(&mut h, MASK_OFFSET, "-1.5");
        save_frames(&mut h);
        let shore = Mask::Raster {
            layer: height,
            min: Some(0.0),
            max: Some(500.0),
            offset: -1.5,
        };
        assert_eq!(mask(&h), Some(shore), "{what}");

        scroll_to_control(&mut h, "Land");
        tap(&mut h, "Land");
        save_frames(&mut h);
        assert_eq!(mask(&h), Some(Mask::Polygons { layer: land }), "{what}");
        scroll_to_control(&mut h, NO_MASK);
        tap(&mut h, NO_MASK);
        save_frames(&mut h);
        assert_eq!(mask(&h), None, "{what}");

        tap(&mut h, "Undo");
        assert_eq!(mask(&h), Some(Mask::Polygons { layer: land }), "{what}");
    }
}

#[test]
fn a_new_raster_mask_keeps_the_offset_of_the_raster_mask_before_it() {
    use vale_app::ui::pad::rules::keep_offset;
    use vale_store::LayerId;
    use vale_style::Mask;
    let before = Mask::Raster {
        layer: LayerId(1),
        min: Some(0.0),
        max: None,
        offset: -1.0,
    };
    let kept = keep_offset(Mask::land(LayerId(2)), Some(before));
    let want = Mask::Raster {
        layer: LayerId(2),
        min: Some(0.0),
        max: None,
        offset: -1.0,
    };
    assert_eq!(kept, want);
    let polygons = Mask::Polygons { layer: LayerId(3) };
    assert_eq!(keep_offset(polygons, Some(before)), polygons);
    assert_eq!(
        keep_offset(Mask::land(LayerId(2)), None),
        Mask::land(LayerId(2))
    );
}

#[test]
fn a_point_layer_has_no_mask_row() {
    use vale_app::ui::pad::rules::{ADD_RULE, MASK};
    let mut h = pad(BOARDS[0]);
    open_panel(&mut h, "Layers");
    tap(&mut h, "Open Places");
    scroll_to_control(&mut h, ADD_RULE);
    assert!(control(&h, MASK).is_none());
}

#[test]
fn removing_a_layer_removes_the_masks_that_name_it() {
    use vale_style::Mask;
    let mut h = pad(BOARDS[0]);
    let rivers = vector(&h, "Rivers");
    let land = vector(&h, "Land");
    let doc = &mut h.state_mut().doc;
    doc.entry_mut(rivers).unwrap().style.style.mask = Some(Mask::Polygons { layer: land });
    assert!(doc.remove_layer(land));
    assert_eq!(doc.entry(rivers).unwrap().style.style.mask, None);
}

/// The layout of an iPhone 16 in portrait, with a height layer.
fn phone_pad() -> Pad {
    let ((w, h), safe) = IPHONES[0];
    safe_pad((w as f32, h as f32), safe)
}

/// The top of the bottom band of an iPhone 16 in portrait.
const PHONE_BOTTOM: f32 = 852.0 - 34.0;

/// The names of the cells of the tool strip.
const STRIP: [&str; 6] = [SCULPT, TERRAIN, VECTOR, select::TOOL, "Pan", MEASURE];

#[test]
fn on_an_iphone_the_tool_strip_is_a_row_above_the_home_indicator() {
    let mut h = phone_pad();
    // The cells that the selected layer enables are in one row, in their
    // order.
    let row = |h: &Pad| {
        let cells: Vec<Rect> = STRIP.iter().filter_map(|n| control(h, n)).collect();
        for pair in cells.windows(2) {
            assert_eq!(pair[0].center().y, pair[1].center().y);
            assert!(pair[0].right() <= pair[1].left());
        }
        cells.len()
    };
    // The cells follow the selected layer, as in the iPad layout.
    assert!(disabled(&h, VECTOR));
    assert_eq!(row(&h), 5);
    let rivers = vector(&h, "Rivers");
    h.state_mut().select(Some(LayerRef::Vector(rivers)));
    h.run_steps(2);
    assert!(!disabled(&h, VECTOR) && disabled(&h, SCULPT));
    assert_eq!(row(&h), 4);
    let strip = h.state().pad.rects.unwrap().tools;
    assert!(strip.bottom() <= PHONE_BOTTOM);
    assert!(PHONE_BOTTOM - strip.bottom() <= 12.0);
    assert_eq!(strip.center().x, 393.0 / 2.0);
    quick_tap(&mut h, VECTOR);
    assert_eq!(h.state().globe.tool, Tool::Line);
}

#[test]
fn on_an_iphone_the_sliders_and_the_brush_button_are_a_row_above_the_strip() {
    let mut h = phone_pad();
    quick_tap(&mut h, SCULPT);
    let strip = h.state().pad.rects.unwrap().tools;
    let size = control(&h, "Size").unwrap();
    let flow = control(&h, "Flow").unwrap();
    let brush = control(&h, "Brush settings").unwrap();
    for c in [size, flow, brush] {
        assert!(c.bottom() <= strip.top(), "{c:?}");
        assert!(c.height() >= 44.0 && c.width() >= 44.0, "{c:?}");
    }
    assert!(size.right() <= flow.left() && flow.right() <= brush.left());
    assert!((brush.right() - (strip.right() - 4.0)).abs() <= 1.0);
    // The line tool has the Smoothing slider and the Snap button in their
    // places.
    let rivers = vector(&h, "Rivers");
    h.state_mut().select(Some(LayerRef::Vector(rivers)));
    h.run_steps(2);
    quick_tap(&mut h, VECTOR);
    let smoothing = control(&h, SMOOTHING).unwrap();
    let snap = control(&h, SNAP).unwrap();
    assert_eq!(smoothing.left(), size.left());
    assert!(smoothing.right() <= snap.left());
    assert_eq!(snap.center().y, brush.center().y);
    assert!(control(&h, "Size").is_none());
}

#[test]
fn a_drag_along_a_slider_of_the_row_moves_its_value() {
    let mut h = phone_pad();
    quick_tap(&mut h, SCULPT);
    let flow = control(&h, "Flow").unwrap();
    let before = h.state().globe.brush.flow;
    // A tap on the track does not move the value.
    let pos = pos2(flow.left() + 10.0, flow.bottom() - 10.0);
    touch(&mut h, egui::TouchPhase::Start, pos, None);
    h.step();
    touch(&mut h, egui::TouchPhase::End, pos, None);
    h.run_steps(2);
    assert_eq!(h.state().globe.brush.flow, before);
    drag(&mut h, pos, pos - vec2(40.0, 0.0), None);
    assert!(h.state().globe.brush.flow < before);
}

#[test]
fn on_an_iphone_the_readout_is_above_the_slider_row() {
    let h = phone_pad();
    let rects = h.state().pad.rects.unwrap();
    let readout = rects.readout.unwrap();
    let card = rects.brush.unwrap();
    assert!(readout.bottom() <= card.top());
    assert!(card.bottom() <= rects.tools.top());
}

#[test]
fn on_an_iphone_the_readout_has_the_view_button_and_the_popover_fits() {
    let mut h = phone_pad();
    let readout = h.state().pad.rects.unwrap().readout.unwrap();
    let view = h.state().pad.view_rect.unwrap();
    assert_eq!(view.left_bottom(), readout.left_bottom());
    assert!(view.width() <= readout.width());
    assert!(view.contains_rect(control(&h, VIEW).unwrap()));
    // The menu does not hold the views.
    tap(&mut h, "Workspace");
    assert!(control(&h, VIEWS[0]).is_none());
    h.state_mut().pad.menu = false;
    h.run_steps(2);
    tap_in_view(&mut h, "Flat");
    assert_eq!(h.state().globe.world_view, WorldView::Flat);
    // The popover fits in the width of the phone.
    let menu = h.state().pad.view_menu_rect.unwrap();
    assert_eq!(menu.left(), view.left());
    assert!(menu.right() <= 393.0 - 8.0);
    for name in [PROJECTION, "Flat", GRATICULE] {
        assert!(menu.contains_rect(control(&h, name).unwrap()), "{name}");
    }
    check_safe(&h, (393.0, 852.0), [59.0, 0.0, 34.0, 0.0], "phone flat");
}

#[test]
fn on_an_iphone_the_mode_list_opens_above_the_strip() {
    let mut h = phone_pad();
    // A tap chooses the brush, and a tap on the chosen brush opens the list.
    quick_tap(&mut h, SCULPT);
    if h.state().pad.flyouts.open.is_none() {
        quick_tap(&mut h, SCULPT);
    }
    let list = h.state().pad.flyouts.rect.unwrap();
    let strip = h.state().pad.rects.unwrap().tools;
    assert!(list.bottom() <= strip.top());
    assert!(list.left() >= 0.0 && list.right() <= 393.0);
    assert!(list.top() >= 59.0);
}

/// Drags the bar of the open sheet from its top by `by` points.
fn pull_sheet(h: &mut Pad, panel_kind: Panel, by: f32) {
    let top = panel(h, panel_kind).unwrap().top();
    let bar = pos2(150.0, top + 26.0);
    drag(h, bar, bar + vec2(0.0, by), None);
}

#[test]
fn on_an_iphone_you_pull_the_layers_sheet_to_the_full_height_and_back() {
    let mut h = phone_pad();
    open_panel(&mut h, "Layers");
    // The sheet opens at half height, and it covers the rows.
    let half = panel(&h, Panel::Layers).unwrap();
    assert_eq!(half.top(), 400.0);
    assert!(control(&h, SCULPT).is_none());
    assert!(h.state().pad.rects.unwrap().readout.is_none());
    pull_sheet(&mut h, Panel::Layers, -200.0);
    let full = panel(&h, Panel::Layers).unwrap();
    assert_eq!(full.top(), 127.0);
    pull_sheet(&mut h, Panel::Layers, 200.0);
    assert_eq!(panel(&h, Panel::Layers).unwrap().top(), 400.0);
    // A layer style opens in the sheet at the same height.
    tap(&mut h, "Open Rivers");
    assert!(control(&h, "Back to Layers").is_some());
    assert_eq!(panel(&h, Panel::Layers).unwrap().top(), 400.0);
    // Down to the title row, the sheet has the rows above it.
    pull_sheet(&mut h, Panel::Layers, 300.0);
    let sheet = panel(&h, Panel::Layers).unwrap();
    assert_eq!(sheet.top(), PHONE_BOTTOM - 52.0);
    assert!(control(&h, "Pan").unwrap().bottom() <= sheet.top());
}

#[test]
fn on_an_iphone_one_sheet_shows_at_a_time() {
    let mut h = phone_pad();
    quick_tap(&mut h, SCULPT);
    tap(&mut h, "Brush settings");
    assert!(h.state().pad.is_open(Panel::Brush));
    open_panel(&mut h, "Layers");
    // The Brush panel stays open behind the sheet of the Layers panel.
    assert!(h.state().pad.is_open(Panel::Layers));
    assert!(h.state().pad.is_open(Panel::Brush));
    assert_eq!(h.state().pad.panels.len(), 1);
    assert_eq!(h.state().pad.panels[0].0, Panel::Layers);
}

#[test]
fn on_an_iphone_the_brush_sheet_has_no_pressure_settings() {
    let mut h = phone_pad();
    quick_tap(&mut h, SCULPT);
    tap(&mut h, "Brush settings");
    pull_sheet(&mut h, Panel::Brush, -300.0);
    assert_eq!(panel(&h, Panel::Brush).unwrap().top(), 127.0);
    let controls = &h.state().pad.controls;
    assert!(controls.iter().any(|c| c.name == "Brush hardness"));
    for c in controls {
        assert!(!c.name.to_lowercase().contains("pressure"), "{}", c.name);
    }
}

#[test]
#[ignore = "gpu"]
fn the_sheet_stays_in_its_place_during_a_drag() {
    let _gpu = one_gpu_test();
    let ((w, hgt), [top, right, bottom, left]) = IPHONES[0];
    let insets = egui::epaint::MarginF32 {
        left,
        right,
        top,
        bottom,
    };
    // A drag from the grabber, from the title row, and from the list.
    for start in [8.0, 26.0, 120.0] {
        let mut state = state();
        state.globe.set_face_size(64);
        state.add_height();
        state.pad.open(Panel::Layers);
        let setup = egui_kittest::wgpu::default_wgpu_setup();
        let mut h = headless::ui_harness(state, (w, hgt), 1.0, setup);
        h.input_mut().safe_area_insets = Some(egui::SafeAreaInsets(insets));
        h.run_steps(4);
        let sheet = panel(&h, Panel::Layers).unwrap();
        let from = pos2(150.0, sheet.top() + start);
        let steps = (0..=20).map(|i| -10.0 * i as f32);
        let path: Vec<f32> = steps
            .chain((0..=40).map(|i| -200.0 + 10.0 * i as f32))
            .collect();
        touch(&mut h, egui::TouchPhase::Start, from, None);
        for dy in path {
            // One frame for each move, as on a device. `touch` queues two
            // events, which the harness runs as two frames.
            let pos = from + vec2(0.0, dy);
            let events = &mut h.input_mut().events;
            events.push(egui::Event::Touch {
                device_id: egui::TouchDeviceId(0),
                id: egui::TouchId(1),
                phase: egui::TouchPhase::Move,
                pos,
                force: None,
            });
            events.push(egui::Event::PointerMoved(pos));
            h.step();
            let sheet = h.state().pad.rects.unwrap().sheet.unwrap();
            let close = control(&h, "Close Layers").unwrap();
            let what = format!("from {start}, at {dy}");
            assert_eq!(close.top(), sheet.top() + 4.0, "{what}");
            let card = h.ctx.memory(|m| m.area_rect(egui::Id::new("pad-sheet")));
            if let Some(card) = card {
                assert_eq!(card.bottom(), 852.0, "{what}");
            }
        }
        touch(&mut h, egui::TouchPhase::End, from, None);
    }
}

/// The iPhone layout in portrait, on a device with no pen.
fn phone(gpu: bool) -> Pad {
    let ((width, height), safe) = IPHONES[0];
    let size = (width as f32, height as f32);
    let mut h = if gpu {
        gpu_pad(size)
    } else {
        safe_pad(size, safe)
    };
    h.state_mut().globe.nav.no_pen = true;
    h
}

/// A touch of finger `id`, with no force, in the next frame. The first
/// finger is also the pointer. The events of one frame stay in that frame.
fn finger(h: &mut Pad, id: u64, phase: egui::TouchPhase, pos: Pos2) {
    let events = &mut h.input_mut().events;
    events.push(egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(id),
        phase,
        pos,
        force: None,
    });
    if id != 1 {
        return;
    }
    events.push(egui::Event::PointerMoved(pos));
    if phase != egui::TouchPhase::Move {
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: phase == egui::TouchPhase::Start,
            modifiers: egui::Modifiers::NONE,
        });
    }
    if phase == egui::TouchPhase::End {
        events.push(egui::Event::PointerGone);
    }
}

/// Two fingers come down left and right of the middle of the canvas, with
/// `gap` frames between them, move apart to twice the distance, and lift.
fn pinch(h: &mut Pad, gap: usize) {
    use egui::TouchPhase::{End, Move, Start};
    let c = h.state().globe.rect.center();
    let at = |id: u64, apart: f32| c + vec2(if id == 1 { -apart } else { apart }, 0.0);
    finger(h, 1, Start, at(1, 40.0));
    for _ in 0..gap {
        h.step();
    }
    finger(h, 2, Start, at(2, 40.0));
    h.step();
    for i in 1..=6 {
        for id in [1, 2] {
            finger(h, id, Move, at(id, 40.0 + 40.0 * i as f32 / 6.0));
        }
        h.step();
    }
    for id in [1, 2] {
        finger(h, id, End, at(id, 80.0));
    }
    h.run_steps(3);
}

/// A phone with the line tool on a new line layer, or the point tool on a
/// new point layer.
fn phone_with(gpu: bool, kind: GeometryKind) -> (Pad, vale_store::LayerId) {
    let mut h = phone(gpu);
    let id = h.state_mut().doc.add_layer(kind);
    h.state_mut().select(Some(LayerRef::Vector(id)));
    h.state_mut().globe.tool = match kind {
        GeometryKind::Point => Tool::Point,
        _ => Tool::Line,
    };
    h.run_steps(2);
    (h, id)
}

#[test]
fn one_finger_draws_a_line_and_places_a_point_on_a_phone() {
    let (mut h, lines) = phone_with(false, GeometryKind::Line);
    assert!(h.state().globe.lining());
    let view = h.state().globe.view;
    let c = h.state().globe.rect.center();
    drag(&mut h, c - vec2(60.0, 0.0), c + vec2(60.0, 40.0), None);
    assert_eq!(line_features(&h, lines).len(), 1);
    assert_eq!(h.state().globe.view, view);

    let (mut h, points) = phone_with(false, GeometryKind::Point);
    assert!(h.state().globe.placing());
    let c = h.state().globe.rect.center();
    touch(&mut h, egui::TouchPhase::Start, c, None);
    h.step();
    touch(&mut h, egui::TouchPhase::End, c, None);
    h.run_steps(3);
    assert_eq!(line_features(&h, points).len(), 1);
    assert_eq!(h.state().globe.view, view);
}

#[test]
fn two_fingers_zoom_the_globe_on_a_phone_and_do_not_draw() {
    // The second finger comes in the same frame, and while the first can
    // still be a gesture.
    for gap in [0, 1] {
        let (mut h, lines) = phone_with(false, GeometryKind::Line);
        let zoom = h.state().globe.view.zoom;
        pinch(&mut h, gap);
        let after = h.state().globe.view.zoom;
        assert!((after / zoom - 2.0).abs() < 0.05, "gap {gap}: {after}");
        assert!(line_features(&h, lines).is_empty(), "gap {gap}");
        assert!(!h.state().globe.nav.active(), "gap {gap}");
    }
}

#[test]
fn a_second_finger_ends_the_stroke_of_the_first_and_zooms() {
    use egui::TouchPhase::{End, Move, Start};
    let (mut h, lines) = phone_with(false, GeometryKind::Line);
    let zoom = h.state().globe.view.zoom;
    let c = h.state().globe.rect.center();
    finger(&mut h, 1, Start, c - vec2(80.0, 0.0));
    h.step();
    finger(&mut h, 1, Move, c - vec2(40.0, 0.0));
    h.step();
    finger(&mut h, 2, Start, c + vec2(40.0, 0.0));
    h.step();
    finger(&mut h, 1, Move, c - vec2(80.0, 0.0));
    finger(&mut h, 2, Move, c + vec2(80.0, 0.0));
    h.step();
    for id in [1, 2] {
        finger(&mut h, id, End, c);
    }
    h.run_steps(3);
    let features = line_features(&h, lines);
    assert_eq!(features.len(), 1);
    let after = h.state().globe.view.zoom;
    assert!((after / zoom - 2.0).abs() < 0.05, "{after}");
}

#[test]
fn a_tap_of_two_or_three_fingers_is_undo_or_redo_on_a_phone() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, lines) = phone_with(false, GeometryKind::Line);
    let state = h.state_mut();
    state.session = Some(Session::open(&dir.path().join("w.gpkg"), &mut state.doc).unwrap());
    state.save_waiting().unwrap();
    h.run_steps(2);
    let c = h.state().globe.rect.center();
    drag(&mut h, c - vec2(60.0, 0.0), c + vec2(60.0, 40.0), None);
    for _ in 0..3 {
        step_and_save(&mut h, false);
    }
    let view = h.state().globe.view;
    assert_eq!(line_features(&h, lines).len(), 1);
    tap_fingers(&mut h, 2);
    assert!(line_features(&h, lines).is_empty());
    tap_fingers(&mut h, 3);
    assert_eq!(line_features(&h, lines).len(), 1);
    assert_eq!(h.state().globe.view, view);
}

#[test]
fn one_finger_turns_the_globe_with_move_on_a_phone_and_with_the_line_tool_on_an_ipad() {
    let mut h = phone(false);
    h.state_mut().globe.tool = Tool::Navigate;
    h.run_steps(2);
    let view = h.state().globe.view;
    let c = h.state().globe.rect.center();
    drag(&mut h, c - vec2(60.0, 0.0), c + vec2(60.0, 40.0), None);
    assert_ne!(h.state().globe.view, view);

    let (mut h, lines) = phone_with(false, GeometryKind::Line);
    h.state_mut().globe.nav.no_pen = false;
    let view = h.state().globe.view;
    drag(&mut h, c - vec2(60.0, 0.0), c + vec2(60.0, 40.0), None);
    assert!(line_features(&h, lines).is_empty());
    assert_ne!(h.state().globe.view, view);
    // The pen draws.
    drag(&mut h, c - vec2(60.0, 0.0), c + vec2(60.0, 40.0), Some(0.5));
    assert_eq!(line_features(&h, lines).len(), 1);
}

#[test]
#[ignore = "gpu"]
fn one_finger_paints_with_no_pressure_on_a_phone() {
    let _gpu = one_gpu_test();
    let mut h = phone(true);
    h.state_mut().globe.tool = Tool::Brush;
    h.run_steps(2);
    let view = h.state().globe.view;
    let before = middle_level(&h);
    let c = h.state().globe.rect.center();
    drag(&mut h, c - vec2(40.0, 0.0), c + vec2(40.0, 0.0), None);
    settle(&mut h);
    let level = middle_level(&h);
    assert!(level > before, "{level} after {before}");
    assert_eq!(h.state().globe.view, view);
    // A finger with a force paints as one with no force.
    tap_fingers(&mut h, 2);
    settle(&mut h);
    assert_eq!(middle_level(&h), before);
    drag(&mut h, c - vec2(40.0, 0.0), c + vec2(40.0, 0.0), Some(0.05));
    settle(&mut h);
    assert_eq!(middle_level(&h), level);
    assert_eq!(h.state().globe.pen.stats.force, None);
    assert_eq!(h.state().globe.view, view);
}

#[test]
#[ignore = "gpu"]
fn two_fingers_zoom_the_globe_with_the_brush_on_a_phone_and_do_not_paint() {
    let _gpu = one_gpu_test();
    let mut h = phone(true);
    h.state_mut().globe.tool = Tool::Brush;
    h.run_steps(2);
    let before = middle_level(&h);
    assert!(!h.state().globe.can_undo());
    for gap in [0, 1] {
        let zoom = h.state().globe.view.zoom;
        pinch(&mut h, gap);
        settle(&mut h);
        assert!(!h.state().globe.can_undo(), "gap {gap}");
        let after = h.state().globe.view.zoom;
        assert!((after / zoom - 2.0).abs() < 0.05, "gap {gap}: {after}");
        assert_eq!(middle_level(&h), before, "gap {gap}");
    }
}

#[test]
#[ignore = "gpu"]
fn a_tap_of_two_or_three_fingers_is_undo_or_redo_of_a_finger_stroke() {
    let _gpu = one_gpu_test();
    let mut h = phone(true);
    h.state_mut().globe.tool = Tool::Brush;
    h.run_steps(2);
    let before = middle_level(&h);
    let c = h.state().globe.rect.center();
    drag(&mut h, c - vec2(40.0, 0.0), c + vec2(40.0, 0.0), None);
    settle(&mut h);
    let level = middle_level(&h);
    assert!(level > before);
    tap_fingers(&mut h, 2);
    settle(&mut h);
    assert_eq!(middle_level(&h), before);
    tap_fingers(&mut h, 3);
    settle(&mut h);
    assert_eq!(middle_level(&h), level);
}

#[test]
fn a_swipe_renames_a_layer_in_the_list() {
    use vale_app::ui::pad::widgets::RENAME;
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        open_panel(&mut h, "Layers");
        swipe(&mut h, "Rivers", -120.0);
        assert!(control(&h, "Delete Rivers").is_some(), "{what}");
        if size == BOARDS[0] {
            screenshot::save(&mut h, "pad-layer-swipe");
        }
        tap(&mut h, &format!("{RENAME} Rivers"));
        assert!(control(&h, LAYER_NAME).is_some(), "{what}");
        check_controls(&h, &what);
        if size == BOARDS[0] {
            screenshot::save(&mut h, "pad-layer-rename");
        }
        // The name changes when the field loses the focus.
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.run_steps(1);
        h.event(egui::Event::Text("Streams".to_owned()));
        h.run_steps(1);
        assert!(layer_names(&h).contains(&"Rivers".to_string()), "{what}");
        h.key_press(egui::Key::Enter);
        h.run_steps(2);
        assert!(control(&h, LAYER_NAME).is_none(), "{what}");
        assert!(layer_names(&h).contains(&"Streams".to_string()), "{what}");

        // An empty name keeps the old name.
        swipe(&mut h, "Streams", -120.0);
        tap(&mut h, &format!("{RENAME} Streams"));
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.run_steps(1);
        h.key_press(egui::Key::Backspace);
        h.run_steps(1);
        h.key_press(egui::Key::Enter);
        h.run_steps(2);
        assert!(layer_names(&h).contains(&"Streams".to_string()), "{what}");
    }
}

#[test]
fn the_selectable_cell_and_list_fit_in_the_world_on_each_screen() {
    use vale_app::ui::pad::selectable::{
        ACTIVE_LAYER, DESELECT_ALL, SELECT_ALL, SELECTABLE, layer_row_name,
    };
    for (size, safe) in measure_screens() {
        let what = format!("{size:?}");
        let mut h = safe_pad(size, safe);
        let rivers = vector(&h, "Rivers");
        h.state_mut().select(Some(LayerRef::Vector(rivers)));
        h.state_mut().globe.tool = Tool::Select;
        h.run_steps(2);
        scroll_strip_to(&mut h, SELECTABLE);
        assert!(control(&h, SELECTABLE).is_some(), "{what}");
        check_controls(&h, &what);
        check_safe(&h, size, safe, &what);
        tap(&mut h, SELECTABLE);
        assert!(h.state().pad.selectable, "{what}");
        let row = layer_row_name("Rivers");
        for name in [ACTIVE_LAYER, SELECT_ALL, DESELECT_ALL] {
            let rect = control(&h, name).unwrap_or_else(|| panic!("{what}: {name}"));
            assert!(rect.height() >= 44.0, "{what}: {name}");
        }
        check_safe(&h, size, safe, &what);
        if control(&h, &row).is_some() {
            tap(&mut h, &row);
            assert!(h.state().selectable_checked(rivers), "{what}");
            assert!(h.state().pad.selectable, "{what}");
        }
        tap(&mut h, ACTIVE_LAYER);
        assert_eq!(
            h.state().selectable,
            vale_app::ui::select::Selectable::Active,
            "{what}"
        );
    }
}

#[test]
fn a_long_press_shows_the_name_of_a_tool_and_a_longer_hold_opens_its_modes() {
    use vale_app::ui::pad::flyout::HOLD_HINT;
    let mut h = pad(BOARDS[0]);
    quick_tap(&mut h, "Pan");
    let tools = h.state().pad.rects.unwrap().tools;
    // The cells show icons only.
    assert_eq!(control(&h, MEASURE).unwrap().size(), vec2(44.0, 44.0));
    assert!(h.query_by_label_contains(HOLD_HINT).is_none());

    // A hold on a tool with no modes shows its name. The end of the hold
    // does not choose the tool.
    let pos = control(&h, MEASURE).unwrap().center();
    touch(&mut h, egui::TouchPhase::Start, pos, None);
    let mut steps = 0;
    while h.state().pad.flyouts.tip.is_none() && steps < 120 {
        h.step();
        steps += 1;
    }
    let tip = h.state().pad.flyouts.tip.clone().expect("a tooltip");
    assert!(steps > 1);
    assert_eq!((tip.name.as_str(), tip.modes), (MEASURE, false));
    let card = h.state().pad.flyouts.tip_rect.unwrap();
    assert!(card.left() > tools.right());
    assert_eq!(card.center().y, tip.cell.center().y);
    h.run_steps(60);
    assert!(h.state().pad.flyouts.tip.is_some());
    assert_eq!(h.state().pad.flyouts.open, None);
    touch(&mut h, egui::TouchPhase::End, pos, None);
    h.run_steps(3);
    assert_eq!(h.state().globe.tool, Tool::Navigate);
    assert_eq!(h.state().pad.flyouts.tip, None);

    // A hold on a brush cell shows the mode and the hint. A longer hold
    // opens the modes and hides the tooltip.
    let pos = control(&h, TERRAIN).unwrap().center();
    touch(&mut h, egui::TouchPhase::Start, pos, None);
    let mut steps = 0;
    while h.state().pad.flyouts.tip.is_none() && steps < 120 {
        h.step();
        steps += 1;
    }
    let tip = h.state().pad.flyouts.tip.clone().expect("a tooltip");
    let face = h.state().pad.brush_faces[1].unwrap().0;
    assert_eq!((tip.name.as_str(), tip.modes), (face, true));
    screenshot::save(&mut h, "pad-tooltip");
    assert_eq!(h.state().pad.flyouts.open, None);
    let mut more = 0;
    while h.state().pad.flyouts.open.is_none() && more < 120 {
        h.step();
        more += 1;
    }
    assert!(more > 1);
    let terrain = Flyout::Brush(ModeGroup::ALL[1]);
    assert_eq!(h.state().pad.flyouts.open, Some(terrain));
    assert_eq!(h.state().pad.flyouts.tip, None);
    touch(&mut h, egui::TouchPhase::End, pos, None);
    h.run_steps(3);
    assert_eq!(h.state().globe.tool, Tool::Navigate);
    assert_eq!(h.state().pad.flyouts.open, Some(terrain));
}

#[test]
fn the_panels_button_opens_a_menu_with_a_tile_for_each_panel() {
    use vale_app::ui::pad::panels_menu::{self, CLOSE_ALL, RESET_LAYOUT};
    let open_tiles = |h: &Pad| -> Vec<&'static str> {
        let groups = panels_menu::world(h.state());
        let tiles = groups.iter().flat_map(|g| g.tiles.iter());
        tiles.filter(|t| t.open).map(|t| t.name).collect()
    };
    for size in BOARDS {
        let mut h = pad(size);
        quick_tap(&mut h, "Pan");
        tap(&mut h, PANELS);
        assert!(h.state().pad.panels_menu, "{size:?}");
        // The menu is on the right edge, below the top row.
        let menu = h.state().pad.panels_menu_rect.unwrap();
        let rects = h.state().pad.rects.unwrap();
        assert_eq!(menu.right(), size.0, "{size:?}");
        assert!(menu.top() > rects.actions.bottom(), "{size:?}");
        for name in [
            "Layers",
            select::ATTRIBUTES,
            "Toolbox",
            "Brush",
            CLOSE_ALL,
            RESET_LAYOUT,
        ] {
            let tile = control(&h, name).unwrap_or_else(|| panic!("{size:?}: {name}"));
            assert!(menu.contains_rect(tile), "{size:?}: {name}");
        }
        // No tile is off, also the Brush tile with the Move tool.
        assert!(!disabled(&h, "Brush"), "{size:?}");
        check_controls(&h, &format!("{size:?} with the Panels menu"));

        // A tap on a tile opens its panel and closes the menu. A tap on
        // the tile of an open panel closes the panel.
        tap(&mut h, "Layers");
        assert!(h.state().pad.is_open(Panel::Layers), "{size:?}");
        assert!(!h.state().pad.panels_menu, "{size:?}");
        assert_eq!(open_tiles(&h), ["Layers"], "{size:?}");
        open_panel(&mut h, "Toolbox");
        assert!(h.state().pad.is_open(Panel::Toolbox), "{size:?}");
        open_panel(&mut h, "Toolbox");
        assert!(!h.state().pad.is_open(Panel::Toolbox), "{size:?}");
        assert!(h.state().pad.is_open(Panel::Layers), "{size:?}");
        open_panel(&mut h, "Layers");
        assert!(h.state().pad.dock().panes.is_empty(), "{size:?}");

        // With a brush, the Brush tile opens the Brush panel.
        quick_tap(&mut h, SCULPT);
        open_panel(&mut h, "Brush");
        assert!(h.state().pad.is_open(Panel::Brush), "{size:?}");
        open_panel(&mut h, select::ATTRIBUTES);
        assert!(h.state().pad.is_open(Panel::Attributes), "{size:?}");

        // Close all closes each panel.
        open_panel(&mut h, CLOSE_ALL);
        assert!(open_tiles(&h).is_empty(), "{size:?}");
        assert!(h.state().pad.panels.is_empty(), "{size:?}");

        // A tap outside the menu closes it.
        tap(&mut h, PANELS);
        let outside = pos2(size.0 / 2.0, menu.bottom() + 40.0);
        finger_tap(&mut h, outside);
        assert!(!h.state().pad.panels_menu, "{size:?}");
        // The Panels button closes the menu too.
        tap(&mut h, PANELS);
        tap(&mut h, PANELS);
        assert!(!h.state().pad.panels_menu, "{size:?}");
    }
}

/// The panes of the dock of the World workspace, by their tabs.
fn dock_tabs(h: &Pad) -> Vec<Vec<Panel>> {
    let panes = &h.state().pad.docks.world.panes;
    panes.iter().map(|p| p.tabs.clone()).collect()
}

/// A layout with these panes in the dock.
fn docked(size: (f32, f32), panes: &[&[Panel]]) -> Pad {
    let mut h = pad(size);
    for tabs in panes {
        h.state_mut().pad.docks.world.open_tabs(tabs);
    }
    h.run_steps(3);
    h
}

#[test]
fn each_pane_has_a_bar_below_it_that_changes_its_height() {
    use vale_app::ui::pad::panes::BAR;
    let mut h = docked(BOARDS[0], &[&[Panel::Layers], &[Panel::Toolbox]]);
    let (layers, toolbox) = (
        panel(&h, Panel::Layers).unwrap(),
        panel(&h, Panel::Toolbox).unwrap(),
    );
    assert_eq!(layers.top(), h.state().pad.rects.unwrap().column_top);
    assert_eq!(toolbox.top(), layers.bottom() + BAR);
    assert_eq!((toolbox.left(), toolbox.right()), (layers.left(), 1194.0));

    // The last pane has a bar too, at the bottom of the dock.
    let dock = h.state().pad.dock_rect.unwrap();
    assert_eq!(dock.bottom(), toolbox.bottom() + BAR + 10.0);

    // The pane above the bar gets shorter. The pane below it shows all its
    // content, so it keeps its height and moves up.
    let bar = pos2(layers.center().x, layers.bottom() + BAR / 2.0);
    drag(&mut h, bar, bar - vec2(0.0, 40.0), None);
    let (above, below) = (
        panel(&h, Panel::Layers).unwrap(),
        panel(&h, Panel::Toolbox).unwrap(),
    );
    assert!(
        (above.height() - (layers.height() - 40.0)).abs() < 1.0,
        "{above:?}"
    );
    assert!((below.height() - toolbox.height()).abs() < 1.0, "{below:?}");
    assert_eq!(below.top(), above.bottom() + BAR);
    let panes = &h.state().pad.docks.world.panes;
    assert!(panes[0].height.is_some() && panes[1].height.is_none());

    // The bar takes a touch 44 points high. The last bar is at the bottom
    // of the dock, and its touch area ends there.
    let mut one = docked(BOARDS[0], &[&[Panel::Layers]]);
    let before = panel(&one, Panel::Layers).unwrap();
    let bottom = one.state().pad.dock_rect.unwrap().bottom();
    assert_eq!(bottom, before.bottom() + BAR + 10.0);
    let at = pos2(before.center().x, bottom - 6.0);
    drag(&mut one, at, at - vec2(0.0, 30.0), None);
    // The first step of the drag goes to its start.
    let after = panel(&one, Panel::Layers).unwrap();
    let less = before.height() - after.height();
    assert!((20.0..=30.0).contains(&less), "{after:?}");

    // The pane grows back into the free room, and never past its content.
    let bar = pos2(above.center().x, above.bottom() + BAR / 2.0);
    drag(&mut h, bar, bar + vec2(0.0, 200.0), None);
    let grown = panel(&h, Panel::Layers).unwrap();
    assert!((grown.height() - layers.height()).abs() < 1.0, "{grown:?}");
}

#[test]
fn the_handle_changes_the_width_of_the_dock_and_a_wide_dock_names_each_tab() {
    use vale_app::ui::pad::dock_ui::{DOCK_WIDTH, tab_name};
    let mut h = docked(BOARDS[0], &[&[Panel::Layers, Panel::Toolbox]]);
    let width = |h: &Pad| h.state().pad.docks.world.width;
    assert_eq!(width(&h), 340.0);
    assert_eq!(control(&h, &tab_name(TOOLBOX)).unwrap().width(), 44.0);

    // During the drag, the dock shows its width.
    let handle = control(&h, DOCK_WIDTH).unwrap().center();
    touch(&mut h, egui::TouchPhase::Start, handle, None);
    h.step();
    for i in 1..=6 {
        let at = handle - vec2(50.0 * i as f32, 0.0);
        touch(&mut h, egui::TouchPhase::Move, at, None);
        h.step();
    }
    assert!(h.state().pad.dock_drag.is_some());
    touch(
        &mut h,
        egui::TouchPhase::End,
        handle - vec2(300.0, 0.0),
        None,
    );
    h.run_steps(3);
    assert_eq!(width(&h), 520.0);
    assert!(h.state().pad.dock_drag.is_none());
    assert_eq!(panel(&h, Panel::Layers).unwrap().width(), 520.0);
    assert!(control(&h, &tab_name(TOOLBOX)).unwrap().width() > 44.0);

    // A drag that stops less than 40 points past the narrowest dock keeps
    // the dock.
    let handle = control(&h, DOCK_WIDTH).unwrap().center();
    drag(&mut h, handle, handle + vec2(240.0, 0.0), None);
    assert_eq!(width(&h), 300.0);
    assert!(!h.state().pad.docks.world.collapsed);
    for (dock, named) in [(419.0, false), (420.0, true)] {
        h.state_mut().pad.docks.world.width = dock;
        h.run_steps(2);
        let tab = control(&h, &tab_name(TOOLBOX)).unwrap();
        assert_eq!(tab.width() > 44.0, named, "{dock}");
    }
}

/// A tap of a finger on the canvas, at `pos`.
fn tap_at(h: &mut Pad, pos: Pos2) {
    touch(h, egui::TouchPhase::Start, pos, None);
    h.step();
    touch(h, egui::TouchPhase::End, pos, None);
    h.run_steps(3);
}

#[test]
fn a_tap_on_the_canvas_closes_no_pane() {
    let mut h = pad(BOARDS[0]);
    open_panel(&mut h, "Layers");
    toolbox(&mut h);
    let c = h.state().globe.rect.center();
    drag(&mut h, c - vec2(40.0, 0.0), c + vec2(40.0, 0.0), Some(0.5));
    pen_tap_at(&mut h, c);
    tap_at(&mut h, c);
    tap_at(&mut h, c + vec2(30.0, 30.0));
    assert_eq!(dock_tabs(&h), [[Panel::Layers], [Panel::Toolbox]]);
}

/// Closes a panel with the menu of its tab, or of the title of its pane.
fn close_panel(h: &mut Pad, name: &str) {
    use vale_app::ui::pad::dock_ui::{close_name, move_name, tab_name};
    let tab = tab_name(name);
    let target = if control(h, &tab).is_some() {
        tab
    } else {
        move_name(name)
    };
    long_press(h, &target);
    tap(h, &close_name(name));
}

#[test]
fn a_long_press_on_a_tab_opens_its_menu() {
    use vale_app::ui::pad::dock_ui::{MOVE_TO_NEW_PANE, close_name, move_name, tab_name};
    let mut h = docked(
        BOARDS[0],
        &[&[Panel::Layers, Panel::Toolbox], &[Panel::Attributes]],
    );
    // A header has no Pin button and no Close button.
    assert!(control(&h, "Pin Layers").is_none());
    assert!(control(&h, &close_name("Layers")).is_none());
    long_press(&mut h, &tab_name(TOOLBOX));
    let menu = h.state().pad.pane_menu.expect("no menu");
    assert_eq!(menu.panel, Panel::Toolbox);
    // The release does not choose the tab.
    assert!(h.state().pad.shows(Panel::Layers));
    check_controls(&h, "the menu of a tab");
    screenshot::save(&mut h, "pad-tab-menu");
    tap(&mut h, MOVE_TO_NEW_PANE);
    assert_eq!(
        dock_tabs(&h),
        [
            vec![Panel::Layers],
            vec![Panel::Toolbox],
            vec![Panel::Attributes]
        ]
    );
    assert!(h.state().pad.pane_menu.is_none());

    // A pane with one tab has the menu on its title, with no Move row.
    long_press(&mut h, &move_name(select::ATTRIBUTES));
    assert!(control(&h, MOVE_TO_NEW_PANE).is_none());
    tap(&mut h, &close_name(select::ATTRIBUTES));
    assert_eq!(dock_tabs(&h), [[Panel::Layers], [Panel::Toolbox]]);

    // A tap outside closes the menu.
    long_press(&mut h, &move_name("Layers"));
    assert!(h.state().pad.pane_menu.is_some());
    let c = h.state().globe.rect.center();
    tap_at(&mut h, c);
    assert!(h.state().pad.pane_menu.is_none());
    assert_eq!(dock_tabs(&h), [[Panel::Layers], [Panel::Toolbox]]);
}

#[test]
fn a_drag_of_a_tab_opens_no_menu() {
    use vale_app::ui::pad::dock_ui::tab_name;
    let mut h = docked(BOARDS[0], &[&[Panel::Layers, Panel::Toolbox]]);
    let from = control(&h, &tab_name(TOOLBOX)).unwrap().center();
    let dock = h.state().pad.dock_rect.unwrap();
    drag(&mut h, from, pos2(from.x, dock.bottom() + 10.0), None);
    assert_eq!(dock_tabs(&h), [[Panel::Layers], [Panel::Toolbox]]);
    assert!(h.state().pad.pane_menu.is_none());
}

fn folded(h: &Pad) -> Vec<bool> {
    let panes = &h.state().pad.docks.world.panes;
    panes.iter().map(|p| p.folded).collect()
}

#[test]
fn a_tap_on_the_open_tab_folds_the_pane_to_its_header() {
    use vale_app::ui::pad::dock_ui::{fold_name, tab_name};
    use vale_app::ui::pad::geometry::ROW;
    use vale_app::ui::pad::panes::BAR;
    let mut h = docked(
        BOARDS[0],
        &[&[Panel::Layers, Panel::Toolbox], &[Panel::Brush]],
    );
    let brush = panel(&h, Panel::Brush).unwrap();
    quick_tap(&mut h, &tab_name("Layers"));
    assert_eq!(folded(&h), [true, false]);
    // The folded pane is its header, and the pane below moves up.
    let layers = panel(&h, Panel::Layers).unwrap();
    assert_eq!(layers.height(), ROW);
    let below = panel(&h, Panel::Brush).unwrap();
    assert_eq!(below.top(), layers.bottom() + BAR);
    assert!(below.top() < brush.top());
    // The chevron points right, and a tap on it opens the pane.
    assert!(control(&h, &fold_name("Layers", false)).is_none());
    check_controls(&h, "a folded pane");
    screenshot::save(&mut h, "pad-folded-pane");
    quick_tap(&mut h, &fold_name("Layers", true));
    assert_eq!(folded(&h), [false, false]);
    // A tap on another tab shows it and opens the pane.
    quick_tap(&mut h, &tab_name("Layers"));
    quick_tap(&mut h, &tab_name(TOOLBOX));
    assert_eq!(folded(&h), [false, false]);
    assert!(h.state().pad.shows(Panel::Toolbox));
    // A tap on the title of a pane with one tab folds it, and the chevron
    // folds it too.
    use vale_app::ui::pad::dock_ui::move_name;
    quick_tap(&mut h, &move_name("Brush"));
    assert_eq!(folded(&h), [false, true]);
    quick_tap(&mut h, &move_name("Brush"));
    assert_eq!(folded(&h), [false, false]);
    quick_tap(&mut h, &fold_name("Brush", false));
    assert_eq!(folded(&h), [false, true]);
    // A folded pane has no resize bar.
    let brush = panel(&h, Panel::Brush).unwrap();
    let bar = pos2(brush.center().x, brush.bottom() + BAR / 2.0);
    drag(&mut h, bar, bar + vec2(0.0, -60.0), None);
    assert_eq!(panel(&h, Panel::Brush).unwrap().height(), ROW);
    assert_eq!(h.state().pad.docks.world.panes[1].height, None);
}

#[test]
fn the_menu_of_a_tab_folds_and_opens_the_pane() {
    use vale_app::ui::pad::dock_ui::{COLLAPSE_PANE, EXPAND_PANE, move_name, tab_name};
    let mut h = docked(
        BOARDS[0],
        &[&[Panel::Layers, Panel::Toolbox], &[Panel::Brush]],
    );
    long_press(&mut h, &tab_name(TOOLBOX));
    assert!(control(&h, EXPAND_PANE).is_none());
    check_controls(&h, "the menu of a tab");
    tap(&mut h, COLLAPSE_PANE);
    assert_eq!(folded(&h), [true, false]);
    // The release did not choose the tab.
    assert!(h.state().pad.shows(Panel::Layers));
    long_press(&mut h, &move_name("Brush"));
    tap(&mut h, COLLAPSE_PANE);
    assert_eq!(folded(&h), [true, true]);
    long_press(&mut h, &tab_name("Layers"));
    tap(&mut h, EXPAND_PANE);
    assert_eq!(folded(&h), [false, true]);
}

#[test]
fn a_tile_opens_a_folded_pane() {
    use vale_app::ui::pad::dock_ui::tab_name;
    let mut h = docked(BOARDS[0], &[&[Panel::Layers, Panel::Toolbox]]);
    quick_tap(&mut h, &tab_name("Layers"));
    assert_eq!(folded(&h), [true]);
    // The tile of the panel that shows opens the pane, and does not close
    // the panel.
    open_panel(&mut h, "Layers");
    assert_eq!(folded(&h), [false]);
    assert!(h.state().pad.shows(Panel::Layers));
    // The tile of a tab behind shows it and opens the pane.
    quick_tap(&mut h, &tab_name("Layers"));
    toolbox(&mut h);
    assert_eq!(folded(&h), [false]);
    assert!(h.state().pad.shows(Panel::Toolbox));
}

/// A drag of a finger on the header control `name` by `by`.
fn drag_header(h: &mut Pad, name: &str, by: egui::Vec2) {
    let from = control(h, name)
        .unwrap_or_else(|| panic!("no control `{name}`"))
        .center();
    drag(h, from, from + by, None);
}

#[test]
fn a_swipe_to_the_right_on_a_header_hides_the_dock() {
    use vale_app::ui::pad::dock_ui::{move_name, tab_name};
    for name in [tab_name("Layers"), tab_name(TOOLBOX), move_name("Brush")] {
        let mut h = docked(
            BOARDS[0],
            &[&[Panel::Layers, Panel::Toolbox], &[Panel::Brush]],
        );
        let before = dock_tabs(&h);
        drag_header(&mut h, &name, vec2(200.0, 10.0));
        let dock = &h.state().pad.docks.world;
        assert!(dock.collapsed, "{name}");
        // The panes stay as they were.
        assert_eq!(dock_tabs(&h), before, "{name}");
        assert!(h.state().pad.panels.is_empty(), "{name}");
        assert!(h.state().pad.dock_drag.is_none(), "{name}");
    }
}

#[test]
fn a_short_swipe_on_a_header_keeps_the_dock() {
    use vale_app::ui::pad::dock_ui::tab_name;
    let mut h = docked(
        BOARDS[0],
        &[&[Panel::Layers, Panel::Toolbox], &[Panel::Brush]],
    );
    let rect = panel(&h, Panel::Layers).unwrap();
    // During the swipe, the dock moves with the finger.
    let from = control(&h, &tab_name(TOOLBOX)).unwrap().center();
    touch(&mut h, egui::TouchPhase::Start, from, None);
    h.step();
    for i in 1..=4 {
        let at = from + vec2(15.0 * i as f32, 0.0);
        touch(&mut h, egui::TouchPhase::Move, at, None);
        h.step();
    }
    h.step();
    let moved = panel(&h, Panel::Layers).unwrap();
    assert!(moved.left() > rect.left() + 30.0, "{moved:?}");
    touch(&mut h, egui::TouchPhase::End, from + vec2(60.0, 0.0), None);
    h.run_steps(3);
    assert!(!h.state().pad.docks.world.collapsed);
    assert_eq!(panel(&h, Panel::Layers), Some(rect));
    assert_eq!(
        dock_tabs(&h),
        [vec![Panel::Layers, Panel::Toolbox], vec![Panel::Brush]]
    );
    assert!(h.state().pad.pane_menu.is_none());
}

#[test]
fn a_vertical_drag_on_a_header_still_moves_the_pane() {
    use vale_app::ui::pad::dock_ui::{move_name, tab_name};
    let mut h = docked(
        BOARDS[0],
        &[&[Panel::Layers, Panel::Toolbox], &[Panel::Brush]],
    );
    let dock = h.state().pad.dock_rect.unwrap();
    // The empty part of the header moves the pane below the other pane.
    let name = move_name("Layers");
    let from = control(&h, &name).unwrap().center();
    drag(&mut h, from, pos2(from.x + 20.0, dock.bottom() - 4.0), None);
    assert!(!h.state().pad.docks.world.collapsed);
    assert_eq!(
        dock_tabs(&h),
        [vec![Panel::Brush], vec![Panel::Layers, Panel::Toolbox]]
    );
    // A tab moves to a pane of its own.
    let from = control(&h, &tab_name(TOOLBOX)).unwrap().center();
    let dock = h.state().pad.dock_rect.unwrap();
    drag(&mut h, from, pos2(from.x + 20.0, dock.bottom() - 4.0), None);
    assert!(!h.state().pad.docks.world.collapsed);
    assert_eq!(
        dock_tabs(&h),
        [
            vec![Panel::Brush],
            vec![Panel::Layers],
            vec![Panel::Toolbox]
        ]
    );
}

#[test]
fn the_brush_pane_stays_with_a_tool_that_has_no_settings() {
    use vale_app::ui::pad::NO_SETTINGS;
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Brush settings");
    assert!(h.state().pad.is_open(Panel::Brush));
    h.state_mut().globe.tool = Tool::Navigate;
    h.run_steps(3);
    assert!(h.state().pad.is_open(Panel::Brush));
    let hint = h.query_by_label(NO_SETTINGS);
    assert!(hint.is_some());
}

#[test]
fn a_drag_of_the_handle_to_the_edge_collapses_the_dock_and_the_edge_tab_shows_it() {
    use vale_app::ui::pad::dock_ui::{DOCK_WIDTH, SHOW_DOCK};
    let mut h = docked(BOARDS[0], &[&[Panel::Layers]]);
    let handle = control(&h, DOCK_WIDTH).unwrap().center();
    drag(&mut h, handle, pos2(1194.0, handle.y), None);
    let dock = &h.state().pad.docks.world;
    assert!(dock.collapsed);
    assert_eq!(dock.panes.len(), 1);
    assert!(h.state().pad.panels.is_empty());
    assert!(control(&h, DOCK_WIDTH).is_none());
    check_controls(&h, "a collapsed dock");
    // A tap on the tab at the edge shows the dock.
    let tab = control(&h, SHOW_DOCK).unwrap();
    assert_eq!(tab.right(), 1194.0);
    assert!(tab.height() >= 44.0 && tab.width() >= 44.0);
    tap(&mut h, SHOW_DOCK);
    assert!(!h.state().pad.docks.world.collapsed);
    assert!(panel(&h, Panel::Layers).is_some());
    // A drag of the tab to the left shows it too.
    h.state_mut().pad.docks.world.collapsed = true;
    h.run_steps(3);
    let tab = control(&h, SHOW_DOCK).unwrap().center();
    drag(&mut h, tab, tab - vec2(120.0, 0.0), None);
    assert!(!h.state().pad.docks.world.collapsed);
}

#[test]
fn the_dock_button_hides_and_shows_the_dock_and_is_off_with_no_panes() {
    use vale_app::ui::pad::DOCK;
    use vale_app::ui::pad::dock_ui::{HIDE_DOCK, SHOW_DOCK};
    for size in [BOARDS[0], BOARDS[1]] {
        let mut h = pad(size);
        let what = format!("{size:?}");
        // With no panes, the button is off.
        assert!(control(&h, DOCK).is_none(), "{what}");
        assert!(disabled(&h, DOCK), "{what}");
        // The Dock button is at the right end, after the Panels button.
        // Undo, Redo, and a line are at their left.
        let actions = h.state().pad.rects.unwrap().actions;
        open_panel(&mut h, "Layers");
        let button = control(&h, DOCK).expect("a Dock button");
        let panels = control(&h, PANELS).unwrap();
        assert_eq!(
            panels.left(),
            actions.left() + 4.0 + 2.0 * 48.0 + 9.0 + 4.0,
            "{what}"
        );
        assert_eq!(button.left(), panels.left() + 48.0, "{what}");
        assert_eq!(panels.right() + 4.0, button.left(), "{what}");
        assert_eq!(button.size(), vec2(44.0, 44.0), "{what}");
        assert_eq!(toggled(&h, DOCK), Some(true), "{what}");
        // The Panels menu has no row of the dock.
        tap(&mut h, PANELS);
        assert!(h.state().pad.panels_menu, "{what}");
        for name in [HIDE_DOCK, SHOW_DOCK] {
            assert!(control(&h, name).is_none(), "{what}: {name}");
            assert!(h.query_by_label(name).is_none(), "{what}: {name}");
        }
        tap(&mut h, PANELS);
        // A tap hides the dock, and a second tap shows it.
        tap(&mut h, DOCK);
        assert!(h.state().pad.docks.world.collapsed, "{what}");
        assert_eq!(toggled(&h, DOCK), Some(false), "{what}");
        assert!(h.state().pad.panels.is_empty(), "{what}");
        tap(&mut h, DOCK);
        assert!(!h.state().pad.docks.world.collapsed, "{what}");
        assert_eq!(toggled(&h, DOCK), Some(true), "{what}");
        assert!(panel(&h, Panel::Layers).is_some(), "{what}");
        // A panel that opens shows the dock.
        tap(&mut h, DOCK);
        toolbox(&mut h);
        assert!(!h.state().pad.docks.world.collapsed, "{what}");
        assert_eq!(dock_tabs(&h), [[Panel::Layers], [Panel::Toolbox]]);
        // The tile of an open panel of a collapsed dock shows it, and does
        // not close it.
        tap(&mut h, DOCK);
        open_panel(&mut h, "Layers");
        assert!(!h.state().pad.docks.world.collapsed, "{what}");
        assert!(h.state().pad.is_open(Panel::Layers), "{what}");
    }
    // A phone has no Dock button.
    let h = phone_pad();
    assert!(h.query_by_label(DOCK).is_none());
}

#[test]
fn the_globe_stays_in_place_when_the_dock_shows_and_hides() {
    use vale_app::ui::pad::DOCK;
    for size in BOARDS {
        for view in WORLD_VIEWS {
            let mut h = pad(size);
            let what = format!("{size:?} {view:?}");
            h.state_mut().globe.world_view = view;
            h.run_steps(2);
            let screen = Rect::from_min_size(Pos2::ZERO, size.into());
            let rect = h.state().globe.rect;
            assert_eq!(rect, screen, "{what}");
            let middle = |h: &Pad| {
                let globe = &h.state().globe;
                globe.unproject(globe.rect.center())
            };
            let center = middle(&h);
            open_panel(&mut h, "Layers");
            assert_eq!(h.state().globe.rect, rect, "{what}");
            assert_eq!(middle(&h), center, "{what}");
            if WidthClass::of(size.0) == WidthClass::Compact {
                continue;
            }
            tap(&mut h, DOCK);
            assert!(h.state().pad.docks.world.collapsed, "{what}");
            assert_eq!(h.state().globe.rect, rect, "{what}");
            assert_eq!(middle(&h), center, "{what}");
            tap(&mut h, DOCK);
            assert_eq!(h.state().globe.rect, rect, "{what}");
            assert_eq!(middle(&h), center, "{what}");
        }
    }
}

#[test]
fn each_workspace_keeps_its_dock_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = docked(BOARDS[0], &[&[Panel::Layers, Panel::Toolbox]]);
    h.state_mut().projects = Locations::single(dir.path());
    open_panel(&mut h, select::ATTRIBUTES);
    let docks = &mut h.state_mut().pad.docks;
    docks.world.width = 460.0;
    docks.world.collapsed = true;
    docks.atlas.open_new(Panel::Attributes);
    docks.atlas.width = 380.0;
    h.state_mut().save_resume();

    let mut next = state();
    next.projects = Locations::single(dir.path());
    next.restore();
    let world = &next.pad.docks.world;
    assert_eq!(world.panes.len(), 2);
    assert_eq!(world.panes[0].tabs, [Panel::Layers, Panel::Toolbox]);
    assert_eq!(world.panes[1].tabs, [Panel::Attributes]);
    assert_eq!(world.width, 460.0);
    assert!(world.collapsed);
    let atlas = &next.pad.docks.atlas;
    assert_eq!(atlas.panes[0].tabs, [Panel::Attributes]);
    assert_eq!(atlas.width, 380.0);
    assert!(!atlas.collapsed);
}

#[test]
fn on_a_phone_a_panel_is_a_sheet_with_its_tabs() {
    use vale_app::ui::pad::dock_ui::{DOCK_WIDTH, close_name, tab_name};
    let mut h = phone_pad();
    h.state_mut()
        .pad
        .docks
        .world
        .open_tabs(&[Panel::Layers, Panel::Toolbox]);
    h.run_steps(3);
    let sheet = h.state().pad.rects.unwrap().sheet.expect("no sheet");
    assert_eq!(panel(&h, Panel::Layers), Some(sheet));
    for name in [tab_name("Layers"), tab_name(TOOLBOX)] {
        assert!(control(&h, &name).is_some(), "{name}");
    }
    assert!(control(&h, "Pin Layers").is_none());
    assert!(control(&h, DOCK_WIDTH).is_none());
    quick_tap(&mut h, &tab_name(TOOLBOX));
    assert_eq!(panel(&h, Panel::Toolbox), Some(sheet));
    check_controls(&h, "the sheet of the Toolbox tab");
    // A long press on a tab opens a menu with Close only.
    long_press(&mut h, &tab_name("Layers"));
    assert_eq!(
        h.state().pad.pane_menu.map(|m| m.panel),
        Some(Panel::Layers)
    );
    assert!(control(&h, vale_app::ui::pad::dock_ui::MOVE_TO_NEW_PANE).is_none());
    tap(&mut h, &close_name("Layers"));
    assert_eq!(dock_tabs(&h), [[Panel::Toolbox]]);
    // The Close button of the sheet closes its pane.
    tap(&mut h, &close_name(TOOLBOX));
    assert!(h.state().pad.docks.world.panes.is_empty());
    assert!(h.state().pad.rects.unwrap().sheet.is_none());
}

#[test]
fn the_tool_column_holds_the_tools_a_divider_and_the_controls_of_the_tool() {
    let mut h = pad(BOARDS[0]);
    let r = h.state().pad.rects.unwrap();
    let column = r.tools;
    assert!(!r.row);
    // The six tools are at the top, from Sculpt to Measure.
    let tools = [SCULPT, TERRAIN, VECTOR, select::TOOL, "Pan", MEASURE];
    let last = control(&h, MEASURE).unwrap();
    for pair in tools.windows(2) {
        let (a, b) = (h.get_by_label(pair[0]), h.get_by_label(pair[1]));
        let (a, b) = (a.rect(), b.rect());
        assert_eq!(a.bottom(), b.top(), "{pair:?}");
    }
    // Below a divider, the controls of the brush: the two sliders and the
    // Brush button.
    let size = control(&h, "Size").unwrap();
    assert_eq!(size.top(), last.bottom() + 13.0);
    let flow = control(&h, "Flow").unwrap();
    assert_eq!(flow.top(), size.bottom());
    let button = control(&h, "Brush settings").unwrap();
    assert_eq!(button.top(), flow.bottom());
    for rect in [size, flow, button] {
        assert!(column.contains_rect(rect));
    }
    // The Measure tool has its cells below the divider.
    quick_tap(&mut h, MEASURE);
    let column = h.state().pad.rects.unwrap().tools;
    let snap = control(&h, SNAP).unwrap();
    assert_eq!(snap.top(), control(&h, MEASURE).unwrap().bottom() + 13.0);
    for name in [SNAP, "Brush settings"] {
        assert!(column.contains_rect(control(&h, name).unwrap()), "{name}");
    }
    assert!(h.query_by_label(CLEAR_PATH).is_some());
    // The Move tool has no controls, so the column ends after the tools.
    quick_tap(&mut h, "Pan");
    let r = h.state().pad.rects.unwrap();
    assert_eq!(r.tools.height(), 12.0 + 6.0 * 44.0);
    assert_eq!(
        r.tools.bottom(),
        control(&h, MEASURE).unwrap().bottom() + 6.0
    );
    check_controls(&h, "the column of the Move tool");
}

#[test]
fn a_hold_on_a_control_of_the_column_shows_its_name() {
    let mut h = pad(BOARDS[0]);
    quick_tap(&mut h, MEASURE);
    let column = h.state().pad.rects.unwrap().tools;
    let pos = control(&h, SNAP).unwrap().center();
    let on = h.state().globe.snap.on;
    touch(&mut h, egui::TouchPhase::Start, pos, None);
    let mut steps = 0;
    while h.state().pad.flyouts.tip_rect.is_none() && steps < 120 {
        h.step();
        steps += 1;
    }
    let tip = h.state().pad.flyouts.tip_rect.expect("a tooltip");
    assert!(tip.left() > column.right());
    assert!(h.query_by_label(SNAP).is_some());
    h.run_steps(60);
    touch(&mut h, egui::TouchPhase::End, pos, None);
    h.run_steps(3);
    assert_eq!(h.state().pad.flyouts.tip_rect, None);
    // A hold longer than a click does not change the switch.
    assert_eq!(h.state().globe.snap.on, on);
}

#[test]
fn the_readout_is_one_card_with_the_view_button_and_the_scale_bar() {
    for size in BOARDS.into_iter().chain([(393.0, 852.0)]) {
        for view in WORLD_VIEWS {
            let mut h = pad(size);
            let what = format!("{size:?} {view:?}");
            h.state_mut().globe.world_view = view;
            let center = h.state().globe.rect.center();
            h.event(egui::Event::PointerMoved(center));
            h.run_steps(2);
            assert!(h.state().pad.readout_scale.is_some(), "{what}");
            let rects = h.state().pad.rects.unwrap();
            let band = rects.readout.unwrap();
            let card = h.state().pad.view_rect.unwrap();
            assert!(band.contains_rect(card), "{what}");
            assert_eq!(card.left_bottom(), band.left_bottom(), "{what}");
            assert_eq!(card.height(), 52.0, "{what}");
            let button = control(&h, VIEW).unwrap();
            assert_eq!(button.min, card.min + vec2(4.0, 4.0), "{what}");
            // The scale bar is at the right of the button.
            assert!(card.right() >= button.right() + 60.0, "{what}");
            check_controls(&h, &what);
        }
    }
}

#[test]
fn the_readout_shows_the_place_and_a_scale_bar_with_a_round_distance() {
    for size in BOARDS {
        let mut h = pad(size);
        let what = format!("{size:?}");
        // A pointer over the globe gives the place.
        let center = h.state().globe.rect.center();
        h.event(egui::Event::PointerMoved(center));
        h.run_steps(2);
        let place = readout_line(h.state());
        assert!(place.contains('°') && place != "–", "{what}: {place}");
        let label = h.state().pad.readout_scale.clone().expect("a scale bar");
        let (value, unit) = label.split_once(' ').unwrap();
        assert!(["km", "m"].contains(&unit), "{what}: {label}");
        let value: f64 = value.parse().unwrap();
        let lead = value / 10f64.powf(value.log10().floor());
        assert!(
            [1.0, 2.0, 5.0].iter().any(|r| (lead - r).abs() < 1e-9),
            "{what}: {label}"
        );
        // A closer view has a shorter distance.
        h.state_mut().globe.view.zoom *= 4.0;
        h.run_steps(2);
        let closer = h.state().pad.readout_scale.clone().unwrap();
        assert_ne!(closer, label, "{what}");
        // Miles in the imperial units.
        h.state_mut().doc.project.world.units = Units::Imperial;
        h.run_steps(2);
        let miles = h.state().pad.readout_scale.clone().unwrap();
        assert!(
            miles.ends_with(" mi") || miles.ends_with(" ft"),
            "{what}: {miles}"
        );
        // The readout is at the lower left of the canvas, at the right of
        // the tool column.
        let r = h.state().pad.rects.unwrap();
        let readout = r.readout.unwrap();
        if !r.row {
            assert_eq!(readout.left(), r.tools.right() + 16.0, "{what}");
            assert_eq!(readout.bottom(), size.1 - 20.0 - 8.0, "{what}");
        }
    }
}

#[test]
fn the_workspace_switch_has_icons_and_a_hold_names_them() {
    use vale_app::ui::pad::flyout::Place;
    for size in [BOARDS[0], BOARDS[1]] {
        let mut h = pad(size);
        let what = format!("{size:?}");
        for name in ["World", "Atlas"] {
            let button = control(&h, name).unwrap_or_else(|| panic!("{what}: {name}"));
            assert_eq!(button.size(), vec2(44.0, 44.0), "{what}: {name}");
        }
        assert_eq!(toggled(&h, "World"), Some(true), "{what}");
        // A hold shows the name below the button.
        let pos = control(&h, "Atlas").unwrap().center();
        touch(&mut h, egui::TouchPhase::Start, pos, None);
        let mut steps = 0;
        while h.state().pad.flyouts.tip.is_none() && steps < 120 {
            h.step();
            steps += 1;
        }
        let tip = h.state().pad.flyouts.tip.clone().expect("a tooltip");
        assert_eq!((tip.name.as_str(), tip.place), ("Atlas", Place::Below));
        let card = h.state().pad.flyouts.tip_rect.unwrap();
        assert!(
            card.top() > control(&h, "Atlas").unwrap().bottom(),
            "{what}"
        );
        touch(&mut h, egui::TouchPhase::End, pos, None);
        h.run_steps(3);
        assert_eq!(h.state().pad.flyouts.tip, None, "{what}");
        // A tap switches the workspace.
        tap(&mut h, "Atlas");
        assert_eq!(h.state().workspace, Workspace::Atlas, "{what}");
        assert_eq!(toggled(&h, "Atlas"), Some(true), "{what}");
        tap(&mut h, "World");
        assert_eq!(h.state().workspace, Workspace::Globe, "{what}");
    }
}

#[test]
fn a_quick_flick_on_a_header_hides_the_dock() {
    use vale_app::ui::pad::dock_ui::tab_name;
    let mut h = promotion_pad();
    let from = control(&h, &tab_name(TOOLBOX)).unwrap().center();
    // 60 points in 10 frames: 720 points each second.
    touch(&mut h, egui::TouchPhase::Start, from, None);
    h.step();
    for i in 1..=10 {
        let at = from + vec2(6.0 * i as f32, 0.0);
        touch(&mut h, egui::TouchPhase::Move, at, None);
        h.step();
    }
    touch(&mut h, egui::TouchPhase::End, from + vec2(60.0, 0.0), None);
    h.run_steps(3);
    assert!(h.state().pad.docks.world.collapsed);
}

#[test]
fn a_flick_that_moves_and_ends_in_one_frame_hides_the_dock() {
    use vale_app::ui::pad::dock_ui::tab_name;
    let mut h = promotion_pad();
    let from = control(&h, &tab_name(TOOLBOX)).unwrap().center();
    touch(&mut h, egui::TouchPhase::Start, from, None);
    h.step();
    // A slow frame: the moves and the end of the flick come together.
    for dx in [20.0, 40.0, 60.0] {
        touch(&mut h, egui::TouchPhase::Move, from + vec2(dx, 2.0), None);
    }
    touch(&mut h, egui::TouchPhase::End, from + vec2(60.0, 2.0), None);
    h.run_steps(3);
    assert!(h.state().pad.docks.world.collapsed);
    // A tap on a tab hides nothing.
    h.state_mut().pad.docks.world.collapsed = false;
    h.run_steps(3);
    let from = control(&h, &tab_name(TOOLBOX)).unwrap().center();
    touch(&mut h, egui::TouchPhase::Start, from, None);
    touch(&mut h, egui::TouchPhase::End, from, None);
    h.run_steps(3);
    assert!(!h.state().pad.docks.world.collapsed);
}

/// A layout with a pane of two tabs, with the frames of a ProMotion screen:
/// 120 each second. The harness puts the end of a touch in a frame of its
/// own, so the steps of other tests are too long for a flick.
fn promotion_pad() -> Pad {
    let mut h = Harness::builder()
        .with_size(vec2(BOARDS[0].0, BOARDS[0].1))
        .with_pixels_per_point(1.0)
        .with_step_dt(1.0 / 120.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state());
    h.run_steps(3);
    h.state_mut()
        .pad
        .docks
        .world
        .open_tabs(&[Panel::Layers, Panel::Toolbox]);
    h.run_steps(3);
    h
}

#[test]
fn a_drag_on_the_edge_of_the_dock_takes_the_width_handle_and_a_tap_there_reaches_the_row() {
    use vale_app::ui::pad::dock_ui::DOCK_WIDTH;
    let mut h = docked(BOARDS[0], &[&[Panel::Layers]]);
    let dock = h.state().pad.dock_rect.unwrap();
    let handle = control(&h, DOCK_WIDTH).unwrap();
    // The handle is on the edge, with a part in the dock.
    assert!(handle.left() < dock.left() && handle.right() > dock.left() + 10.0);
    // A drag that starts just inside the dock changes its width.
    let at = pos2(dock.left() + 8.0, handle.center().y);
    drag(&mut h, at, at - vec2(60.0, 0.0), None);
    assert!(h.state().pad.docks.world.width > 380.0);
}
