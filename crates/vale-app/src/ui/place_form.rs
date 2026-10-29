//! The section of the panel of a layer from a file that places the file on
//! the world. Both layouts show it with the texts and the logic of this
//! module.

use eframe::egui::{self, Align2, Color32, Pos2, Rect, pos2};
use vale_store::{
    ControlPoint, Extents, Geometry, IMAGE_TILE_SIZE, ImageId, LayerId, PlaceMethod, Placement,
};

use super::{AppState, LayerRef};
use crate::image_layer::NOT_WIDE;
use crate::place::{PlaceDraft, layer_extents};

/// The label of the row with the name of the file.
pub const FILE: &str = "File";
pub const HEADING: &str = "Place on the world";
/// The methods and their names, in the order of the segmented control.
pub const METHODS: [(PlaceMethod, &str); 3] = [
    (PlaceMethod::Globe, "Globe"),
    (PlaceMethod::Extents, "Extents"),
    (PlaceMethod::Points, "Points"),
];
/// The labels of the fields of the edges: west, east, south, and north.
pub const EDGES: [&str; 4] = ["West", "East", "South", "North"];
/// The labels of the rows of the control points.
pub const POINTS: [&str; 2] = ["Point 1", "Point 2"];
/// The label of the read-out of the extents of the method Points.
pub const EXTENTS: &str = "Extents";
pub const PLACE: &str = "Place";
/// The name of the picture of the file, which takes the markers.
pub const PICTURE: &str = "Picture of the file";
pub const BAD_EDGES: &str = "Type valid edges.";
pub const BAD_COORDINATES: &str = "Type valid coordinates.";
pub const BAD_POINTS: &str = "Move the points apart.";
/// The color of the paper under the picture of the file.
pub const PAPER: Color32 = Color32::from_rgb(0xf4, 0xf1, 0xe6);
/// The color of the markers and of the lines of a vector file.
const INK: Color32 = Color32::from_rgb(0x17, 0x19, 0x1c);
/// The widest level of the image pyramid that the picture reads.
const PICTURE_PIXELS: u32 = 512;

/// The name of the control of a segment of the method.
pub fn method_name(label: &str) -> String {
    format!("Place by {label}")
}

/// The name of the field of a coordinate of a control point.
pub fn coordinate_name(point: usize, axis: Axis) -> String {
    let what = match axis {
        Axis::Lon => "longitude",
        Axis::Lat => "latitude",
    };
    format!("{} {what}", POINTS[point])
}

/// A longitude or a latitude.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Axis {
    Lon,
    Lat,
}

impl Axis {
    /// The letter of the positive side and of the negative side.
    fn letters(self) -> [char; 2] {
        match self {
            Axis::Lon => ['E', 'W'],
            Axis::Lat => ['N', 'S'],
        }
    }
}

/// The degrees of a text such as "24", "-24", "24°E", "24 W", or "24.5°".
/// W and S give a negative value. A longitude takes no N or S, and a
/// latitude takes no E or W.
pub fn parse_degrees(text: &str, axis: Axis) -> Option<f64> {
    let mut text = text.trim();
    let mut sign = 1.0;
    if let Some(last) = text.chars().last().filter(char::is_ascii_alphabetic) {
        let [plus, minus] = axis.letters();
        sign = match last.to_ascii_uppercase() {
            c if c == plus => 1.0,
            c if c == minus => -1.0,
            _ => return None,
        };
        text = text[..text.len() - 1].trim_end();
    }
    let text = text.strip_suffix('°').unwrap_or(text).trim_end();
    let value: f64 = text.parse().ok()?;
    if !value.is_finite() || (sign < 0.0 && value < 0.0) {
        return None;
    }
    Some(sign * value)
}

/// The number with up to two decimals and no zeros at the end.
fn number(value: f64) -> String {
    let text = format!("{value:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0" } else { text }.to_owned()
}

/// The letter of the side of a value, or `None` for zero.
fn side(value: f64, axis: Axis) -> Option<char> {
    let [plus, minus] = axis.letters();
    match number(value.abs()).as_str() {
        "0" => None,
        _ if value > 0.0 => Some(plus),
        _ => Some(minus),
    }
}

/// The degrees with their side, such as "24°E" or "0°".
pub fn format_degrees(value: f64, axis: Axis) -> String {
    let side = side(value, axis).map(String::from).unwrap_or_default();
    format!("{}°{side}", number(value.abs()))
}

/// A range of degrees, such as "19–40°E" or "10°W–20°E".
pub fn format_range(low: f64, high: f64, axis: Axis) -> String {
    match (side(low, axis), side(high, axis)) {
        (a, b) if a == b || a.is_none() || b.is_none() => {
            let side = a.or(b).map(String::from).unwrap_or_default();
            format!("{}–{}°{side}", number(low.abs()), number(high.abs()))
        }
        _ => format!(
            "{}–{}",
            format_degrees(low, axis),
            format_degrees(high, axis)
        ),
    }
}

/// The extents as a short text, such as "19–40°E · 38–54°N".
pub fn extents_text(extents: &Extents) -> String {
    format!(
        "{} · {}",
        format_range(extents.west, extents.east, Axis::Lon),
        format_range(extents.south, extents.north, Axis::Lat)
    )
}

/// True if a file of this size is not twice as wide as it is high, by
/// more than one percent.
pub fn not_wide(size: [f64; 2]) -> bool {
    (size[0] / size[1] / 2.0 - 1.0).abs() > 0.01
}

/// The warning of the method Globe for a file that is not 2:1.
pub fn not_wide_text() -> String {
    format!("The file {NOT_WIDE}.")
}

/// A layer from a file: an image layer, or a vector layer with a placement.
pub struct FileLayer {
    pub id: LayerId,
    /// The name of the file.
    pub source: Option<String>,
    /// The stored placement. An image layer with no placement covers the
    /// globe.
    pub placement: Placement,
    pub image: Option<ImageId>,
}

/// The layer from a file that a panel shows, or `None` for another layer.
pub fn file_layer(state: &AppState, of: LayerRef) -> Option<FileLayer> {
    let project = &state.doc.project;
    let (layer, image, placement) = match of {
        LayerRef::Image(id) => {
            let image = project.image(id)?;
            let layer = project.image_layer(id)?;
            let size = [f64::from(image.width), f64::from(image.height)];
            let placement = layer.placement.unwrap_or(Placement::globe(size));
            (layer, Some(id), placement)
        }
        LayerRef::Vector(id) => {
            let layer = project.layer(id)?;
            (layer, None, layer.placement?)
        }
        LayerRef::Raster(_) => return None,
    };
    Some(FileLayer {
        id: layer.id,
        source: layer.source.clone(),
        placement,
        image,
    })
}

/// What the picture of the file shows.
pub enum Picture {
    /// The picture is not read yet.
    Unread,
    Image(egui::TextureHandle),
    /// The lines and the points of a vector file, as fractions of the width
    /// and the height from the top left.
    Shapes {
        lines: Vec<Vec<[f32; 2]>>,
        points: Vec<[f32; 2]>,
    },
    /// Only the paper.
    Blank,
}

/// The texts and the method of the section, which stay across frames.
pub struct PlaceForm {
    pub layer: LayerId,
    /// The stored placement that the texts come from.
    stored: Placement,
    pub method: PlaceMethod,
    /// The texts of the west, the east, the south, and the north edge.
    pub edges: [String; 4],
    /// The texts of the longitude and the latitude of each control point.
    pub coordinates: [[String; 2]; 2],
    /// The places of the control points in the file, as fractions of the
    /// width and the height from the top left.
    pub marks: [[f64; 2]; 2],
    /// The marker that a press moves.
    held: Option<usize>,
    /// The last frame that showed the section.
    shown: u64,
    pub picture: Picture,
}

impl PlaceForm {
    pub fn new(layer: LayerId, stored: Placement) -> PlaceForm {
        let e = stored.extents;
        let edges = [
            format_degrees(e.west, Axis::Lon),
            format_degrees(e.east, Axis::Lon),
            format_degrees(e.south, Axis::Lat),
            format_degrees(e.north, Axis::Lat),
        ];
        let point = |p: &ControlPoint| {
            [
                format_degrees(p.lon, Axis::Lon),
                format_degrees(p.lat, Axis::Lat),
            ]
        };
        PlaceForm {
            layer,
            stored,
            method: stored.method,
            edges,
            coordinates: stored.points.each_ref().map(point),
            marks: stored.points.map(|p| [p.x, p.y]),
            held: None,
            shown: 0,
            picture: Picture::Unread,
        }
    }

    /// The placement of the texts, or the hint that tells what is wrong.
    pub fn candidate(&self) -> Result<Placement, &'static str> {
        let stored = self.stored;
        let placement = |extents, points| Placement {
            method: self.method,
            extents,
            points,
            size: stored.size,
        };
        // The control points keep their places in the file and get their
        // places in the extents.
        let on = |e: Extents| {
            stored.points.map(|p| ControlPoint {
                lon: e.west + p.x * (e.east - e.west),
                lat: e.north - p.y * (e.north - e.south),
                ..p
            })
        };
        match self.method {
            PlaceMethod::Globe => Ok(placement(Extents::GLOBE, on(Extents::GLOBE))),
            PlaceMethod::Extents => {
                let axes = [Axis::Lon, Axis::Lon, Axis::Lat, Axis::Lat];
                let mut edges = [0.0; 4];
                for (i, edge) in edges.iter_mut().enumerate() {
                    *edge = parse_degrees(&self.edges[i], axes[i]).ok_or(BAD_EDGES)?;
                }
                let [west, east, south, north] = edges;
                let extents = Extents {
                    west,
                    east,
                    south,
                    north,
                };
                match extents.is_valid() {
                    true => Ok(placement(extents, on(extents))),
                    false => Err(BAD_EDGES),
                }
            }
            PlaceMethod::Points => {
                let mut points = stored.points;
                for (i, point) in points.iter_mut().enumerate() {
                    let [lon, lat] = &self.coordinates[i];
                    let lon = parse_degrees(lon, Axis::Lon).ok_or(BAD_COORDINATES)?;
                    let lat = parse_degrees(lat, Axis::Lat).ok_or(BAD_COORDINATES)?;
                    let [x, y] = self.marks[i];
                    *point = ControlPoint { x, y, lon, lat };
                }
                let extents = Extents::from_points(points[0], points[1]);
                match extents.filter(Extents::is_valid) {
                    Some(extents) => Ok(placement(extents, points)),
                    None => Err(BAD_POINTS),
                }
            }
        }
    }

    /// The placement that "Place" stores: a valid one that differs from the
    /// stored one.
    pub fn ready(&self) -> Option<Placement> {
        self.candidate().ok().filter(|p| *p != self.stored)
    }

    /// Moves a marker to a place in the file. The first place of a press
    /// picks the nearest marker.
    pub fn press(&mut self, at: [f64; 2], aspect: f64) {
        let at = at.map(|n| n.clamp(0.0, 1.0));
        let distance = |m: &[f64; 2]| ((m[0] - at[0]) * aspect).hypot(m[1] - at[1]);
        let nearest = usize::from(distance(&self.marks[1]) < distance(&self.marks[0]));
        let held = *self.held.get_or_insert(nearest);
        self.marks[held] = at;
    }

    pub fn release(&mut self) {
        self.held = None;
    }
}

/// Makes the form of the layer of a panel, and reads its picture. A new
/// layer or a new stored placement gives new texts and clears the draft.
/// Returns the layer, or `None` if it did not come from a file.
pub fn open(state: &mut AppState, ctx: &egui::Context, of: LayerRef) -> Option<FileLayer> {
    let file = file_layer(state, of)?;
    let same = state.place_form.as_ref().filter(|f| f.layer == file.id);
    if same.is_none_or(|f| f.stored != file.placement) {
        let mut form = PlaceForm::new(file.id, file.placement);
        if let Some(old) = state.place_form.take().filter(|f| f.layer == file.id) {
            form.picture = old.picture;
        }
        state.place_form = Some(form);
        state.cancel_place_draft();
    }
    if matches!(state.place_form.as_ref()?.picture, Picture::Unread) {
        let picture = read_picture(state, ctx, &file);
        state.place_form.as_mut()?.picture = picture;
    }
    let frames = state.frames;
    state.place_form.as_mut()?.shown = frames;
    Some(file)
}

/// Sets the draft to the placement of the form, so the world view shows
/// it. A form with no new valid placement clears the draft.
pub fn preview(state: &mut AppState) {
    let draft = state.place_form.as_ref().and_then(|form| {
        let placement = form.ready()?;
        Some(PlaceDraft {
            layer: form.layer,
            placement,
        })
    });
    if state.place_draft != draft {
        state.set_place_draft(draft);
    }
}

/// Stores the placement of the form as one undo step.
pub fn place(state: &mut AppState) -> bool {
    preview(state);
    state.commit_place_draft()
}

/// Removes the form and the draft after a frame that did not show the
/// section: the panel closed, or it shows another layer.
pub fn end_frame(state: &mut AppState) {
    let frames = state.frames;
    if state.place_form.as_ref().is_some_and(|f| f.shown != frames) {
        state.place_form = None;
    }
    let layer = state.place_form.as_ref().map(|f| f.layer);
    if state.place_draft.is_some_and(|d| Some(d.layer) != layer) {
        state.cancel_place_draft();
    }
}

fn read_picture(state: &AppState, ctx: &egui::Context, file: &FileLayer) -> Picture {
    match file.image {
        Some(id) => read_image(state, ctx, id).map_or(Picture::Blank, Picture::Image),
        None => vector_shapes(state, file.id),
    }
}

/// The picture of an image from a small level of its pyramid.
fn read_image(state: &AppState, ctx: &egui::Context, id: ImageId) -> Option<egui::TextureHandle> {
    let image = state.doc.project.image(id)?;
    let (width, height) = (image.width, image.height);
    let levels = vale_store::image_levels(width, height);
    let level = (0..levels)
        .find(|&l| vale_store::level_size(width, height, l).0 <= PICTURE_PIXELS)
        .unwrap_or(levels - 1);
    let (w, h) = vale_store::level_size(width, height, level);
    let mut rgba = vec![0u8; w as usize * h as usize * 4];
    for row in 0..h.div_ceil(IMAGE_TILE_SIZE) {
        for column in 0..w.div_ceil(IMAGE_TILE_SIZE) {
            let blob = state.image_tile(id, level, column, row).ok()??;
            let (tw, th, tile) = vale_store::decode_tile(&blob).ok()?;
            let (x0, y0) = (column * IMAGE_TILE_SIZE, row * IMAGE_TILE_SIZE);
            let tw = tw.min(w - x0);
            for y in 0..th.min(h - y0) {
                let from = (y * tw * 4) as usize;
                let to = (((y0 + y) * w + x0) * 4) as usize;
                let len = (tw * 4) as usize;
                rgba[to..to + len].copy_from_slice(tile.get(from..from + len)?);
            }
        }
    }
    let picture = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
    let name = format!("place-picture-{}", id.0);
    Some(ctx.load_texture(name, picture, egui::TextureOptions::LINEAR))
}

/// The lines and the points of a vector layer in its extents. A line keeps
/// a point only if it is far enough from the last one for the picture.
fn vector_shapes(state: &AppState, id: LayerId) -> Picture {
    let Some(layer) = state.doc.project.layer(id) else {
        return Picture::Blank;
    };
    let e = layer_extents(layer);
    let place = |p: &[f64; 2]| {
        [
            ((p[0] - e.west) / (e.east - e.west)) as f32,
            ((e.north - p[1]) / (e.north - e.south)) as f32,
        ]
    };
    const STEP: f32 = 1.0 / 600.0;
    let line = |points: &Vec<[f64; 2]>| {
        let mut out: Vec<[f32; 2]> = Vec::new();
        for p in points.iter().map(place) {
            let far = out
                .last()
                .is_none_or(|q| (p[0] - q[0]).abs().max((p[1] - q[1]).abs()) >= STEP);
            if far {
                out.push(p);
            }
        }
        out
    };
    let (mut lines, mut points) = (Vec::new(), Vec::new());
    for feature in &layer.features {
        match &feature.geometry {
            Geometry::Points(p) => points.extend(p.iter().map(place)),
            Geometry::Lines(l) => lines.extend(l.iter().map(line)),
            Geometry::Polygons(p) => lines.extend(p.iter().flatten().map(line)),
        }
    }
    Picture::Shapes { lines, points }
}

/// The rectangle of the picture at the top of `space`: as wide as the space
/// and at the aspect ratio of the file, or as high as `max_height` and in
/// the middle.
pub fn picture_rect(space: Rect, size: [f64; 2], max_height: f32) -> Rect {
    let aspect = (size[0] / size[1]) as f32;
    let aspect = if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        2.0
    };
    let mut width = space.width();
    let mut height = width / aspect;
    if height > max_height {
        height = max_height;
        width = height * aspect;
    }
    let left = space.center().x - width / 2.0;
    Rect::from_min_size(pos2(left, space.top()), egui::vec2(width, height))
}

/// The place on the screen of a place in the file.
fn screen_place(rect: Rect, at: [f64; 2]) -> Pos2 {
    pos2(
        rect.left() + at[0] as f32 * rect.width(),
        rect.top() + at[1] as f32 * rect.height(),
    )
}

/// Paints the paper, the picture, and, for the method Points, the numbered
/// markers. `radius` is the radius of a marker.
pub fn paint_picture(
    painter: &egui::Painter,
    rect: Rect,
    form: &PlaceForm,
    corner: f32,
    radius: f32,
    font: egui::FontId,
) {
    painter.rect_filled(rect, corner, PAPER);
    match &form.picture {
        Picture::Image(texture) => {
            let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
            let shape = egui::epaint::RectShape::filled(rect, corner, Color32::WHITE)
                .with_texture(texture.id(), uv);
            painter.add(shape);
        }
        Picture::Shapes { lines, points } => {
            let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
            let at = |p: &[f32; 2]| {
                pos2(
                    rect.left() + p[0] * rect.width(),
                    rect.top() + p[1] * rect.height(),
                )
            };
            let stroke = egui::Stroke::new(1.0, INK.gamma_multiply(0.8));
            for line in lines.iter().filter(|l| l.len() > 1) {
                painter.line(line.iter().map(at).collect(), stroke);
            }
            for point in points {
                painter.circle_filled(at(point), 1.6, INK);
            }
        }
        Picture::Unread | Picture::Blank => {}
    }
    if form.method != PlaceMethod::Points {
        return;
    }
    for (i, mark) in form.marks.iter().enumerate() {
        let center = screen_place(rect, *mark);
        painter.circle(center, radius, INK, egui::Stroke::new(1.5, Color32::WHITE));
        let number = (i + 1).to_string();
        painter.text(center, Align2::CENTER_CENTER, number, font.clone(), PAPER);
    }
}

/// Moves the nearest marker while a press or a drag of the method Points is
/// on the picture.
pub fn picture_input(form: &mut PlaceForm, rect: Rect, resp: &egui::Response) {
    let pressed = resp.is_pointer_button_down_on() || resp.dragged() || resp.clicked();
    match resp.interact_pointer_pos() {
        Some(at) if pressed && form.method == PlaceMethod::Points => {
            let x = f64::from((at.x - rect.left()) / rect.width());
            let y = f64::from((at.y - rect.top()) / rect.height());
            form.press([x, y], f64::from(rect.width() / rect.height()));
        }
        _ => form.release(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_gives_degrees_with_west_and_south_negative() {
        let lon = |t| parse_degrees(t, Axis::Lon);
        let lat = |t| parse_degrees(t, Axis::Lat);
        assert_eq!(lon("24"), Some(24.0));
        assert_eq!(lon("-24"), Some(-24.0));
        assert_eq!(lon("24°E"), Some(24.0));
        assert_eq!(lon("24 W"), Some(-24.0));
        assert_eq!(lon("24w"), Some(-24.0));
        assert_eq!(lon(" 24.5° "), Some(24.5));
        assert_eq!(lon("24° e"), Some(24.0));
        assert_eq!(lat("38.25°S"), Some(-38.25));
        assert_eq!(lat("50n"), Some(50.0));
        // A longitude takes no N or S, and a latitude takes no E or W.
        assert_eq!(lon("50N"), None);
        assert_eq!(lat("24E"), None);
        for bad in ["", "°", "E", "abc", "-24W", "inf", "NaN", "1e999", "24°°"] {
            assert_eq!(lon(bad), None, "{bad}");
        }
    }

    #[test]
    fn degrees_have_up_to_two_decimals_and_a_side() {
        assert_eq!(format_degrees(24.0, Axis::Lon), "24°E");
        assert_eq!(format_degrees(-24.5, Axis::Lon), "24.5°W");
        assert_eq!(format_degrees(38.256, Axis::Lat), "38.26°N");
        assert_eq!(format_degrees(-90.0, Axis::Lat), "90°S");
        assert_eq!(format_degrees(0.0, Axis::Lat), "0°");
        assert_eq!(format_degrees(-0.001, Axis::Lon), "0°");
        for v in [-179.99, -12.5, 0.0, 7.25, 180.0] {
            let text = format_degrees(v, Axis::Lon);
            assert_eq!(parse_degrees(&text, Axis::Lon), Some(v), "{text}");
        }
    }

    #[test]
    fn a_range_has_one_side_if_both_ends_are_on_it() {
        let e = Extents {
            west: 19.0,
            east: 40.0,
            south: 38.0,
            north: 54.0,
        };
        assert_eq!(extents_text(&e), "19–40°E · 38–54°N");
        assert_eq!(format_range(-10.0, 20.0, Axis::Lon), "10°W–20°E");
        assert_eq!(format_range(-40.0, -19.0, Axis::Lon), "40–19°W");
        assert_eq!(format_range(0.0, 20.5, Axis::Lat), "0–20.5°N");
        assert_eq!(format_range(-90.0, 90.0, Axis::Lat), "90°S–90°N");
    }

    #[test]
    fn a_file_is_wide_if_it_is_2_to_1_within_one_percent() {
        assert!(!not_wide([2000.0, 1000.0]));
        assert!(!not_wide([2010.0, 1000.0]));
        assert!(not_wide([2100.0, 1000.0]));
        assert!(not_wide([1000.0, 1000.0]));
        assert_eq!(
            not_wide_text(),
            "The file is not 2:1, so it is stretched to cover the globe."
        );
    }

    #[test]
    fn a_press_moves_the_nearest_marker_until_the_release() {
        let mut form = PlaceForm::new(LayerId(1), Placement::globe([200.0, 100.0]));
        form.method = PlaceMethod::Points;
        assert_eq!(form.marks, [[0.25, 0.25], [0.75, 0.75]]);
        form.press([0.7, 0.6], 2.0);
        assert_eq!(form.marks[1], [0.7, 0.6]);
        // The held marker follows the press past the other one.
        form.press([0.1, 0.1], 2.0);
        assert_eq!(form.marks, [[0.25, 0.25], [0.1, 0.1]]);
        form.release();
        form.press([0.3, 0.3], 2.0);
        assert_eq!(form.marks[0], [0.3, 0.3]);
    }

    #[test]
    fn the_points_give_the_extents_and_a_bad_text_gives_a_hint() {
        let mut form = PlaceForm::new(LayerId(1), Placement::globe([200.0, 100.0]));
        assert_eq!(form.ready(), None);
        form.method = PlaceMethod::Points;
        form.marks = [[0.25, 0.25], [0.75, 0.75]];
        form.coordinates = [["10E".into(), "60N".into()], ["20E".into(), "40N".into()]];
        let placement = form.ready().unwrap();
        let expected = Extents {
            west: 5.0,
            east: 25.0,
            south: 30.0,
            north: 70.0,
        };
        assert_eq!(placement.extents, expected);
        assert_eq!(placement.method, PlaceMethod::Points);
        form.coordinates[1][0] = "20N".into();
        assert_eq!(form.candidate(), Err(BAD_COORDINATES));
        form.coordinates[1][0] = "10E".into();
        assert_eq!(form.candidate(), Err(BAD_POINTS));
        form.method = PlaceMethod::Extents;
        form.edges = ["20E".into(), "10E".into(), "0".into(), "10N".into()];
        assert_eq!(form.candidate(), Err(BAD_EDGES));
    }
}
