//! A map on the globe: the globe checks the collections of the map and
//! shows the extent of the map.

use vale_store::MapId;

use super::{AppState, Workspace};
use crate::extent::Outline;
use crate::globe::WorldView;
use crate::globe::math::{V3, angle, dir_to_lonlat};
use crate::globe::view::GlobeView;

impl AppState {
    /// Opens the globe workspace with the collections of `map` checked on
    /// top of the stack in the order of the map, as one undo step, and with
    /// the extent of the map. The view moves to the extent, and the flat
    /// projection gets its center there.
    pub fn show_map_on_globe(&mut self, map: MapId) {
        let project = &self.doc.project;
        if project.atlas.map(map).is_none() {
            return;
        }
        let stack = project.map_stack(map);
        let mut checks: Vec<_> = (project.globe_collections().iter())
            .filter(|id| !stack.contains(id))
            .copied()
            .collect();
        checks.extend(stack);
        if checks != project.globe_collections() {
            self.save_edit();
            let changed = self.doc.project.set_globe_collections(checks);
            self.redraw_collections(changed);
            self.save_edit();
        }
        self.globe_map = Some(map);
        self.workspace = Workspace::Globe;
        self.pad.workspace = Workspace::Globe;
        if let (_, Some(outline)) = self.update_extent() {
            self.look_at_extent(&outline);
        }
    }

    /// Removes the extent and keeps the checks.
    pub fn close_map_on_globe(&mut self) {
        self.globe_map = None;
        self.update_extent();
    }

    /// The map on the globe. The map leaves the globe when it is gone or
    /// when the globe does not check a collection of the map.
    pub fn map_on_globe(&mut self) -> Option<MapId> {
        let map = self.globe_map?;
        let project = &self.doc.project;
        let checked = project.globe_collections();
        let shown = project.atlas.map(map).is_some()
            && project.map_stack(map).iter().all(|id| checked.contains(id));
        if !shown {
            self.globe_map = None;
        }
        self.globe_map
    }

    /// Brings the extent of the map on the globe up to date, and gives the
    /// map and the outline. The outline is `None` with no map, or with a map
    /// that has no frames.
    pub fn update_extent(&mut self) -> (Option<MapId>, Option<Outline>) {
        let Some(map) = self.map_on_globe() else {
            self.globe.extent = None;
            return (None, None);
        };
        let wait = self.headless;
        let doc = &self.doc;
        let Ok(projection) = self.pipeline.projection(doc, map) else {
            self.globe.extent = None;
            return (Some(map), None);
        };
        let (outline, gpu) = self.extent.update(&doc.project, map, projection, wait);
        self.globe.extent = gpu;
        let outline = (!outline.is_empty()).then_some(outline);
        (Some(map), outline)
    }

    /// Puts the middle of an outline at the middle of the canvas, and zooms
    /// so that the outline fits.
    fn look_at_extent(&mut self, outline: &[Vec<V3>]) {
        let points = || outline.iter().flatten();
        let mut sum = [0.0; 3];
        for p in points() {
            (0..3).for_each(|k| sum[k] += p[k]);
        }
        let length = (sum[0] * sum[0] + sum[1] * sum[1] + sum[2] * sum[2]).sqrt();
        if length < 1e-9 {
            return;
        }
        let center = sum.map(|c| c / length);
        // North is up, which needs a center off the poles.
        let (lon, lat) = dir_to_lonlat(center);
        let lat = lat.clamp(-89.9, 89.9);
        let reach = points().map(|&p| angle(p, center)).fold(0.0, f64::max);
        let globe = &mut self.globe;
        match globe.world_view {
            WorldView::Globe => {
                let mut view = GlobeView::centered(lon, lat);
                view.zoom = fit_zoom(reach);
                globe.view = view;
            }
            // The flat projection gets its center at the extent, as the
            // Recenter command does, so the extent is in one piece.
            WorldView::Flat => {
                let flat = &mut globe.flat;
                let spec = flat.spec();
                flat.set_spec(vale_sphere::ProjectionSpec {
                    lon0: lon,
                    lat0: lat,
                    ..spec
                });
                let projection = flat.projection();
                let projected = points().filter_map(|&p| {
                    let (lon, lat) = dir_to_lonlat(p);
                    projection.forward([lon, lat])
                });
                let bbox = projected.fold(
                    kurbo::Rect::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN),
                    |r, p| r.union_pt(p),
                );
                let rect = globe.rect;
                if bbox.x0 > bbox.x1 || rect.width() <= 0.0 || rect.height() <= 0.0 {
                    return;
                }
                flat.center = bbox.center();
                let unit = flat.scale(rect) / flat.zoom;
                let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
                let fit = (w / (bbox.width() * unit)).min(h / (bbox.height() * unit));
                flat.zoom = 0.8 * fit;
                flat.clamp();
            }
        }
    }
}

/// The zoom of the globe that shows the directions within `reach` radians
/// of the middle of the canvas, with a margin.
fn fit_zoom(reach: f64) -> f64 {
    let sin = reach.min(std::f64::consts::FRAC_PI_2).sin().max(1e-6);
    (0.8 / sin).clamp(1.0, crate::globe::view::ZOOM_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

    /// A state with collections A, B, and C, the globe checking World and
    /// B, and the first map with the stack C, A and one frame.
    fn state() -> (AppState, MapId, [vale_store::CollectionId; 4]) {
        let mut state = AppState::new(Document::sample()).unwrap();
        state.headless = true;
        let project = &mut state.doc.project;
        let world = project.world_collection();
        let [a, b, c] = ["A", "B", "C"].map(|name| project.add_collection(name).unwrap());
        assert!(project.set_globe_collections(vec![world, b]));
        let map = project.atlas.maps[0].id;
        assert!(project.set_map_stack(map, vec![c, a]));
        // One regional frame.
        let spec = state.doc.projection(map);
        let projection = vale_sphere::Projection::new(spec, 6_371_000.0).unwrap();
        let atlas = &mut state.doc.project.atlas;
        let mut frame = crate::atlas::new_frame(&atlas.pages[0], &atlas.maps[0], &projection);
        frame.shape = vale_store::FrameShape::Rectangle([20.0, 20.0, 180.0, 120.0]);
        frame.center = [10.0, 45.0];
        frame.scale = 20_000_000.0;
        atlas.pages[0].frames = vec![frame];
        (state, map, [world, a, b, c])
    }

    #[test]
    fn showing_a_map_checks_its_collections_in_its_order() {
        let (mut state, map, [world, a, b, c]) = state();
        state.workspace = Workspace::Atlas;
        state.pad.workspace = Workspace::Atlas;
        state.show_map_on_globe(map);
        assert_eq!(state.doc.project.globe_collections(), [world, b, c, a]);
        assert_eq!(state.map_on_globe(), Some(map));
        assert_eq!(state.workspace, Workspace::Globe);
        assert_eq!(state.pad.workspace, Workspace::Globe);
    }

    #[test]
    fn an_undo_takes_the_checks_and_the_map_back() {
        let dir = tempfile::tempdir().unwrap();
        let (mut state, map, [world, _, b, _]) = state();
        let path = dir.path().join("project.gpkg");
        state.session = Some(crate::session::Session::open(&path, &mut state.doc).unwrap());
        state.save_waiting().unwrap();
        state.show_map_on_globe(map);
        state.save_waiting().unwrap();
        assert!(state.can_undo());
        state.undo();
        assert_eq!(state.doc.project.globe_collections(), [world, b]);
        assert_eq!(state.map_on_globe(), None);
    }

    #[test]
    fn a_restart_keeps_the_map_on_the_globe() {
        let dir = tempfile::tempdir().unwrap();
        let (mut state, map, _) = state();
        let path = dir.path().join("project.gpkg");
        state.session = Some(crate::session::Session::open(&path, &mut state.doc).unwrap());
        state.screen = super::super::Screen::Editor;
        state.show_map_on_globe(map);
        let view = state.resume_now().view.unwrap();
        assert_eq!(view.map, Some(map));
        state.close_map_on_globe();
        state.show_view(view);
        assert_eq!(state.map_on_globe(), Some(map));
    }

    #[test]
    fn the_flat_projection_centers_on_the_extent() {
        let (mut state, map, _) = state();
        state.globe.world_view = WorldView::Flat;
        state.globe.rect = eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(800.0, 600.0),
        );
        state.show_map_on_globe(map);
        let spec = state.globe.flat.spec();
        // The frame is centered at 10 degrees east and 45 degrees north.
        assert!((spec.lon0 - 10.0).abs() < 2.0, "{spec:?}");
        assert!(state.globe.flat.zoom > 1.0);
    }

    #[test]
    fn closing_keeps_the_checks() {
        let (mut state, map, [world, a, b, c]) = state();
        state.show_map_on_globe(map);
        state.close_map_on_globe();
        assert_eq!(state.map_on_globe(), None);
        assert!(state.globe.extent.is_none());
        assert_eq!(state.doc.project.globe_collections(), [world, b, c, a]);
    }

    #[test]
    fn clearing_a_check_of_the_map_removes_the_map() {
        let (mut state, map, [_, a, b, _]) = state();
        state.show_map_on_globe(map);
        // A check of another collection does not remove it.
        state.check_collection(b, false);
        assert_eq!(state.map_on_globe(), Some(map));
        state.check_collection(a, false);
        assert_eq!(state.map_on_globe(), None);
    }

    #[test]
    fn the_map_has_an_extent_and_an_outline() {
        let (mut state, map, _) = state();
        state.show_map_on_globe(map);
        let (shown, outline) = state.update_extent();
        assert_eq!(shown, Some(map));
        assert!(outline.is_some_and(|o| !o.is_empty()));
        assert!(state.globe.extent.is_some());
    }

    #[test]
    fn the_view_moves_to_the_extent() {
        let (mut state, map, _) = state();
        let globe = &mut state.globe;
        globe.view = GlobeView::centered(-120.0, -40.0);
        state.show_map_on_globe(map);
        let (_, outline) = state.update_extent();
        let outline = outline.unwrap();
        let points = outline.iter().flatten();
        let view = &state.globe.view;
        // Each point of the outline faces the eye.
        assert!(points.clone().all(|&p| view.rot.mul_vec(p)[2] > 0.0));
    }
}
