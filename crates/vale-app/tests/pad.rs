//! The iPad layout of the world workspace.

mod common;

use eframe::egui::{self, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use vale_app::document::Document;
use vale_app::globe::view::GlobeView;
use vale_app::globe::{StackItem, Tool, WorldView};
use vale_app::headless;
use vale_app::projects::Locations;
use vale_app::session::Session;
use vale_app::ui::gradient::{COLUMN, ColumnParts, GRADIENT, band_row, widest_gap_value};
use vale_app::ui::pad::geometry::{BrushCard, WidthClass};
use vale_app::ui::pad::raster;
use vale_app::ui::pad::widgets::SLIDER_INSET;
use vale_app::ui::pad::{
    ADD_LAYER, EXPORT_GEOTIFF, GRATICULE, IMPORT_HEIGHTMAP, LAYER_NAME, LINE, LayerRef, MORE,
    NEW_RASTER, OPACITY, POLYGON, Page, Panel, RECENTER, RESET, SMOOTHING, SNAP, SNAP_DISTANCE,
    SNAP_EDGES, SNAP_VERTICES, SNAPPING, VIEW_OPTIONS, WORLD_PROPERTIES, WORLDS, WORLDS_ROW,
    brush_size_text, readout_text, theme,
};
use vale_app::ui::raster_form::{self, PRESETS, RasterForm, face_size_name};
use vale_app::ui::world_form::{CREATE, FormMode, NAME, NEW_WORLD, RADIUS_SLIDER, WorldForm};
use vale_app::ui::{Action, AppState, Layout, Workspace, draw, panels};
use vale_app::world::{radius_fraction, three_significant};
use vale_store::{Hillshade, RasterDisplay, RasterId, Units, World};
use vale_terrain::{Mode, meters_to_level};

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

/// A tap of a finger on the middle of a control.
fn tap(h: &mut Pad, name: &str) {
    let rect = control(h, name).unwrap_or_else(|| panic!("no control `{name}`"));
    let pos = rect.center();
    touch(h, egui::TouchPhase::Start, pos, None);
    h.step();
    touch(h, egui::TouchPhase::End, pos, None);
    h.run_steps(3);
}

/// The rectangles of all cards on the screen.
fn cards(h: &Pad) -> Vec<Rect> {
    let pad = &h.state().pad;
    let r = pad.rects.unwrap();
    let mut cards = vec![r.workspace, r.actions, r.tools];
    cards.extend(r.view);
    cards.extend(r.brush);
    cards.extend(pad.panels.iter().map(|(_, rect)| *rect));
    cards.extend(pad.menu_rect);
    cards.extend(pad.more_rect);
    cards.extend(pad.add_rect);
    cards.extend(pad.view_options_rect);
    cards
}

#[test]
fn the_canvas_fills_the_screen_and_the_panels_float_over_it() {
    for size in BOARDS {
        let mut h = pad(size);
        let screen = Rect::from_min_size(Pos2::ZERO, size.into());
        assert_eq!(h.state().globe.rect, screen, "{size:?}");
        tap(&mut h, "Layers");
        if !h.state().pad.rects.unwrap().one_panel {
            tap(&mut h, "Brush settings");
        }
        assert!(!h.state().pad.panels.is_empty());
        assert_eq!(h.state().globe.rect, screen, "{size:?}");
        for card in cards(&h) {
            assert!(screen.contains_rect(card), "{size:?}: {card:?}");
        }
    }
}

/// Makes sure that a finger can hit each control of the frame.
fn check_controls(h: &Pad, what: &str) {
    let screen = h.state().pad.rects.unwrap().screen;
    let controls = &h.state().pad.controls;
    assert!(controls.len() >= 7, "{what}: {} controls", controls.len());
    let in_menu = h.state().pad.menu_rect.is_some();
    let in_more = h.state().pad.more_rect.is_some();
    let in_add = h.state().pad.add_rect.is_some();
    let in_list = h.state().pad.projections_rect.is_some();
    let in_stop = h.state().pad.style.card.is_some() || h.state().pad.style.reset_card.is_some();
    let in_edit = controls
        .iter()
        .any(|c| c.name == vale_app::ui::pad::widgets::COPY);
    let popup = in_edit
        || in_menu
        || in_more
        || in_add
        || in_list
        || in_stop
        || h.state().pad.view_options_rect.is_some();
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
            // options, the open menu of the More cell, and the open menu of
            // the new layers cover the cards below them.
            let row = |c: &vale_app::ui::pad::widgets::Control| {
                let name = c.name.as_str();
                let kind = vale_sphere::ProjectionKind::ALL
                    .iter()
                    .any(|k| k.name() == name);
                let menu =
                    ["World", "Globe", "Flat", GRATICULE, "Atlas", WORLDS_ROW].contains(&name);
                let more = in_more
                    && [
                        "Carve", "Ridge", "Deposit", "Erode", "Weather", "Uplift", "Warp",
                    ]
                    .contains(&name);
                let rows = [NEW_RASTER, "Lines", "Points", "Polygons", IMPORT_HEIGHTMAP];
                let add = in_add && rows.contains(&name);
                let stop = in_stop
                    && (name.starts_with(vale_app::ui::pad::raster::COLOR) || raster_stop(name));
                menu || kind || more || add || stop || (in_menu && name == "Projection")
            };
            // The edit menu of a field covers the controls below it.
            let edit = |c: &vale_app::ui::pad::widgets::Control| {
                use vale_app::ui::pad::widgets::{COPY, PASTE};
                c.name == COPY || c.name == PASTE
            };
            let covered = popup && (row(a) != row(b) || edit(a) != edit(b));
            let free = !a.rect.shrink(0.5).intersects(b.rect.shrink(0.5));
            assert!(free || covered, "{what}: {name} and {} overlap", b.name);
        }
    }
}

/// A control of the card of the selected stop.
fn raster_stop(name: &str) -> bool {
    use vale_app::ui::pad::raster;
    [
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
        for name in ["Layers", "Open Height", "Toolbox", "Brush settings"] {
            if control(&h, name).is_none() {
                // An open sheet covers the Brush button.
                assert_eq!(name, "Brush settings");
                tap(&mut h, "Close Toolbox");
            }
            tap(&mut h, name);
            check_controls(&h, &format!("{size:?} after {name}"));
        }
        tap(&mut h, MORE);
        assert!(h.state().pad.more);
        for name in [
            MORE, "Carve", "Ridge", "Deposit", "Erode", "Weather", "Uplift", "Warp",
        ] {
            assert!(control(&h, name).is_some(), "{size:?}: {name}");
        }
        check_controls(&h, &format!("{size:?} with the More menu"));
        tap(&mut h, MORE);
        if control(&h, "Workspace").is_some() {
            tap(&mut h, "Workspace");
            assert!(h.state().pad.menu);
            check_controls(&h, &format!("{size:?} with the menu"));
            h.state_mut().pad.menu = false;
        }

        // The list of the layers, the menu of the new layers, and the panel
        // of a vector layer.
        let pad = &mut h.state_mut().pad;
        pad.brush = false;
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
        for name in [LAYER_NAME, "Back to Layers", "Close Lines 1"] {
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
        StackItem::Vector(_) => None,
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
        for name in ["Raise", "Lower", "Smooth", "Flatten", MORE] {
            assert!(control(&h, name).is_none(), "{what}: {name}");
        }
        assert!(control(&h, "Pan").is_some(), "{what}");
        tap(&mut h, "Layers");
        assert!(control(&h, "Layer Height").is_none(), "{what}");
        assert!(control(&h, IMPORT_HEIGHTMAP).is_none(), "{what}");

        // The first row of the menu opens the form of a raster layer. The
        // form adds a layer, selects it, and opens its panel.
        tap(&mut h, ADD_LAYER);
        let (raster, lines) = (control(&h, NEW_RASTER), control(&h, "Lines"));
        assert!(raster.unwrap().bottom() <= lines.unwrap().top() + 0.5);
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
    let cells = ["Raise", "Lower", "Smooth", "Flatten", MORE, "Size", "Flow"];
    let on = |h: &Pad| cells.map(|name| control(h, name).is_some());
    assert_eq!(on(&h), [true; 7]);
    assert_eq!(h.state().globe.tool, Tool::Brush);
    h.state_mut().select(None);
    h.run_steps(2);
    assert_eq!(on(&h), [false; 7]);
    // The six cells stay in their places, and the Move cell is the tool.
    let tools = h.state().pad.rects.unwrap().tools;
    assert!(tools.contains_rect(control(&h, "Pan").unwrap()));
    assert!(!h.state().globe.brushing());
    let height = h.state().doc.project.rasters()[0].id;
    h.state_mut().select(Some(LayerRef::Raster(height)));
    h.run_steps(2);
    assert_eq!(on(&h), [true; 7]);
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
    tap(&mut h, MORE);
    for name in ["Carve", "Ridge", "Erode", "Weather", "Uplift"] {
        assert!(control(&h, name).is_none(), "{name}");
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
        tap(&mut h, "Layers");
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
        tap(&mut h, "Layers");
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
    tap(&mut h, "Layers");
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
        tap(&mut h, "Layers");
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
        tap(&mut h, "Layers");
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

    // The sliders of the Brush panel go over the range of the layer.
    tap(&mut h, "Brush settings");
    let slider = control(&h, "Brush floor level").unwrap();
    let y = slider.center().y;
    drag(
        &mut h,
        pos2(slider.center().x, y),
        pos2(slider.right() + 40.0, y),
        None,
    );
    assert_eq!(h.state().globe.brush.floor_m, 4000.0);
    drag(
        &mut h,
        pos2(slider.center().x, y),
        pos2(slider.left() - 40.0, y),
        None,
    );
    assert_eq!(h.state().globe.brush.floor_m, 0.0);

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

        tap(&mut h, "Layers");
        tap(&mut h, "Open Height");
        check_controls(&h, &format!("{what} with the panel of a raster layer"));
        type_text(&mut h, LAYER_NAME, "Ground");
        assert_eq!(raster_name(&h, height), "Ground");
        assert!(control(&h, "Close Ground").is_some(), "{what}");
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
        tap(&mut h, "Layers");
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
        assert!(control(&h, "Close Polygons 1").is_some());

        type_text(&mut h, LAYER_NAME, "Coast");
        assert_eq!(h.state().doc.layer(id).unwrap().name, "Coast");
        assert!(control(&h, "Close Coast").is_some());

        tap(&mut h, "Back to Layers");
        assert_eq!(h.state().pad.layer, None);
        tap(&mut h, "Hide Coast");
        assert!(!h.state().doc.visible(id));
        assert!(control(&h, "Hide Coast").is_none());
        tap(&mut h, "Show Coast");
        assert!(h.state().doc.visible(id));

        swipe(&mut h, "Coast", -70.0);
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
    tap(&mut h, "Layers");
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
        tap(&mut h, "Layers");
        let list = panel(&h, Panel::Layers).unwrap();
        let rivers = vector(&h, "Rivers");
        tap(&mut h, "Open Rivers");
        assert_eq!(h.state().pad.layer, Some(LayerRef::Vector(rivers)));
        assert_eq!(h.state().selected_layer, Some(LayerRef::Vector(rivers)));
        let open = panel(&h, Panel::Layers).unwrap();
        assert_eq!(open.left_top(), list.left_top(), "{size:?}");
        assert_eq!(open.width(), list.width(), "{size:?}");
        for name in ["Close Rivers", "Back to Layers", LAYER_NAME] {
            assert!(control(&h, name).is_some(), "{size:?}: {name}");
        }
        assert!(control(&h, ADD_LAYER).is_none());
        tap(&mut h, "Back to Layers");
        tap(&mut h, "Open Height");
        let height = h.state().doc.project.rasters()[0].id;
        assert_eq!(h.state().pad.layer, Some(LayerRef::Raster(height)));
        assert_eq!(h.state().selected_layer, Some(LayerRef::Raster(height)));
        assert!(control(&h, "Close Height").is_some());

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
        tap(&mut h, "Layers");
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
        tap(&mut h, "Layers");
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
        tap(&mut h, "Layers");
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
        tap(&mut h, "Layers");
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

        // A tap on the track sets a whole percent.
        let middle = pos2(slider.center().x + 3.3, y);
        touch(&mut h, egui::TouchPhase::Start, middle, None);
        step_and_save(&mut h, true);
        touch(&mut h, egui::TouchPhase::End, middle, None);
        for _ in 0..3 {
            step_and_save(&mut h, false);
        }
        let half = opacity(&h);
        assert!(half > 0.3 && half < 0.7, "{what}: {half}");
        assert_eq!((half * 100.0).round() / 100.0, half, "{what}");
        assert_eq!(saves(&h), saved + 2, "{what}");

        // One undo puts back the opacity from before the tap, and the next
        // one puts back the opacity from before the drag.
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
    tap(&mut h, "Layers");
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
    swipe(h, name, -70.0);
    tap(h, &format!("Delete {name}"));
}

#[test]
fn a_swipe_shows_delete_a_tap_deletes_and_undo_brings_it_back() {
    for size in BOARDS {
        let what = format!("{size:?}");
        let dir = tempfile::tempdir().unwrap();
        let mut h = saved_pad(size, dir.path());
        tap(&mut h, "Layers");
        assert!(control(&h, "Delete Rivers").is_none(), "{what}");
        swipe(&mut h, "Rivers", -70.0);
        let rivers = vector(&h, "Rivers");
        let open = h.state().pad.swipe.unwrap();
        assert_eq!((open.layer, open.moving), (rivers, false), "{what}");
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
        assert_eq!(open.offset, -88.0, "{what}");
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
        tap(&mut h, "Layers");
        swipe(&mut h, "Land", -30.0);
        assert_eq!(h.state().pad.swipe, None, "{what}");
        assert!(control(&h, "Delete Land").is_none(), "{what}");
        assert!(control(&h, "Hide Land").is_some(), "{what}");
        assert_eq!(layer_names(&h).len(), 4, "{what}");

        // A tap on the body of an open row shuts it, and a press elsewhere
        // shuts it.
        swipe(&mut h, "Land", -70.0);
        assert!(h.state().pad.swipe.is_some(), "{what}");
        tap(&mut h, "Layer Land");
        assert_eq!(h.state().pad.swipe, None, "{what}");
        swipe(&mut h, "Land", -70.0);
        tap(&mut h, "Layer Places");
        assert_eq!(h.state().pad.swipe, None, "{what}");
        assert!(control(&h, "Hide Land").is_some(), "{what}");
        // Only one row is open.
        swipe(&mut h, "Land", -70.0);
        swipe(&mut h, "Places", -70.0);
        let open = h.state().pad.swipe.map(|s| s.layer);
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
    tap(&mut h, "Layers");
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
        tap(&mut h, "Layers");
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
        tap(&mut h, "Layers");
        assert!(control(&h, "Open Height").is_none());
        tap(&mut h, ADD_LAYER);
        check_controls(&h, &format!("{size:?} with the import row"));
        tap(&mut h, IMPORT_HEIGHTMAP);
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
    tap(&mut h, "Layers");
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
    tap(&mut h, "Layers");
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
    tap(&mut h, "Layers");
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
        tap(&mut h, "Layers");
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
        tap(&mut h, name);
        assert_eq!(h.state().globe.tool, Tool::Brush);
        assert_eq!(h.state().globe.brush.mode, mode);
    }
    tap(&mut h, MORE);
    tap(&mut h, "Carve");
    assert_eq!(h.state().globe.brush.mode, Mode::Carve);
    tap(&mut h, "Pan");
    assert_eq!(h.state().globe.tool, Tool::Navigate);

    for (name, which) in [
        ("Layers", Panel::Layers),
        ("Toolbox", Panel::Toolbox),
        ("Brush settings", Panel::Brush),
    ] {
        tap(&mut h, name);
        assert!(h.state().pad.is_open(which), "{name}");
    }
    assert!(!h.state().pad.is_open(Panel::Layers));

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

/// True if the More cell has the active highlight.
fn more_is_active(h: &Pad) -> bool {
    let cell = h.get_by_label(MORE);
    cell.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

#[test]
fn the_more_cell_opens_a_menu_with_the_modes_that_have_no_cell() {
    let mut h = pad(BOARDS[0]);
    let tools = h.state().pad.rects.unwrap().tools;
    let strip = |h: &Pad| -> Vec<String> {
        let controls = h.state().pad.controls.iter();
        let cells = controls.filter(|c| tools.contains_rect(c.rect));
        cells.map(|c| c.name.clone()).collect()
    };
    let cells = ["Raise", "Lower", "Smooth", "Flatten", MORE, "Pan"];
    assert_eq!(strip(&h), cells);
    assert!(!more_is_active(&h));
    for (name, mode) in [
        ("Warp", Mode::Warp),
        ("Uplift", Mode::Uplift),
        ("Weather", Mode::Weather),
        ("Erode", Mode::Erode),
        ("Carve", Mode::Carve),
        ("Ridge", Mode::Ridge),
        ("Deposit", Mode::Deposit),
    ] {
        // A row of the menu also picks the brush tool.
        tap(&mut h, "Pan");
        assert!(!more_is_active(&h));
        assert!(control(&h, name).is_none());
        tap(&mut h, MORE);
        assert!(h.state().pad.more);
        let menu = h.state().pad.more_rect.unwrap();
        let cell = control(&h, MORE).unwrap();
        assert_eq!(menu.left_top(), pos2(tools.right() + 12.0, cell.top()));
        for row in [
            "Carve", "Ridge", "Deposit", "Erode", "Weather", "Uplift", "Warp",
        ] {
            assert!(menu.contains_rect(control(&h, row).unwrap()), "{row}");
        }
        assert_eq!(strip(&h), cells);

        tap(&mut h, name);
        let s = h.state();
        assert_eq!((s.globe.tool, s.globe.brush.mode), (Tool::Brush, mode));
        assert!(!s.pad.more && s.pad.more_rect.is_none());
        assert!(control(&h, name).is_none());
        assert!(more_is_active(&h));
        assert_eq!(strip(&h), cells);
    }
    // The cell of another mode takes the highlight.
    tap(&mut h, "Smooth");
    assert!(!more_is_active(&h));

    // The Brush panel has the angle in the Weather mode only.
    tap(&mut h, "Brush settings");
    assert!(control(&h, "Brush angle").is_none());
    tap(&mut h, MORE);
    // The menu is in front of the Brush panel.
    let menu = h.state().pad.more_rect.unwrap();
    assert!(menu.intersects(panel(&h, Panel::Brush).unwrap()));
    tap(&mut h, "Weather");
    assert_eq!(h.state().globe.brush.mode, Mode::Weather);
    assert!(h.state().pad.is_open(Panel::Brush));
    let rect = control(&h, "Brush angle").unwrap();
    drag(&mut h, rect.center(), rect.right_center(), None);
    assert!((h.state().globe.brush.talus_deg - 20.0).abs() < 1e-9);
}

#[test]
fn the_brush_panel_has_the_two_sliders_of_the_uplift_mode() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Brush settings");
    assert!(control(&h, "Brush erosion").is_none());
    tap(&mut h, MORE);
    tap(&mut h, "Uplift");
    let s = h.state();
    assert_eq!(
        (s.globe.tool, s.globe.brush.mode),
        (Tool::Brush, Mode::Uplift)
    );
    assert!(more_is_active(&h));
    // The strength slider sets the rate of the uplift.
    let rect = control(&h, "Brush strength").unwrap();
    drag(&mut h, rect.center(), rect.right_center(), None);
    assert!((h.state().globe.brush.strength_m - 6000.0).abs() < 1e-6);
    let rect = control(&h, "Brush erosion").unwrap();
    drag(&mut h, rect.center(), rect.left_center(), None);
    assert_eq!(h.state().globe.brush.erosion, 0.0);
    drag(&mut h, rect.left_center(), rect.right_center(), None);
    assert_eq!(h.state().globe.brush.erosion, 1.0);
    assert!(control(&h, "Brush angle").is_some());
    check_controls(&h, "the Uplift mode");
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
    for mode in [None, Some("Erode"), Some("Uplift"), Some("Warp")] {
        let mut h = pad(BOARDS[0]);
        tap(&mut h, "Brush settings");
        if let Some(mode) = mode {
            tap(&mut h, MORE);
            tap(&mut h, mode);
        }
        let what = format!("{mode:?}");
        // Both switches are off at the start, and no level shows.
        let brush = h.state().globe.brush;
        assert!(!brush.floor_on && !brush.ceiling_on, "{what}");
        assert_eq!((brush.floor_m, brush.ceiling_m), (0.0, 1000.0), "{what}");
        for (switch, level) in SWITCHES.iter().zip(LEVELS) {
            assert!(!switch_is_on(&h, switch), "{what}: {switch}");
            assert!(control(&h, level).is_none(), "{what}: {level}");
        }
        check_controls(&h, &format!("{what} with the limits off"));

        // The floor goes on alone, and its level shows.
        tap(&mut h, SWITCHES[0]);
        let brush = h.state().globe.brush;
        assert!(brush.floor_on && !brush.ceiling_on, "{what}");
        assert!(switch_is_on(&h, SWITCHES[0]) && !switch_is_on(&h, SWITCHES[1]));
        assert!(control(&h, LEVELS[0]).is_some() && control(&h, LEVELS[1]).is_none());
        // The switch and its level are two rows of the panel, and the track
        // of the level is as wide as the track of the size.
        let (switch, level) = (control(&h, SWITCHES[0]), control(&h, LEVELS[0]));
        let (switch, level) = (switch.unwrap(), level.unwrap());
        let size = control(&h, "Brush size").unwrap();
        assert_eq!(level.top(), switch.bottom(), "{what}");
        assert_eq!(switch.x_range(), size.x_range(), "{what}");
        assert_eq!(level.x_range(), size.x_range(), "{what}");

        tap(&mut h, SWITCHES[1]);
        let brush = h.state().globe.brush;
        assert!(brush.floor_on && brush.ceiling_on, "{what}");
        check_controls(&h, &format!("{what} with the limits on"));

        // A drag on a level sets the meters, in steps of 50 m.
        let rect = control(&h, LEVELS[0]).unwrap();
        drag(&mut h, rect.center(), rect.left_center(), None);
        assert_eq!(h.state().globe.brush.floor_m, -6000.0, "{what}");
        let to = rect.center() + vec2(-17.0, 0.0);
        drag(&mut h, rect.left_center(), to, None);
        let floor = h.state().globe.brush.floor_m;
        assert!(floor < 0.0 && floor > -3000.0, "{what}: {floor}");
        assert_eq!(floor % 50.0, 0.0, "{what}: {floor}");
        let rect = control(&h, LEVELS[1]).unwrap();
        drag(&mut h, rect.center(), rect.right_center(), None);
        assert_eq!(h.state().globe.brush.ceiling_m, 6000.0, "{what}");
        // The floor can be above the ceiling.
        drag(&mut h, rect.right_center(), rect.left_center(), None);
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
        assert!(control(&h, LEVELS[0]).is_none() && control(&h, LEVELS[1]).is_some());
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
            let names = SWITCHES.iter().chain(&LEVELS);
            names.filter(|name| control(h, name).is_some()).count()
        };
        if h.state().pad.rects.unwrap().sheet.is_some() {
            // The sheet is too short for the rows. A finger that moves up
            // on a slider scrolls the rows and does not move the slider.
            assert_eq!(shown(&h), 0, "{size:?}");
            let brush = h.state().globe.brush;
            let from = control(&h, "Brush angle").unwrap().center();
            drag(&mut h, from, from - vec2(4.0, 260.0), None);
            assert_eq!(h.state().globe.brush, brush, "{size:?}");
            check_controls(&h, &format!("{size:?} after the scroll"));
        }
        assert_eq!(shown(&h), 4, "{size:?}");
        tap(&mut h, SWITCHES[1]);
        assert!(!h.state().globe.brush.ceiling_on, "{size:?}");
    }
}

#[test]
fn the_brush_panel_has_the_two_sliders_of_the_warp_mode() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Brush settings");
    assert!(control(&h, "Brush scale").is_none());
    tap(&mut h, MORE);
    tap(&mut h, "Warp");
    let s = h.state();
    assert_eq!(
        (s.globe.tool, s.globe.brush.mode),
        (Tool::Brush, Mode::Warp)
    );
    assert!(more_is_active(&h));
    assert_eq!(
        (s.globe.brush.warp_km, s.globe.brush.scale_km),
        (50.0, 200.0)
    );
    // The strength slider sets the offset in kilometers.
    let meters = s.globe.brush.strength_m;
    let rect = control(&h, "Brush strength").unwrap();
    drag(&mut h, rect.center(), rect.right_center(), None);
    assert!((h.state().globe.brush.warp_km - 500.0).abs() < 1e-6);
    assert_eq!(h.state().globe.brush.strength_m, meters);
    let rect = control(&h, "Brush scale").unwrap();
    drag(&mut h, rect.center(), rect.left_center(), None);
    assert!((h.state().globe.brush.scale_km - 5.0).abs() < 1e-9);
    drag(&mut h, rect.left_center(), rect.right_center(), None);
    assert!((h.state().globe.brush.scale_km - 2000.0).abs() < 1e-6);
    // The seed slider sets the seed of the world.
    assert_eq!(h.state().doc.project.world.seed, 0);
    let rect = control(&h, "Brush seed").unwrap();
    drag(&mut h, rect.center(), rect.right_center(), None);
    assert_eq!(h.state().doc.project.world.seed, 999);
    assert_eq!(h.state().globe.world_seed, 999);
    assert!(control(&h, "Brush angle").is_none());
    check_controls(&h, "the Warp mode");
}

#[test]
fn a_press_outside_the_more_menu_closes_it() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Lower");
    tap(&mut h, MORE);
    assert!(h.state().pad.more);
    // A second tap on the cell closes the menu.
    tap(&mut h, MORE);
    assert!(!h.state().pad.more);

    tap(&mut h, MORE);
    let menu = h.state().pad.more_rect.unwrap();
    let outside = menu.right_bottom() + vec2(40.0, 40.0);
    touch(&mut h, egui::TouchPhase::Start, outside, None);
    h.step();
    touch(&mut h, egui::TouchPhase::End, outside, None);
    h.run_steps(3);
    let s = h.state();
    assert!(!s.pad.more && s.pad.more_rect.is_none());
    assert!(control(&h, "Weather").is_none());
    assert_eq!(
        (s.globe.tool, s.globe.brush.mode),
        (Tool::Brush, Mode::Lower)
    );

    // A tap on another control closes the menu too.
    tap(&mut h, MORE);
    tap(&mut h, "Layers");
    assert!(!h.state().pad.more && h.state().pad.is_open(Panel::Layers));
}

#[test]
fn the_globe_does_not_move_when_you_touch_a_card() {
    for size in BOARDS {
        let mut h = pad(size);
        tap(&mut h, "Layers");
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
            tap(&mut h, "Layers");
        }
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
    let mut h = gpu_pad(BOARDS[0]);
    let state = h.state_mut();
    state.pad.open(Panel::Brush);
    state.pad.open(Panel::Layers);
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
fn each_card_shows_the_blurred_canvas_and_the_blur_follows_the_globe() {
    let _gpu = one_gpu_test();
    // Light and dark stripes along the meridians, each 20 degrees wide.
    let mut h = blur_pad(|lon| match (lon / 20.0).floor().rem_euclid(2.0) {
        0.0 => u16::MAX,
        _ => 0,
    });
    let cards = cards(&h);
    assert!(cards.len() >= 7);
    let blurred = h.render().unwrap();
    blurred.save("../../target/app/test-pad-blur.png").unwrap();
    h.state_mut().globe.blur = false;
    h.run_steps(2);
    let plain = h.render().unwrap();
    h.state_mut().globe.blur = true;
    h.state_mut().globe.view = GlobeView::centered(10.0, 0.0);
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
fn the_pen_paints_on_the_canvas_and_a_panel_stays_open() {
    let _gpu = one_gpu_test();
    let mut h = gpu_pad(BOARDS[0]);
    tap(&mut h, "Layers");
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

    // A panel closes with its close button and with the button that opened it.
    tap(&mut h, "Close Layers");
    assert!(!h.state().pad.is_open(Panel::Layers));
    tap(&mut h, "Brush settings");
    assert!(!h.state().pad.is_open(Panel::Brush));
    tap(&mut h, "Toolbox");
    tap(&mut h, "Toolbox");
    assert_eq!(h.state().pad.right, None);
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
fn one_tap_switches_the_view_and_the_tool_and_the_panels_stay() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Lower");
    tap(&mut h, "Layers");
    tap(&mut h, "Brush settings");
    let panels = h.state().pad.panels.clone();
    assert_eq!(panels.len(), 2);
    for (name, view) in [("Flat", WorldView::Flat), ("Globe", WorldView::Globe)] {
        tap(&mut h, name);
        let s = h.state();
        assert_eq!(s.globe.world_view, view);
        assert_eq!(s.workspace, Workspace::Globe);
        assert_eq!(
            (s.globe.tool, s.globe.brush.mode),
            (Tool::Brush, Mode::Lower)
        );
        assert_eq!(s.pad.panels, panels);
    }
}

#[test]
fn the_view_switch_has_the_list_of_the_projections_of_the_flat_view() {
    use vale_sphere::ProjectionKind;
    let mut h = pad(BOARDS[0]);
    assert!(control(&h, "Projection").is_none());
    tap(&mut h, "Lower");
    tap(&mut h, "Flat");
    // The card is wider by the button, and it stays in the middle.
    let view = h.state().pad.rects.unwrap().view.unwrap();
    assert_eq!(view.center().x, 597.0);
    assert!(view.contains_rect(control(&h, "Projection").unwrap()));
    assert!(control(&h, "Projection").unwrap().width() >= 44.0);

    tap(&mut h, "Projection");
    let list = h.state().pad.projections_rect.unwrap();
    assert_eq!(list.top(), view.bottom() + 4.0);
    for kind in ProjectionKind::ALL {
        assert!(list.contains_rect(control(&h, kind.name()).unwrap()));
    }
    check_controls(&h, "with the projections");
    tap(&mut h, "Equal Earth");
    let s = h.state();
    assert_eq!(s.globe.flat.spec().kind, ProjectionKind::EqualEarth);
    assert!(!s.pad.projections && s.pad.projections_rect.is_none());
    assert_eq!(
        (s.globe.tool, s.globe.brush.mode),
        (Tool::Brush, Mode::Lower)
    );

    // A touch off the list closes it, and the globe has no list.
    tap(&mut h, "Projection");
    tap(&mut h, "Layers");
    assert!(!h.state().pad.projections);
    tap(&mut h, "Projection");
    tap(&mut h, "Globe");
    assert!(!h.state().pad.projections && control(&h, "Projection").is_none());
}

#[test]
fn the_view_options_menu_has_the_graticule_switch() {
    use vale_app::ui::pad::raster::{self, RIVERS};
    for size in BOARDS {
        let mut h = painted_pad(size);
        for view in WORLD_VIEWS {
            let what = format!("{size:?} {view:?}");
            h.state_mut().globe.world_view = view;
            h.run_steps(2);
            let rects = h.state().pad.rects.unwrap();
            // The compact layout has the switch in the workspace menu.
            let open = match rects.view {
                Some(card) => {
                    let button = control(&h, VIEW_OPTIONS).unwrap();
                    assert!(card.contains_rect(button), "{what}");
                    let right = card.right() - 4.0;
                    assert!((button.right() - right).abs() <= 0.5, "{what}");
                    assert!(card.right() + 12.0 <= rects.actions.left(), "{what}");
                    VIEW_OPTIONS
                }
                None => {
                    assert!(control(&h, VIEW_OPTIONS).is_none(), "{what}");
                    "Workspace"
                }
            };
            tap(&mut h, open);
            if let Some(card) = rects.view {
                let options = h.state().pad.view_options_rect.unwrap();
                assert_eq!(options.top(), card.bottom() + 4.0, "{what}");
                assert!(options.contains_rect(control(&h, GRATICULE).unwrap()));
            }
            check_controls(&h, &format!("{what} with the view options"));
            let before = h.state().globe.preview.graticule;
            tap(&mut h, GRATICULE);
            assert_eq!(h.state().globe.preview.graticule, !before, "{what}");
            tap(&mut h, GRATICULE);
            assert_eq!(h.state().globe.preview.graticule, before, "{what}");
            tap(&mut h, open);
            let pad = &h.state().pad;
            assert!(!pad.view_options && !pad.menu, "{what}");
            assert!(control(&h, GRATICULE).is_none(), "{what}");

            // The view options and the list of the projections exclude each
            // other, and a press outside closes the view options.
            if rects.view.is_some() && view == WorldView::Flat {
                tap(&mut h, VIEW_OPTIONS);
                tap(&mut h, "Projection");
                let pad = &h.state().pad;
                assert!(pad.projections && !pad.view_options, "{what}");
                tap(&mut h, VIEW_OPTIONS);
                let pad = &h.state().pad;
                assert!(!pad.projections && pad.view_options, "{what}");
                tap(&mut h, "Layers");
                assert!(!h.state().pad.view_options, "{what}");
                tap(&mut h, "Layers");
            }
        }

        // The panel of the height layer has the rivers and no graticule.
        open_height(&mut h);
        let mut rivers = control(&h, RIVERS).is_some();
        for _ in 0..8 {
            assert!(control(&h, GRATICULE).is_none(), "{size:?}");
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
                .unwrap_or(pos2(card.left() + 6.0, card.bottom() - 40.0));
            let to = pos2(from.x, (from.y - 400.0).max(40.0));
            drag(&mut h, from, to, None);
            rivers = control(&h, RIVERS).is_some();
        }
        assert!(rivers, "{size:?}: the panel has no rivers");
    }
}

#[test]
fn the_flat_view_has_buttons_that_move_the_center_of_the_projection_and_reset_it() {
    for size in BOARDS {
        let mut h = pad(size);
        h.state_mut().globe.world_view = WorldView::Flat;
        h.run_steps(2);
        let rects = h.state().pad.rects.unwrap();
        // The buttons are in the card of the view switch, to the right of
        // the switch. With no switch in the top row, they have a card below
        // the top row.
        let card = match rects.view {
            Some(view) => {
                assert_eq!(h.state().pad.center_rect, None);
                assert_eq!(view.center().x, size.0 / 2.0, "{size:?}");
                assert!(view.right() + 12.0 <= rects.actions.left(), "{size:?}");
                view
            }
            None => {
                let card = h.state().pad.center_rect.unwrap();
                assert_eq!(card.center().x, size.0 / 2.0, "{size:?}");
                assert_eq!(card.top(), rects.tools.top());
                assert!(!card.intersects(rects.tools));
                card
            }
        };
        // The middle of the view is the center of the projection, and the
        // view shows the whole map, so the two actions are off.
        assert!(control(&h, RECENTER).is_none() && control(&h, RESET).is_none());

        // A drag moves the map at the first zoom, with no zoom before it.
        let canvas = h.state().globe.rect;
        let from = pos2(canvas.center().x, canvas.bottom() - 120.0);
        drag(&mut h, from, from + vec2(-60.0, 0.0), None);
        let (recenter, reset) = (control(&h, RECENTER).unwrap(), control(&h, RESET).unwrap());
        assert!(card.contains_rect(recenter) && card.contains_rect(reset));
        assert_eq!(reset.left() - recenter.right(), 4.0);
        if rects.view.is_some() {
            let switch = control(&h, "Flat").unwrap();
            assert!(recenter.left() > control(&h, "Projection").unwrap().right());
            assert!(recenter.left() > switch.right() && recenter.top() == switch.top());
        }
        check_controls(&h, &format!("{size:?} with the center buttons"));
        tap(&mut h, RECENTER);
        let flat = &h.state().globe.flat;
        assert!(flat.spec().lon0 > 1.0, "{size:?}: {:?}", flat.spec());
        assert!(control(&h, RECENTER).is_none());

        tap(&mut h, RESET);
        let flat = &h.state().globe.flat;
        assert_eq!((flat.spec().lon0, flat.zoom), (0.0, 1.0));
        assert!(control(&h, RESET).is_none());

        // The globe has no buttons.
        h.state_mut().globe.world_view = WorldView::Globe;
        h.run_steps(2);
        assert_eq!(h.state().pad.center_rect, None);
        assert!(control(&h, RECENTER).is_none() && control(&h, RESET).is_none());
    }
}

#[test]
fn the_top_row_and_the_panels_match_the_landscape_board() {
    let mut h = pad(BOARDS[0]);
    let r = h.state().pad.rects.unwrap();
    assert_eq!(r.class, WidthClass::Wide);
    assert_eq!(r.workspace.min, pos2(16.0, 32.0));
    assert_eq!(r.workspace.height(), 52.0);
    // The Worlds button and a line are at the left of the workspaces.
    assert_eq!(control(&h, WORLDS).unwrap().min, pos2(20.0, 36.0));
    assert_eq!(control(&h, "World").unwrap().min, pos2(73.0, 36.0));
    assert!(control(&h, "Atlas").is_some());
    assert!(control(&h, "Workspace").is_none());
    assert!(r.view.unwrap().contains_rect(control(&h, "Flat").unwrap()));
    assert_eq!(r.view.unwrap().center(), pos2(597.0, 58.0));
    assert!(r.view.unwrap().contains_rect(control(&h, "Globe").unwrap()));
    assert_eq!(
        r.actions,
        Rect::from_min_size(pos2(969.0, 32.0), vec2(209.0, 52.0))
    );
    let lefts = ["Layers", "Toolbox"].map(|name| control(&h, name).unwrap().left());
    assert_eq!(lefts, [1082.0, 1130.0]);
    assert_eq!(
        r.tools,
        Rect::from_min_size(pos2(16.0, 96.0), vec2(64.0, 320.0))
    );
    assert_eq!(
        control(&h, "Raise").unwrap(),
        Rect::from_min_size(pos2(20.0, 100.0), vec2(56.0, 52.0))
    );
    assert_eq!(
        r.brush.unwrap(),
        Rect::from_min_size(pos2(16.0, 428.0), vec2(64.0, 340.0))
    );
    assert_eq!(
        control(&h, "Brush settings").unwrap().min,
        pos2(20.0, 712.0)
    );

    tap(&mut h, "Layers");
    tap(&mut h, "Brush settings");
    let layers = panel(&h, Panel::Layers).unwrap();
    assert_eq!((layers.min, layers.width()), (pos2(858.0, 96.0), 320.0));
    // The title row, the import row, and the three layers of the sample.
    assert_eq!(layers.height(), 52.0 + 4.0 * 48.0 + 10.0);
    assert_eq!(
        control(&h, "Close Layers").unwrap().min,
        pos2(1130.0, 100.0)
    );
    let brush = panel(&h, Panel::Brush).unwrap();
    assert_eq!((brush.min, brush.width()), (pos2(92.0, 96.0), 340.0));
    // Toolbox opens in the place of Layers.
    tap(&mut h, "Toolbox");
    assert_eq!(panel(&h, Panel::Layers), None);
    assert_eq!(panel(&h, Panel::Toolbox).unwrap().min, pos2(858.0, 96.0));
    assert!(panel(&h, Panel::Brush).is_some());
}

#[test]
fn the_top_row_and_the_panels_match_the_portrait_board() {
    let mut h = pad(BOARDS[1]);
    let r = h.state().pad.rects.unwrap();
    assert_eq!(r.class, WidthClass::Medium);
    assert!(control(&h, "World").is_none());
    assert_eq!(control(&h, WORLDS).unwrap().min, pos2(20.0, 36.0));
    let button = control(&h, "Workspace").unwrap();
    assert_eq!((button.left(), button.right()), (69.0, r.workspace.right()));
    assert_eq!(
        (r.workspace.min, r.workspace.height()),
        (pos2(16.0, 32.0), 52.0)
    );
    assert_eq!(r.view.unwrap().center(), pos2(417.0, 58.0));
    assert!(control(&h, "Globe").is_some());
    assert_eq!(r.actions.right(), 818.0);

    tap(&mut h, "Toolbox");
    let toolbox = panel(&h, Panel::Toolbox).unwrap();
    assert_eq!((toolbox.min, toolbox.width()), (pos2(498.0, 96.0), 320.0));
    tap(&mut h, "Brush settings");
    assert!(h.state().pad.is_open(Panel::Toolbox) && h.state().pad.is_open(Panel::Brush));

    // The menu holds the workspaces. The view switch stays in the top row.
    tap(&mut h, "Workspace");
    let menu = h.state().pad.menu_rect.unwrap();
    assert_eq!((menu.min, menu.width()), (pos2(16.0, 88.0), 240.0));
    assert!(menu.contains_rect(control(&h, "Atlas").unwrap()));
    tap(&mut h, "Atlas");
    assert_eq!(h.state().workspace, Workspace::Atlas);
    // The menu of the Atlas workspace goes back to the world.
    tap(&mut h, "Workspace");
    tap(&mut h, "World");
    assert_eq!(h.state().workspace, Workspace::Globe);
    assert!(control(&h, "Workspace").is_some());
}

#[test]
fn the_top_row_and_the_panels_match_the_split_view_boards() {
    for size in [BOARDS[2], BOARDS[3]] {
        let mut h = pad(size);
        let r = h.state().pad.rects.unwrap();
        assert_eq!(r.class, WidthClass::Compact);
        assert_eq!(r.view, None);
        assert!(control(&h, "Globe").is_none());
        assert_eq!(r.workspace.min, pos2(16.0, 32.0));
        assert_eq!(r.actions.right(), size.0 - 16.0);
        assert!(r.workspace.right() < r.actions.left());
        assert_eq!(r.readout.unwrap().left_bottom(), pos2(16.0, 808.0));

        // The menu holds the views.
        tap(&mut h, "Workspace");
        let menu = h.state().pad.menu_rect.unwrap();
        assert_eq!(
            menu,
            Rect::from_min_size(pos2(16.0, 88.0), vec2(240.0, 334.0))
        );
        let tops = ["World", "Globe", "Atlas"].map(|name| control(&h, name).unwrap().top());
        assert_eq!(tops, [98.0, 146.0, 303.0]);
        tap(&mut h, "Flat");
        assert!(!h.state().pad.menu);
        assert_eq!(h.state().globe.world_view, WorldView::Flat);
        // In the flat view, the menu opens the list of the projections.
        tap(&mut h, "Workspace");
        tap(&mut h, "Projection");
        assert!(!h.state().pad.menu);
        assert_eq!(h.state().pad.projections_rect.unwrap().min, menu.min);
        tap(&mut h, "Mercator");
        let kind = h.state().globe.flat.spec().kind;
        assert_eq!(kind, vale_sphere::ProjectionKind::Mercator);
        tap(&mut h, "Workspace");
        tap(&mut h, "Globe");
        assert!(!h.state().pad.menu);
        assert_eq!(h.state().globe.world_view, WorldView::Globe);

        // A panel is a sheet at the bottom, and one sheet shows at a time.
        tap(&mut h, "Layers");
        let sheet = Rect::from_min_size(pos2(0.0, 434.0), vec2(size.0, 400.0));
        assert_eq!(panel(&h, Panel::Layers), Some(sheet));
        assert_eq!(h.state().pad.rects.unwrap().readout, None);
        tap(&mut h, "Toolbox");
        assert_eq!(h.state().pad.panels, vec![(Panel::Toolbox, sheet)]);
    }
}

#[test]
fn you_pull_the_sheet_down_to_its_title_row() {
    let mut h = painted_pad(BOARDS[3]);
    tap(&mut h, "Layers");
    assert!(control(&h, "Open Height").is_some());
    let bar = pos2(150.0, 460.0);
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
    assert_eq!(panel(&h, Panel::Layers).unwrap().top(), 434.0);
    let bar = pos2(150.0, 460.0);
    drag(&mut h, bar, bar + vec2(0.0, 60.0), None);
    assert_eq!(panel(&h, Panel::Layers).unwrap().top(), 434.0);

    tap(&mut h, "Close Layers");
    assert!(h.state().pad.panels.is_empty());
}

#[test]
fn the_layout_follows_the_turn_of_the_ipad() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Layers");
    tap(&mut h, "Brush settings");
    for size in [BOARDS[1], BOARDS[3], BOARDS[0]] {
        h.set_size(size.into());
        h.run_steps(3);
        let screen = Rect::from_min_size(Pos2::ZERO, size.into());
        assert_eq!(h.state().globe.rect, screen);
        check_controls(&h, &format!("turned to {size:?}"));
    }
    // The narrow window has room for one panel.
    assert!(h.state().pad.is_open(Panel::Brush));
    assert!(!h.state().pad.is_open(Panel::Layers));
}

#[test]
fn a_short_window_makes_the_sliders_short_and_then_leaves_them_out() {
    // 1024 by 768 is an iPad in landscape. The sliders fit with less height.
    let h = pad((1024.0, 768.0));
    assert_eq!(
        h.state().pad.rects.unwrap().brush_card,
        BrushCard::Full(122.0)
    );
    check_controls(&h, "768 high");

    // The smallest iPad in landscape. The strip of six cells leaves room for
    // the sliders.
    let mut h = pad((1133.0, 744.0));
    let rects = h.state().pad.rects.unwrap();
    assert_eq!(rects.brush_card, BrushCard::Full(110.0));
    assert_eq!(rects.tool_cells, 6);
    tap(&mut h, MORE);
    check_controls(&h, "744 high with the More menu");

    let mut h = pad((1024.0, 700.0));
    assert_eq!(h.state().pad.rects.unwrap().brush_card, BrushCard::Button);
    assert!(control(&h, "Size").is_none() && control(&h, "Flow").is_none());
    check_controls(&h, "700 high");
    // The Brush panel holds the two sliders.
    tap(&mut h, "Brush settings");
    assert!(control(&h, "Brush size").is_some() && control(&h, "Brush flow").is_some());
    check_controls(&h, "700 high with the Brush panel");
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
            state.pad.open(*panel);
        }
        state.pad.menu = menu;
        if name == "more" {
            state.pad.more = true;
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
        let canvas = image.get_pixel(size.0 as u32 - 3, 300);
        assert_eq!(canvas.0, [15, 17, 19, 255], "{name}");
        assert_ne!(card.0, canvas.0, "{name}");
        assert_eq!(state.pad.more_rect.is_some(), name == "more", "{name}");
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
            seed: 0,
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
        tap_radius(&mut h, 1737.4);
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
            seed: 0,
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

/// A tap on the radius slider of the open form at the place of this radius.
fn tap_radius(h: &mut Pad, km: f64) {
    let slider = control(h, RADIUS_SLIDER).unwrap();
    let (left, width) = (
        slider.left() + SLIDER_INSET,
        slider.width() - 2.0 * SLIDER_INSET,
    );
    let pos = pos2(left + radius_fraction(km) as f32 * width, slider.center().y);
    touch(h, egui::TouchPhase::Start, pos, None);
    h.step();
    touch(h, egui::TouchPhase::End, pos, None);
    h.run_steps(3);
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
            slider.left() + SLIDER_INSET,
            slider.width() - 2.0 * SLIDER_INSET,
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
            at(12_000.0),
            pos2(slider.right(), slider.center().y),
            None,
        );
        assert_eq!(radius(&h), 20_000.0);
        tap_radius(&mut h, 1737.4);
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
            let free = !a.rect.shrink(0.5).intersects(b.rect.shrink(0.5));
            assert!(free, "{what}: {name} and {} overlap", b.name);
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

    tap_radius(&mut h, 3389.5);
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
    tap(h, "Layers");
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
        let from = pos2(card.left() + 6.0, card.center().y);
        let reach = from.y - card.top() - 60.0;
        let dy = (card.center().y - at).clamp(-reach, reach);
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

    // The weight goes in steps of a quarter point up to 8 points.
    let slider = control(&h, LINE_WEIGHT).unwrap();
    drag(
        &mut h,
        slider.left_center(),
        slider.right_center() + vec2(40.0, 0.0),
        None,
    );
    assert_eq!(stop(&h).line.weight, 8.0);
    let slider = control(&h, LINE_WEIGHT).unwrap();
    drag(
        &mut h,
        slider.right_center(),
        slider.left_center() - vec2(40.0, 0.0),
        None,
    );
    assert_eq!(stop(&h).line.weight, 0.25);

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
    let code = vale_app::ui::pad::widgets::hex_text(h.state().globe.map.bands.colors()[sea]);
    long_press(&mut h, &hex);
    assert!(no_keyboard(&h));
    assert!(control(&h, COPY).is_some());
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
    finger_tap(&mut h, pos2(panel.left() + 6.0, panel.top() + 80.0));
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
    h.state_mut().run_action(Action::ImportRamp(height, path));
    h.run();
    let before = h.state().globe.map.bands.clone();
    let count = before.colors().len();
    assert!(count > colors.len());

    // A fit keeps the stops, and each band takes the color at its middle
    // along the ramp.
    h.state_mut()
        .apply_ramp(height, 0, true, Part::All)
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
        .apply_ramp(height, 0, false, Part::All)
        .unwrap();
    assert_eq!(h.state().globe.map.bands.colors(), colors);
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
            .contains(&Action::ImportRampDialog(height))
    );
    h.state_mut().actions.clear();

    let colors = [[33, 113, 181], [116, 196, 118], [140, 81, 10]];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legend.png");
    let legend = image::RgbImage::from_fn(90, 12, |x, _| image::Rgb(colors[x as usize / 30]));
    legend.save(&path).unwrap();
    let before = h.state().globe.map.bands.clone();
    h.state_mut().run_action(Action::ImportRamp(height, path));
    h.run();
    // The import changes no layer.
    assert_eq!(h.state().globe.map.bands, before);
    assert_eq!(h.state().ramps.list().len(), 1);
    assert!(h.query_by_label("Added legend (3 colors)").is_some());

    // The layer has more bands than the ramp has colors, so the card offers
    // two ways.
    let bands = before.stops().len() - 1;
    tap(&mut h, "legend");
    assert!(h.state().pad.style.ramp_card.is_some());
    check_controls(&h, "with the card of a ramp");
    assert!(control(&h, &format!("Fit to {bands} bands")).is_some());
    // A reverse keeps the card open, and a second reverse puts back the
    // order.
    tap(&mut h, REVERSE_RAMP);
    let reversed: Vec<_> = colors.iter().rev().copied().collect();
    assert_eq!(h.state().ramps.list()[0].colors, reversed);
    assert!(h.state().pad.style.ramp_card.is_some());
    tap(&mut h, REVERSE_RAMP);
    assert_eq!(h.state().ramps.list()[0].colors, colors);
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

    tap(&mut h, "legend");
    tap(&mut h, DELETE_RAMP);
    assert!(h.state().ramps.list().is_empty());
    assert_eq!(h.state().globe.map.bands.bands().len(), 3);

    // A ramp with no blue colors changes only the land.
    let land = [[116, 196, 118], [140, 81, 10], [246, 244, 240]];
    let path = dir.path().join("land.png");
    let legend = image::RgbImage::from_fn(90, 12, |x, _| image::Rgb(land[x as usize / 30]));
    legend.save(&path).unwrap();
    h.state_mut().run_action(Action::ImportRamp(height, path));
    h.run();
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
    h.state_mut().run_action(Action::ImportRamp(height, path));
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
        tap(&mut h, "Layers");
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
fn the_line_cell_takes_the_place_of_more_for_a_line_layer() {
    let mut h = pad(BOARDS[0]);
    let more = control(&h, MORE).unwrap();
    let rivers = vector(&h, "Rivers");
    h.state_mut().select(Some(LayerRef::Vector(rivers)));
    h.run_steps(2);
    assert_eq!(control(&h, LINE), Some(more));
    for name in ["Raise", "Lower", "Smooth", "Flatten", MORE, "Size", "Flow"] {
        assert!(control(&h, name).is_none(), "{name}");
    }
    assert!(control(&h, SMOOTHING).is_some());
    assert!(!h.state().globe.lining());
    tap(&mut h, LINE);
    assert_eq!(h.state().globe.tool, Tool::Line);
    assert!(h.state().globe.lining());
    // A finger turns the globe and draws no line.
    let center = h.state().globe.rect.center();
    let before = line_features(&h, rivers);
    drag(&mut h, center, center + vec2(120.0, 0.0), None);
    assert_eq!(line_features(&h, rivers), before);
    // A layer of points has no line tool.
    let places = vector(&h, "Places");
    h.state_mut().select(Some(LayerRef::Vector(places)));
    h.run_steps(2);
    assert!(control(&h, LINE).is_none());
    assert!(!h.state().globe.lining());
}

#[test]
fn a_line_stroke_is_one_feature_that_undo_removes_and_a_stroke_from_its_end_extends_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = saved_pad(BOARDS[0], dir.path());
    let rivers = vector(&h, "Rivers");
    h.state_mut().select(Some(LayerRef::Vector(rivers)));
    h.run_steps(2);
    tap(&mut h, LINE);
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
fn the_polygon_cell_takes_the_place_of_more_for_a_polygon_layer() {
    let mut h = pad(BOARDS[0]);
    let more = control(&h, MORE).unwrap();
    let areas = h
        .state_mut()
        .doc
        .add_layer(vale_store::GeometryKind::Polygon);
    h.state_mut().select(Some(LayerRef::Vector(areas)));
    h.run_steps(2);
    assert_eq!(control(&h, POLYGON), Some(more));
    assert!(control(&h, LINE).is_none());
    assert!(control(&h, SMOOTHING).is_some());
    assert!(!h.state().globe.filling());
    tap(&mut h, POLYGON);
    assert_eq!(h.state().globe.tool, Tool::Polygon);
    assert!(h.state().globe.filling());
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
    tap(&mut h, POLYGON);
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
        let image = h.render().unwrap();
        std::fs::create_dir_all("../../target/app").unwrap();
        image
            .save(format!(
                "../../target/app/test-pad-smoothing-{}.png",
                size.0
            ))
            .unwrap();

        // A tap at the left end of the slider gives a size of zero.
        let slider = control(&h, FEATURE_SIZE).unwrap();
        finger_tap(&mut h, pos2(slider.left() + 1.0, slider.center().y));
        assert_eq!(smoothing(&h).feature_size, 0.0, "{what}");

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
        let image = h.render().unwrap();
        std::fs::create_dir_all("../../target/app").unwrap();
        image
            .save(format!(
                "../../target/app/test-pad-hillshade-{}.png",
                size.0
            ))
            .unwrap();

        // A tap at the left end of the opacity slider gives zero.
        scroll_to_row(&mut h, HILLSHADE_OPACITY);
        let slider = control(&h, HILLSHADE_OPACITY).unwrap();
        finger_tap(&mut h, pos2(slider.left() + 1.0, slider.center().y));
        assert_eq!(hillshade_of(&h, height).opacity, 0.0, "{what}");
        let slider = control(&h, HILLSHADE_OPACITY).unwrap();
        finger_tap(&mut h, slider.center());
        let opacity = hillshade_of(&h, height).opacity;
        assert!(opacity > 0.0, "{what}: {opacity}");

        // The highlights start on, and a tap turns them off.
        assert!(hillshade_of(&h, height).highlights, "{what}");
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
        // The track of a slider with a short label starts 72 points right
        // of the row, and ends 56 points left of its end, inside the half
        // of the thumb.
        let ends = |h: &Pad, name: &str| {
            let slider = control(h, name).unwrap();
            let y = slider.center().y;
            let left = pos2(slider.left() + 1.0, y);
            let right = pos2(slider.right() - 1.0, y);
            let middle = (slider.left() + 72.0 + slider.right() - 56.0) / 2.0;
            (left, pos2(middle, y), right)
        };
        scroll_to(&mut h, HILLSHADE_RELIEF);
        let (left, _, right) = ends(&h, HILLSHADE_ALTITUDE);
        finger_tap(&mut h, left);
        assert_eq!(hillshade_of(&h, height).altitude, 5.0, "{what}");
        finger_tap(&mut h, right);
        assert_eq!(hillshade_of(&h, height).altitude, 85.0, "{what}");

        let (left, middle, _) = ends(&h, HILLSHADE_RELIEF);
        finger_tap(&mut h, left);
        assert_eq!(hillshade_of(&h, height).exaggeration, 0.25, "{what}");
        finger_tap(&mut h, middle);
        assert_eq!(hillshade_of(&h, height).exaggeration, 1.0, "{what}");
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

/// Scrolls the panel of a layer until a control is inside the card, with a
/// drag up on the margin at the left of the rows.
fn scroll_to(h: &mut Pad, name: &str) {
    for _ in 0..8 {
        let card = panel(h, Panel::Layers).unwrap();
        let inside = |rect: Rect| card.shrink2(vec2(0.0, 40.0)).contains_rect(rect);
        if control(h, name).is_some_and(inside) {
            return;
        }
        let from = pos2(card.left() + 6.0, card.bottom() - 40.0);
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
    tap(&mut h, LINE);
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
        let image = h.render().unwrap();
        std::fs::create_dir_all("../../target/app").unwrap();
        let path = format!("../../target/app/test-pad-snap-{}.png", size.0);
        image.save(path).unwrap();
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
        let slider = control(&h, SNAP_DISTANCE).unwrap();
        let y = slider.center().y;
        finger_tap(&mut h, pos2(slider.left() + 1.0, y));
        assert_eq!(h.state().globe.snap.distance, 4.0, "{what}");
        drag(
            &mut h,
            pos2(slider.left() + 1.0, y),
            pos2(slider.right() - 1.0, y),
            None,
        );
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
        tap(&mut h, LINE);
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
        let image = h.render().unwrap();
        std::fs::create_dir_all("../../target/app").unwrap();
        image
            .save(format!(
                "../../target/app/test-pad-snap-stops-{}.png",
                size.0
            ))
            .unwrap();
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
    tap(h, vale_app::ui::pad::POINT);
    h.state_mut().globe.snap.on = false;
    id
}

/// The names of the fields of the open point form.
fn form_fields(h: &Pad) -> Vec<String> {
    let form = h.state().point_form.as_ref().expect("no point form");
    form.fields.iter().map(|f| f.name.clone()).collect()
}

#[test]
fn a_pen_tap_places_a_point_and_opens_a_form_with_each_field_of_the_layer() {
    let mut h = pad(BOARDS[0]);
    let more = control(&h, MORE).unwrap();
    let places = vector(&h, "Places");
    h.state_mut().select(Some(LayerRef::Vector(places)));
    h.run_steps(2);
    assert_eq!(control(&h, vale_app::ui::pad::POINT), Some(more));
    tap(&mut h, vale_app::ui::pad::POINT);
    assert_eq!(h.state().globe.tool, Tool::Point);
    let before = line_features(&h, places);
    let center = h.state().globe.rect.center();
    // A finger turns the globe and places no point.
    drag(&mut h, center, center + vec2(60.0, 0.0), None);
    assert_eq!(line_features(&h, places), before);
    assert!(h.state().point_form.is_none());

    pen_tap_at(&mut h, center);
    let after = line_features(&h, places);
    assert_eq!(after.len(), before.len() + 1);
    let fields = h.state().doc.project.layer(places).unwrap().field_names();
    assert!(!fields.is_empty());
    assert_eq!(form_fields(&h), fields);
    let form = h.state().point_form.as_ref().unwrap();
    assert_eq!(form.feature, after.last().unwrap().uuid());
    // The fields that do not fit scroll.
    assert!(control(&h, &fields[0]).is_some());
    // Another tool closes the form.
    tap(&mut h, "Pan");
    assert!(h.state().point_form.is_none());
}

#[test]
fn a_new_point_layer_has_a_name_field_and_return_closes_the_form() {
    let mut h = pad(BOARDS[0]);
    let id = new_point_layer(&mut h);
    let layer = h.state().doc.project.layer(id).unwrap();
    assert_eq!(layer.field_names(), ["name"]);
    assert_eq!(layer.fields[0].kind, vale_store::FieldType::Text);
    // The layer labels its points with the name.
    let entry = h.state().doc.frame.entries.iter().find(|e| e.layer == id);
    let label = &entry.unwrap().style.label;
    assert_eq!(label.field.as_deref(), Some("name"));
    assert!(label.enabled);
    let center = h.state().globe.rect.center();
    pen_tap_at(&mut h, center);
    assert_eq!(form_fields(&h), ["name"]);
    // The field has the focus, so the keyboard types into it at once.
    h.event(egui::Event::Text("Avalon".into()));
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert!(h.state().point_form.is_none());
    let layer = h.state().doc.project.layer(id).unwrap();
    let name = &layer.features[0].attributes["name"];
    assert_eq!(*name, vale_store::Value::Text("Avalon".into()));
}

#[test]
fn a_point_layer_with_no_fields_places_a_point_with_no_form() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = saved_pad(BOARDS[0], dir.path());
    let id = new_point_layer(&mut h);
    h.state_mut().doc.project.remove_field(id, "name");
    for _ in 0..3 {
        step_and_save(&mut h, false);
    }
    let saved = saves(&h);
    let center = h.state().globe.rect.center();
    pen_tap_at(&mut h, center);
    assert!(h.state().point_form.is_none());
    assert_eq!(line_features(&h, id).len(), 1);
    assert!(line_features(&h, id)[0].attributes.is_empty());
    assert_eq!(saves(&h), saved + 1);
    tap(&mut h, "Undo");
    assert!(line_features(&h, id).is_empty());
}

#[test]
fn the_form_shows_each_field_of_the_schema_with_its_default() {
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
    let center = h.state().globe.rect.center();
    pen_tap_at(&mut h, center);
    assert_eq!(form_fields(&h), ["town", "people", "height", "port"]);
    let form = h.state().point_form.as_ref().unwrap();
    let texts: Vec<&str> = form.fields.iter().map(|f| f.text.as_str()).collect();
    assert_eq!(texts, ["Hamlet", "7", "2.5", "false"]);
    // Return goes from field to field. The whole number and the true or
    // false field get new values.
    let typed = [None, Some((1, "12")), None, Some((5, "true"))];
    for edit in typed {
        if let Some((old, new)) = edit {
            for _ in 0..old {
                h.key_press(egui::Key::Backspace);
            }
            h.event(egui::Event::Text(new.into()));
            h.run_steps(2);
        }
        h.key_press(egui::Key::Enter);
        h.run_steps(2);
    }
    assert!(h.state().point_form.is_none());
    let attributes = &line_features(&h, id)[0].attributes;
    assert_eq!(attributes["town"], Value::Text("Hamlet".into()));
    assert_eq!(attributes["people"], Value::Number(12.0));
    assert_eq!(attributes["height"], Value::Number(2.5));
    assert_eq!(attributes["port"], Value::Bool(true));
}

#[test]
fn a_text_that_is_not_a_value_of_the_field_type_gives_null() {
    use vale_store::{Field, FieldType, Value};
    let mut h = pad(BOARDS[0]);
    let id = new_point_layer(&mut h);
    let field = Field::new("people", FieldType::Integer);
    assert!(h.state_mut().doc.project.add_field(id, field));
    let center = h.state().globe.rect.center();
    pen_tap_at(&mut h, center);
    assert_eq!(form_fields(&h), ["name", "people"]);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    h.event(egui::Event::Text("many".into()));
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert!(h.state().point_form.is_none());
    let attributes = &line_features(&h, id)[0].attributes;
    assert_eq!(attributes["name"], Value::Null);
    assert_eq!(attributes["people"], Value::Null);
}

#[test]
fn ten_named_points_in_a_row_are_ten_undo_steps() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = saved_pad(BOARDS[0], dir.path());
    let id = new_point_layer(&mut h);
    let saved = saves(&h);
    let center = h.state().globe.rect.center();
    for i in 0..10 {
        pen_tap_at(&mut h, center + vec2(30.0 * i as f32 - 135.0, 0.0));
        h.event(egui::Event::Text(format!("City {i}")));
        h.run_steps(2);
        // Return closes the form, and so does the tap of the next point.
        if i % 2 == 0 || i == 9 {
            h.key_press(egui::Key::Enter);
        }
        for _ in 0..3 {
            step_and_save(&mut h, false);
        }
    }
    assert!(h.state().point_form.is_none());
    let names: Vec<_> = line_features(&h, id)
        .iter()
        .map(|f| f.attributes["name"].clone())
        .collect();
    let want: Vec<_> = (0..10)
        .map(|i| vale_store::Value::Text(format!("City {i}")))
        .collect();
    assert_eq!(names, want);
    assert_eq!(saves(&h), saved + 10);
    tap(&mut h, "Undo");
    assert_eq!(line_features(&h, id).len(), 9);
}

#[test]
fn the_keyboard_does_not_hide_the_form_or_the_new_point() {
    for size in [BOARDS[0], BOARDS[1]] {
        let what = format!("{size:?}");
        let mut h = pad(size);
        let id = new_point_layer(&mut h);
        let rect = h.state().globe.rect;
        // A point low on the screen is under the keyboard.
        pen_tap_at(&mut h, rect.center() + vec2(0.0, 150.0));
        h.run_steps(2);
        let keyboard = vale_app::ui::point_form::keyboard_top(rect);
        let field = control(&h, "name").unwrap();
        assert!(field.bottom() < keyboard, "{what}");
        let point = match &line_features(&h, id)[0].geometry {
            vale_store::Geometry::Points(points) => points[0],
            _ => panic!("not a point"),
        };
        let dir = vale_app::globe::math::lonlat_to_dir(point[0], point[1]);
        let pos = h.state().globe.project(dir).unwrap();
        assert!(
            pos.y > field.bottom() && pos.y < keyboard,
            "{what}: {pos:?}"
        );
    }
}

#[test]
fn a_new_point_snaps_to_a_vertex() {
    let mut h = pad(BOARDS[0]);
    let places = vector(&h, "Places");
    h.state_mut().select(Some(LayerRef::Vector(places)));
    h.run_steps(2);
    tap(&mut h, vale_app::ui::pad::POINT);
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
    use vale_app::ui::pad::fields::{ADD, ADD_FIELD, DELETE_FIELD, FIELD_NAME, NEW_FIELD};
    use vale_store::FieldType::{Integer, Text};
    let dir = tempfile::tempdir().unwrap();
    let mut h = saved_pad(BOARDS[0], dir.path());
    tap(&mut h, "Layers");
    tap(&mut h, ADD_LAYER);
    tap(&mut h, "Points");
    let id = vector(&h, "Points 1");
    assert_eq!(pad_schema(&h, id), [("name".to_string(), Text)]);
    h.state_mut().save_waiting().unwrap();
    let saved = saves(&h);

    tap(&mut h, ADD_FIELD);
    assert_eq!(h.state().pad.page, Some(Page::NewField));
    assert!(control(&h, &format!("Close {NEW_FIELD}")).is_some());
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
    assert!(control(&h, "Close people").is_some());
    tap(&mut h, "Back to Points 1");

    // Return in the name adds the field.
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
    tap(&mut h, "Layers");
    tap(&mut h, ADD_LAYER);
    tap(&mut h, "Points");
    let id = vector(&h, "Points 1");
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
        let composed = pipeline.compose(doc, (1280.0, 800.0), &mut fonts, Quality::Final, None);
        composed.unwrap().labels.placed
    };
    let mut h = pad(BOARDS[0]);
    let places = vector(&h, "Places");
    let before = placed(&h.state().doc);
    assert!(before > 0);
    let index = h.state().doc.layer(places).unwrap().field_index("name");
    tap(&mut h, "Layers");
    h.state_mut().pad.layer = Some(LayerRef::Vector(places));
    h.state_mut().pad.page = Some(Page::Field(index.unwrap()));
    h.run_steps(2);
    type_text(&mut h, FIELD_NAME, "title");
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    let doc = &h.state().doc;
    assert!(doc.layer(places).unwrap().field("title").is_some());
    let label = &doc.entry(places).unwrap().style.label;
    assert_eq!(label.field.as_deref(), Some("title"));
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
        tap(&mut h, "Layers");
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
        let from = pos2(card.left() + 6.0, card.top() + 60.0);
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

/// The rules of the style of a vector layer.
fn rules(h: &Pad, id: vale_store::LayerId) -> Vec<vale_style::Rule> {
    h.state().doc.entry(id).unwrap().style.style.rules.clone()
}

fn rule_names(h: &Pad, id: vale_store::LayerId) -> Vec<String> {
    rules(h, id).into_iter().map(|r| r.name).collect()
}

/// Opens the panel of a vector layer and the page of its rule `rule`.
fn open_rule(h: &mut Pad, layer: &str, rule: &str) {
    tap(h, "Layers");
    tap(h, &format!("Open {layer}"));
    scroll_to_control(h, rule);
    tap(h, rule);
    assert!(matches!(h.state().pad.page, Some(Page::Rule(_))));
}

#[test]
fn the_style_section_adds_moves_and_deletes_rules() {
    use vale_app::ui::pad::rules::{ADD_RULE, DELETE_RULE, MOVE_UP};
    for size in [BOARDS[0], BOARDS[3]] {
        let what = format!("{size:?}");
        let dir = tempfile::tempdir().unwrap();
        let mut h = saved_pad(size, dir.path());
        let rivers = vector(&h, "Rivers");
        tap(&mut h, "Layers");
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
    use vale_app::ui::pad::filter::{ADD_CONDITION, NO_CONDITION, REMOVE_CONDITION, VALUE};
    use vale_app::ui::pad::rules::{BAD_FILTER, BAD_RANGE, EXPRESSION, MAX_SCALE, MIN_SCALE};
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
        // A condition on a number: a field chip, an operator chip, and the
        // keypad.
        scroll_to_control(&mut h, ADD_CONDITION);
        tap(&mut h, ADD_CONDITION);
        assert_eq!(h.state().pad.page, Some(Page::Condition(0)), "{what}");
        check_controls(&h, &format!("{what} on the page of a new condition"));
        scroll_to_control(&mut h, "scalerank");
        tap(&mut h, "scalerank");
        scroll_to_control(&mut h, "<");
        tap(&mut h, "<");
        type_scrolled_keys(&mut h, VALUE, "5");
        check_controls(&h, &format!("{what} with the keypad of a value"));
        scroll_to_control(&mut h, ADD_CONDITION);
        tap(&mut h, ADD_CONDITION);
        assert_eq!(h.state().pad.page, Some(Page::Rule(0)), "{what}");
        assert_eq!(filter(&h).as_deref(), Some("scalerank < 5"), "{what}");

        // A condition on a text: a chip of a common value.
        scroll_to_control(&mut h, ADD_CONDITION);
        tap(&mut h, ADD_CONDITION);
        scroll_to_control(&mut h, "featurecla");
        tap(&mut h, "featurecla");
        scroll_to_control(&mut h, "River");
        tap(&mut h, "River");
        scroll_to_control(&mut h, ADD_CONDITION);
        tap(&mut h, ADD_CONDITION);
        let text = "scalerank < 5 and featurecla = 'River'";
        assert_eq!(filter(&h).as_deref(), Some(text), "{what}");
        assert!(rule(&h).filter.error().is_none(), "{what}");

        // The button of a condition removes it.
        let first = format!("{REMOVE_CONDITION} 1");
        scroll_to_control(&mut h, &first);
        tap(&mut h, &first);
        let text = "featurecla = 'River'";
        assert_eq!(filter(&h).as_deref(), Some(text), "{what}");

        // The expression field takes any filter, and warns about a wrong one.
        type_text(&mut h, EXPRESSION, "scalerank < 5 and");
        assert!(rule(&h).filter.error().is_some(), "{what}");
        assert!(h.query_by_label(BAD_FILTER).is_some(), "{what}");
        type_text(&mut h, EXPRESSION, "scalerank < 5");
        assert!(h.query_by_label(BAD_FILTER).is_none(), "{what}");

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

        // A tap on the width slider sets a width in twentieths of a point.
        scroll_to_control(&mut h, STROKE_WIDTH);
        let slider = control(&h, STROKE_WIDTH).unwrap();
        let at = pos2(slider.right() - 30.0, slider.center().y);
        touch(&mut h, egui::TouchPhase::Start, at, None);
        h.step();
        touch(&mut h, egui::TouchPhase::End, at, None);
        h.run_steps(3);
        let width = rule(&h).stroke().unwrap().width;
        assert!(width > stroke.width, "{what}: {width}");
        assert_eq!((width * 20.0).round() / 20.0, width, "{what}");

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
fn a_width_drag_and_a_color_drag_redraw_and_are_one_undo_step_each() {
    use vale_app::ui::pad::rules::{STROKE_COLOR, STROKE_WIDTH};
    let size = BOARDS[1];
    let dir = tempfile::tempdir().unwrap();
    let mut h = saved_pad(size, dir.path());
    let rivers = vector(&h, "Rivers");
    let stroke = |h: &Pad| rules(h, rivers)[0].stroke().unwrap();
    let before = stroke(&h);
    open_rule(&mut h, "Rivers", "Default");
    scroll_to_control(&mut h, STROKE_WIDTH);
    save_frames(&mut h);
    let saved = saves(&h);

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
    let rect = control(&h, STROKE_WIDTH).unwrap();
    let widths = held(&mut h, STROKE_WIDTH, 30.0, rect.width() - 20.0, &|h| {
        stroke(h).width
    });
    // The press sets the width at the left of the track.
    assert!(widths.len() >= 5, "{widths:?}");
    assert!(widths[1..].is_sorted(), "{widths:?}");
    assert_eq!(saves(&h), saved + 1);

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
    assert_eq!(stroke(&h).width, *widths.last().unwrap());
    tap(&mut h, "Undo");
    assert_eq!(stroke(&h), before);
}

#[test]
fn a_rule_page_sets_the_line_type_the_dashes_and_the_taper() {
    use vale_app::ui::pad::rules::{
        BRUSH, DASH, DASHED, GAP, LINE, SOLID, STROKE_WIDTH, TAPER, TAPER_START,
    };
    use vale_style::StrokeKind;
    // A tap near the right end of a slider.
    let touch_right = |h: &mut Pad, name: &str| {
        scroll_to_control(h, name);
        let slider = control(h, name).unwrap();
        let at = pos2(slider.right() - 30.0, slider.center().y);
        touch(h, egui::TouchPhase::Start, at, None);
        h.step();
        touch(h, egui::TouchPhase::End, at, None);
        h.run_steps(3);
    };
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
        touch_right(&mut h, DASH);
        touch_right(&mut h, GAP);
        let StrokeKind::Dashed { dash, gap } = kind(&h) else {
            panic!("{what}: {:?}", kind(&h));
        };
        assert!(
            dash > StrokeKind::DASH && gap > StrokeKind::GAP,
            "{what}: {dash} {gap}"
        );
        assert!(dash <= 20.0 && gap <= 20.0, "{what}: {dash} {gap}");
        assert_eq!((dash * 20.0).round() / 20.0, dash, "{what}");
        assert_eq!((gap * 20.0).round() / 20.0, gap, "{what}");

        scroll_to_control(&mut h, TAPER);
        tap(&mut h, TAPER);
        assert_eq!(kind(&h), StrokeKind::taper(), "{what}");
        assert!(control(&h, DASH).is_none(), "{what}");
        touch_right(&mut h, TAPER_START);
        let StrokeKind::Taper { start } = kind(&h) else {
            panic!("{what}: {:?}", kind(&h));
        };
        assert!(
            start > StrokeKind::TAPER_START && start <= 1.0,
            "{what}: {start}"
        );
        check_controls(&h, &format!("{what} with a taper"));
        h.render()
            .unwrap()
            .save(format!("../../target/app/test-pad-line-{}.png", size.0))
            .unwrap();

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
    scroll_to_control(&mut h, STROKE_WIDTH);
    assert!(control(&h, STROKE_WIDTH).is_some());
    assert!(control(&h, LINE).is_none());
}

/// A tap near the right end of a slider.
fn touch_right(h: &mut Pad, name: &str) {
    scroll_to_control(h, name);
    let slider = control(h, name).unwrap();
    let at = pos2(slider.right() - 30.0, slider.center().y);
    touch(h, egui::TouchPhase::Start, at, None);
    h.step();
    touch(h, egui::TouchPhase::End, at, None);
    h.run_steps(3);
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
fn a_rule_page_sets_a_point_symbol() {
    use vale_app::ui::pad::rules::{
        CIRCLE, FILL, NO_FIELD, ROTATION, SIZE, SIZE_FIELD, STROKE, SVG_SYMBOL, SYMBOL, TINT,
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
        h.render()
            .unwrap()
            .save(format!("../../target/app/test-pad-symbols-{}.png", size.0))
            .unwrap();
        tap(&mut h, "castle");
        assert_eq!(h.state().pad.page, Some(Page::Rule(0)), "{what}");
        assert_eq!(symbol(&h).unwrap().symbol, "castle", "{what}");

        touch_right(&mut h, SIZE);
        let s = symbol(&h).unwrap();
        let points = s.size.fallback;
        assert!(points > 12.0 && points <= 64.0, "{what}: {points}");
        assert_eq!(points.round(), points, "{what}");

        // A tap at the right of the dial turns the symbol to the east.
        scroll_to_control(&mut h, ROTATION);
        let dial = control(&h, ROTATION).unwrap();
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
        h.render()
            .unwrap()
            .save(format!("../../target/app/test-pad-symbol-{}.png", size.0))
            .unwrap();

        scroll_to_control(&mut h, CIRCLE);
        tap(&mut h, CIRCLE);
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
        h.render()
            .unwrap()
            .save(format!(
                "../../target/app/test-pad-symbols-empty-{}.png",
                size.0
            ))
            .unwrap();
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
fn a_circle_size_drag_is_one_undo_step() {
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
    scroll_to_control(&mut h, SIZE);
    save_frames(&mut h);
    let saved = saves(&h);
    let rect = control(&h, SIZE).unwrap();

    let y = rect.center().y;
    let (from, to) = (pos2(rect.left() + 100.0, y), pos2(rect.right() - 20.0, y));
    touch(&mut h, egui::TouchPhase::Start, from, None);
    step_and_save(&mut h, true);
    let mut values = Vec::new();
    for i in 1..=6 {
        touch(
            &mut h,
            egui::TouchPhase::Move,
            from.lerp(to, i as f32 / 6.0),
            None,
        );
        step_and_save(&mut h, true);
        values.extend(radius(&h));
    }
    touch(&mut h, egui::TouchPhase::End, to, None);
    save_frames(&mut h);
    values.dedup();
    assert!(values.len() >= 4, "{values:?}");
    assert!(values.is_sorted(), "{values:?}");
    // The slider sets the diameter in half points.
    let last = *values.last().unwrap();
    assert!(last > before && 2.0 * last <= CIRCLE_SIZES.1, "{values:?}");
    assert_eq!((4.0 * last).round(), 4.0 * last, "{values:?}");
    assert_eq!(saves(&h), saved + 1);

    tap(&mut h, "Undo");
    assert_eq!(radius(&h), Some(before));
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

        touch_right(&mut h, SPACING);
        let s = scatter(&h).unwrap();
        let spacing = s.spacing.fallback;
        assert!(spacing > 10.0 && spacing <= 100.0, "{what}: {spacing}");
        assert!(spacing >= s.size.fallback, "{what}");

        // A larger size pushes the spacing up with it.
        touch_right(&mut h, SIZE);
        let s = scatter(&h).unwrap();
        assert!(s.size.fallback > 8.0 && s.size.fallback <= 48.0, "{what}");
        assert!(s.spacing.fallback >= s.size.fallback, "{what}");

        touch_right(&mut h, SIZE_VARIATION);
        touch_right(&mut h, ROTATION_VARIATION);
        let s = scatter(&h).unwrap();
        assert!(s.size_jitter > 0.2 && s.size_jitter <= 0.5, "{what}");
        assert!(
            s.rotation_jitter > 0.0 && s.rotation_jitter <= 180.0,
            "{what}"
        );
        scroll_to_control(&mut h, TINT);
        check_controls(&h, &what);
        h.render()
            .unwrap()
            .save(format!("../../target/app/test-pad-scatter-{}.png", size.0))
            .unwrap();

        scroll_to_control(&mut h, SCATTER);
        tap(&mut h, SCATTER);
        assert!(scatter(&h).is_none(), "{what}");
        assert!(rules(&h, land)[0].fill().is_some(), "{what}");
    }
}

#[test]
fn a_spacing_drag_is_one_undo_step() {
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
    scroll_to_control(&mut h, SPACING);
    save_frames(&mut h);
    let saved = saves(&h);
    let rect = control(&h, SPACING).unwrap();

    let y = rect.center().y;
    let (from, to) = (pos2(rect.left() + 100.0, y), pos2(rect.right() - 60.0, y));
    touch(&mut h, egui::TouchPhase::Start, from, None);
    step_and_save(&mut h, true);
    let mut values = Vec::new();
    for i in 1..=6 {
        touch(
            &mut h,
            egui::TouchPhase::Move,
            from.lerp(to, i as f32 / 6.0),
            None,
        );
        step_and_save(&mut h, true);
        values.extend(spacing(&h));
    }
    touch(&mut h, egui::TouchPhase::End, to, None);
    save_frames(&mut h);
    values.dedup();
    assert!(values.len() >= 4, "{values:?}");
    assert!(values.is_sorted(), "{values:?}");
    assert_eq!(saves(&h), saved + 1);

    // One undo takes back the whole drag, and the next takes back the scatter.
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
    let (from, to) = (pos2(rect.right() - 20.0, y), pos2(rect.left() + 20.0, y));
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
    assert!(starts.iter().rev().is_sorted(), "{starts:?}");
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
            tap(&mut h, "Layers");
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
        tap(&mut h, "Layers");
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
    tap(&mut h, "Layers");
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
    tap(&mut h, "Layers");
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
        tap(&mut h, "Layers");
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
