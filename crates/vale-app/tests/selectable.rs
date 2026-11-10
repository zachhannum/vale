//! The selectable layers of the Select tool in the World workspace.

use eframe::egui::{self, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use vale_app::document::Document;
use vale_app::globe::Tool;
use vale_app::globe::select::FeatureRef;
use vale_app::session::Session;
use vale_app::ui::pad::{LayerRef, Panel};
use vale_app::ui::select::Selectable;
use vale_app::ui::{AppState, Layout, draw};
use vale_store::{Feature, Geometry, GeometryKind, LayerId};

type Pad = Harness<'static, AppState>;

const LANDSCAPE: (f32, f32) = (1194.0, 834.0);

fn harness() -> Pad {
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    state.set_layout(Layout::Pad);
    let mut h = Harness::builder()
        .with_size(vec2(LANDSCAPE.0, LANDSCAPE.1))
        .with_pixels_per_point(1.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
    h.run_steps(3);
    h
}

/// Hides the layers of the sample, and adds two point layers, Towns and
/// Wells, each with one point at 0°, 0°. Wells is above Towns, and Towns
/// is the active layer. The Select tool is the tool, and 0°, 0° is near the
/// center of the globe. Returns the two points and the place of 0°, 0°.
fn two_layers(h: &mut Pad) -> (FeatureRef, FeatureRef, Pos2) {
    let state = h.state_mut();
    let shown: Vec<_> = state.doc.project.layers().iter().map(|l| l.id).collect();
    for id in shown {
        state.doc.project.set_layer_visible(id, false);
    }
    let mut add = |name: &str| {
        let id = state.doc.add_layer(GeometryKind::Point);
        state.doc.rename_layer(id, name);
        let feature = Feature::new(Geometry::Points(vec![[0.0, 0.0]]), Default::default());
        let found = FeatureRef {
            layer: id,
            uuid: feature.uuid(),
        };
        assert!(state.doc.project.add_feature(id, feature));
        found
    };
    let towns = add("Towns");
    let wells = add("Wells");
    state.select(Some(LayerRef::Vector(towns.layer)));
    state.globe.tool = Tool::Select;
    h.run_steps(2);
    let dir = vale_app::globe::math::lonlat_to_dir(0.0, 0.0);
    let at = h.state().globe.project(dir).expect("0°, 0° does not show");
    let center = h.state().globe.rect.center() - vec2(0.0, 80.0);
    h.state_mut().globe.move_place(at, center);
    h.run_steps(2);
    let at = h.state().globe.project(dir).unwrap();
    (towns, wells, at)
}

/// Adds a point at 0°, 0° to `layer`.
fn add_point(h: &mut Pad, layer: LayerId) -> FeatureRef {
    let feature = Feature::new(Geometry::Points(vec![[0.0, 0.0]]), Default::default());
    let found = FeatureRef {
        layer,
        uuid: feature.uuid(),
    };
    assert!(h.state_mut().doc.project.add_feature(layer, feature));
    h.run_steps(2);
    found
}

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

fn tap_at(h: &mut Pad, pos: Pos2) {
    touch(h, egui::TouchPhase::Start, pos, None);
    h.step();
    touch(h, egui::TouchPhase::End, pos, None);
    h.run_steps(3);
}

/// A drag of the pen from `a` to `b`.
fn pen_box(h: &mut Pad, a: Pos2, b: Pos2) {
    touch(h, egui::TouchPhase::Start, a, Some(0.5));
    h.step();
    for i in 1..=8 {
        let p = a + (b - a) * (i as f32 / 8.0);
        touch(h, egui::TouchPhase::Move, p, Some(0.5));
        h.step();
    }
    touch(h, egui::TouchPhase::End, b, Some(0.5));
    h.run_steps(3);
}

fn sorted(mut v: Vec<FeatureRef>) -> Vec<FeatureRef> {
    v.sort_by_key(|f| (f.layer, f.uuid));
    v
}

#[test]
fn by_default_the_select_tool_selects_in_the_active_layer() {
    let mut h = harness();
    let (towns, wells, at) = two_layers(&mut h);
    assert_eq!(h.state().selectable, Selectable::Active);
    assert_eq!(h.state().selectable_layers(), vec![towns.layer]);
    tap_at(&mut h, at);
    assert_eq!(h.state().feature_selection, vec![towns]);
    h.state_mut().select(Some(LayerRef::Vector(wells.layer)));
    h.run_steps(2);
    tap_at(&mut h, at);
    assert_eq!(h.state().feature_selection, vec![wells]);
}

#[test]
fn with_checked_layers_a_tap_and_a_box_select_in_each_checked_layer() {
    let mut h = harness();
    let (towns, wells, at) = two_layers(&mut h);
    h.state_mut().toggle_selectable(towns.layer);
    h.state_mut().toggle_selectable(wells.layer);
    assert_eq!(
        h.state().selectable_layers(),
        vec![wells.layer, towns.layer]
    );
    // The top layer wins a tie.
    tap_at(&mut h, at);
    assert_eq!(h.state().feature_selection, vec![wells]);
    pen_box(&mut h, at - vec2(30.0, 30.0), at + vec2(30.0, 30.0));
    assert_eq!(
        sorted(h.state().feature_selection.clone()),
        sorted(vec![towns, wells])
    );
}

#[test]
fn a_hidden_layer_and_a_layer_with_no_check_have_no_hit() {
    let mut h = harness();
    let (towns, wells, at) = two_layers(&mut h);
    h.state_mut().select_all_layers();
    h.state_mut()
        .doc
        .project
        .set_layer_visible(wells.layer, false);
    h.run_steps(2);
    tap_at(&mut h, at);
    assert_eq!(h.state().feature_selection, vec![towns]);
    h.state_mut().toggle_selectable(towns.layer);
    tap_at(&mut h, at);
    assert!(h.state().feature_selection.is_empty());
    h.state_mut().deselect_all_layers();
    assert!(h.state().selectable_layers().is_empty());
    pen_box(&mut h, at - vec2(30.0, 30.0), at + vec2(30.0, 30.0));
    assert!(h.state().feature_selection.is_empty());
}

#[test]
fn a_selection_in_one_layer_makes_that_layer_the_active_layer() {
    let mut h = harness();
    let (towns, wells, at) = two_layers(&mut h);
    h.state_mut().toggle_selectable(wells.layer);
    tap_at(&mut h, at);
    assert_eq!(h.state().feature_selection, vec![wells]);
    assert_eq!(h.state().selected_vector(), Some(wells.layer));
    // A selection in two layers leaves the active layer.
    h.state_mut().select_all_layers();
    pen_box(&mut h, at - vec2(30.0, 30.0), at + vec2(30.0, 30.0));
    assert_eq!(h.state().feature_selection.len(), 2);
    assert_eq!(h.state().selected_vector(), Some(wells.layer));
    let _ = towns;
}

#[test]
fn the_inspector_shows_the_count_of_each_layer_of_a_mixed_selection() {
    let mut h = harness();
    // The open pane moves the globe, so it opens first.
    h.state_mut().pad.open(Panel::Attributes);
    let (towns, wells, at) = two_layers(&mut h);
    add_point(&mut h, towns.layer);
    h.state_mut().select_all_layers();
    pen_box(&mut h, at - vec2(30.0, 30.0), at + vec2(30.0, 30.0));
    assert_eq!(h.state().feature_selection.len(), 3);
    assert_eq!(
        h.state().mixed_lines(),
        vec!["Wells: 1 feature", "Towns: 2 features"]
    );
    h.run_steps(2);
    assert!(h.query_by_label("Wells: 1 feature").is_some());
    assert!(h.query_by_label("Towns: 2 features").is_some());
    let _ = wells;
}

#[test]
fn delete_selection_deletes_a_mixed_selection_in_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness();
    let (towns, wells, at) = two_layers(&mut h);
    let state = h.state_mut();
    state.session = Some(Session::open(&dir.path().join("w.gpkg"), &mut state.doc).unwrap());
    state.save_waiting().unwrap();
    h.run_steps(2);
    h.state_mut().select_all_layers();
    pen_box(&mut h, at - vec2(30.0, 30.0), at + vec2(30.0, 30.0));
    assert_eq!(h.state().feature_selection.len(), 2);
    h.state_mut().delete_selection();
    h.run_steps(2);
    let count = |h: &Pad, id| h.state().doc.layer(id).unwrap().features.len();
    assert_eq!((count(&h, towns.layer), count(&h, wells.layer)), (0, 0));
    h.state_mut().undo();
    h.run_steps(2);
    assert_eq!((count(&h, towns.layer), count(&h, wells.layer)), (1, 1));
}

/// Runs a frame and the save of the frame, as the app does.
fn frame_and_save(h: &mut Pad) {
    h.run_steps(1);
    h.state_mut().save_tick(false, std::time::Instant::now());
}

#[test]
fn undo_of_delete_selection_selects_the_features_again_and_redo_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness();
    let (towns, wells, at) = two_layers(&mut h);
    let state = h.state_mut();
    state.session = Some(Session::open(&dir.path().join("w.gpkg"), &mut state.doc).unwrap());
    state.save_waiting().unwrap();
    h.state_mut().select_all_layers();
    pen_box(&mut h, at - vec2(30.0, 30.0), at + vec2(30.0, 30.0));
    frame_and_save(&mut h);
    let mut selected = h.state().feature_selection.clone();
    selected.sort_by_key(|f| f.layer.0);
    assert_eq!(selected, [towns, wells]);
    h.state_mut().delete_selection();
    frame_and_save(&mut h);
    assert!(h.state().feature_selection.is_empty());
    h.state_mut().undo();
    frame_and_save(&mut h);
    let mut restored = h.state().feature_selection.clone();
    restored.sort_by_key(|f| f.layer.0);
    assert_eq!(restored, selected);
    h.state_mut().redo();
    frame_and_save(&mut h);
    assert!(h.state().feature_selection.is_empty());
}

#[test]
fn a_selection_change_is_not_an_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness();
    let (towns, wells, _) = two_layers(&mut h);
    let state = h.state_mut();
    state.session = Some(Session::open(&dir.path().join("w.gpkg"), &mut state.doc).unwrap());
    state.save_waiting().unwrap();
    h.state_mut().select_all_layers();
    h.state_mut().set_feature_selection(vec![towns]);
    frame_and_save(&mut h);
    h.state_mut().delete_selection();
    frame_and_save(&mut h);
    h.state_mut().set_feature_selection(vec![wells]);
    frame_and_save(&mut h);
    // The undo undoes the delete, and the selection of Wells makes no step.
    h.state_mut().undo();
    frame_and_save(&mut h);
    assert!(h.state().doc.layer(towns.layer).unwrap().features.len() == 1);
    assert_eq!(h.state().feature_selection, [towns]);
    assert!(!h.state().can_undo());
}

#[test]
fn a_restored_selection_leaves_out_what_is_not_there() {
    use vale_store::{FrameId, StepSelection};
    let mut h = harness();
    let (towns, wells, _) = two_layers(&mut h);
    let gone = Feature::new(Geometry::Points(vec![[0.0, 0.0]]), Default::default());
    let gone = (towns.layer, gone.uuid());
    let pair = |f: FeatureRef| (f.layer, f.uuid);
    h.state_mut().restore_selection(&StepSelection {
        features: vec![pair(towns), gone, pair(wells)],
        key: Some(gone),
        frame: Some(FrameId(99)),
        item: None,
    });
    assert_eq!(h.state().feature_selection, [towns, wells]);
    assert_eq!(h.state().key_feature(), None);
    assert_eq!(h.state().atlas.frame(), None);
}

#[test]
fn merge_is_off_for_a_mixed_selection() {
    let mut h = harness();
    let (towns, wells, _) = two_layers(&mut h);
    let other = add_point(&mut h, towns.layer);
    h.state_mut().select_all_layers();
    h.state_mut().set_feature_selection(vec![towns, wells]);
    assert!(!h.state().can_merge());
    h.state_mut().set_feature_selection(vec![towns, other]);
    assert!(h.state().can_merge());
}
