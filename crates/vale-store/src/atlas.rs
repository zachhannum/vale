//! The atlas: the maps, and the pages with their map frames.
//!
//! A page coordinate is in millimeters from the top-left corner of the page,
//! with y down.

use std::collections::BTreeSet;
use std::ops::RangeInclusive;

use crate::gpkg::Reader;
use crate::{LayerId, LonLat, Rgb, Units};

/// The ID of a map.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MapId(pub u32);

/// The ID of a page.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PageId(pub u32);

/// The ID of a map frame.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FrameId(pub u32);

/// A map: a projection and the layers that it does not show.
#[derive(Clone, Debug, PartialEq)]
pub struct MapRecord {
    pub id: MapId,
    pub name: String,
    /// The PROJ string of the projection, with no radius.
    pub projection: String,
    /// The layers that the map does not show.
    pub hidden: BTreeSet<LayerId>,
}

/// The units in which the app shows the size of a page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PageUnits {
    #[default]
    Mm,
    In,
}

impl PageUnits {
    pub const ALL: [PageUnits; 2] = [PageUnits::Mm, PageUnits::In];

    /// The name of the units in the project file.
    pub fn id(self) -> &'static str {
        match self {
            PageUnits::Mm => "mm",
            PageUnits::In => "in",
        }
    }

    pub fn from_id(id: &str) -> Option<PageUnits> {
        PageUnits::ALL.into_iter().find(|u| u.id() == id)
    }
}

/// A page and its map frames.
#[derive(Clone, Debug, PartialEq)]
pub struct PageRecord {
    pub id: PageId,
    /// Free text. An empty name shows as the number of the page.
    pub name: String,
    pub width_mm: f64,
    pub height_mm: f64,
    pub units: PageUnits,
    /// From the bottom frame to the top frame.
    pub frames: Vec<FrameRecord>,
}

/// The outline of a map frame, in page millimeters.
#[derive(Clone, Debug, PartialEq)]
pub enum FrameShape {
    /// `[x0, y0, x1, y1]`.
    Rectangle([f64; 4]),
    /// Ring 0 is the outer ring, and each other ring is a hole. The fill
    /// rule is even-odd.
    Polygon(Vec<Vec<[f64; 2]>>),
}

impl FrameShape {
    /// The name of the kind of shape in the project file.
    pub fn kind_id(&self) -> &'static str {
        match self {
            FrameShape::Rectangle(_) => "rectangle",
            FrameShape::Polygon(_) => "polygon",
        }
    }

    /// The rings of the shape. A rectangle has one ring of 4 corners.
    pub fn rings(&self) -> Vec<Vec<[f64; 2]>> {
        match self {
            FrameShape::Rectangle([x0, y0, x1, y1]) => {
                vec![vec![[*x0, *y0], [*x1, *y0], [*x1, *y1], [*x0, *y1]]]
            }
            FrameShape::Polygon(rings) => rings.clone(),
        }
    }

    /// The shape of a kind and its rings, as `rings` gives them. `None`: the
    /// rings do not fit the kind.
    pub fn from_rings(kind: &str, rings: Vec<Vec<[f64; 2]>>) -> Option<FrameShape> {
        match kind {
            "rectangle" => match rings.as_slice() {
                [ring] if ring.len() == 4 => {
                    let ([x0, y0], [x1, y1]) = (ring[0], ring[2]);
                    Some(FrameShape::Rectangle([x0, y0, x1, y1]))
                }
                _ => None,
            },
            "polygon" if !rings.is_empty() => Some(FrameShape::Polygon(rings)),
            _ => None,
        }
    }
}

/// A view of a map on a page.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameRecord {
    pub id: FrameId,
    pub map: MapId,
    pub shape: FrameShape,
    /// The place at the middle of the frame.
    pub center: LonLat,
    /// N of the scale 1:N.
    pub scale: f64,
    /// Degrees clockwise.
    pub rotation: f64,
    pub graticule: Graticule,
}

/// The lines of latitude and longitude in a map frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Graticule {
    pub on: bool,
    /// Degrees between two lines. `None`: the frame picks the interval from
    /// its scale.
    pub interval: Option<f64>,
    pub color: Rgb,
    /// The line width in points.
    pub width: f64,
}

impl Graticule {
    /// The line widths in points that the app offers.
    pub const WIDTHS: RangeInclusive<f64> = 0.1..=2.0;
}

impl Default for Graticule {
    fn default() -> Self {
        Graticule {
            on: true,
            interval: None,
            color: [0x96, 0xa5, 0xaf],
            width: 0.5,
        }
    }
}

/// A projection that the project keeps under a name. A custom projection is
/// a preset that you edit and save.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectionPreset {
    pub name: String,
    /// The PROJ string of the projection, with no radius.
    pub projection: String,
}

/// The maps and the pages. The atlas has at least one map and one page.
#[derive(Clone, Debug, PartialEq)]
pub struct Atlas {
    pub maps: Vec<MapRecord>,
    /// The custom projections, in the order in which they were first saved.
    pub presets: Vec<ProjectionPreset>,
    /// In atlas order.
    pub pages: Vec<PageRecord>,
    pub(crate) next_map_id: u32,
    pub(crate) next_page_id: u32,
    pub(crate) next_frame_id: u32,
}

/// The PROJ string of the Equal Earth projection at 0, 0.
pub const EQUAL_EARTH: &str = "+proj=eqearth +lon_0=0";

impl Atlas {
    /// An atlas with one map and one empty page. The map has an Equal Earth
    /// projection at 0, 0. The page is A4 landscape for metric units and
    /// Letter landscape for imperial units.
    pub fn new(units: Units) -> Atlas {
        let (width_mm, height_mm, units) = match units {
            Units::Metric => (297.0, 210.0, PageUnits::Mm),
            Units::Imperial => (279.4, 215.9, PageUnits::In),
        };
        Atlas {
            maps: vec![MapRecord {
                id: MapId(0),
                name: "Map".to_string(),
                projection: EQUAL_EARTH.to_string(),
                hidden: BTreeSet::new(),
            }],
            presets: Vec::new(),
            pages: vec![PageRecord {
                id: PageId(0),
                name: String::new(),
                width_mm,
                height_mm,
                units,
                frames: Vec::new(),
            }],
            next_map_id: 1,
            next_page_id: 1,
            next_frame_id: 0,
        }
    }

    /// Adds an empty page directly after a page, or at the end. Returns its ID.
    pub fn add_page(
        &mut self,
        after: Option<PageId>,
        width_mm: f64,
        height_mm: f64,
        units: PageUnits,
    ) -> PageId {
        let id = PageId(self.next_page_id);
        self.next_page_id += 1;
        let at = after
            .and_then(|a| self.page_index(a))
            .map_or(self.pages.len(), |i| i + 1);
        let page = PageRecord {
            id,
            name: String::new(),
            width_mm,
            height_mm,
            units,
            frames: Vec::new(),
        };
        self.pages.insert(at, page);
        id
    }

    /// Removes a page and its frames. `None`: no such page, or it is the
    /// last page.
    pub fn remove_page(&mut self, id: PageId) -> Option<PageRecord> {
        let i = self.page_index(id)?;
        (self.pages.len() > 1).then(|| self.pages.remove(i))
    }

    /// The place of a page in the atlas order.
    pub fn page_index(&self, id: PageId) -> Option<usize> {
        self.pages.iter().position(|p| p.id == id)
    }

    pub fn page(&self, id: PageId) -> Option<&PageRecord> {
        self.pages.iter().find(|p| p.id == id)
    }

    pub fn page_mut(&mut self, id: PageId) -> Option<&mut PageRecord> {
        self.pages.iter_mut().find(|p| p.id == id)
    }

    /// Puts a frame on top of the frames of a page, with a new ID in place
    /// of the ID of `frame`. `None`: no such page or no such map.
    pub fn add_frame(&mut self, page: PageId, mut frame: FrameRecord) -> Option<FrameId> {
        self.map(frame.map)?;
        let id = FrameId(self.next_frame_id);
        frame.id = id;
        self.page_mut(page)?.frames.push(frame);
        self.next_frame_id += 1;
        Some(id)
    }

    pub fn frame(&self, id: FrameId) -> Option<&FrameRecord> {
        self.pages
            .iter()
            .flat_map(|p| &p.frames)
            .find(|f| f.id == id)
    }

    pub fn frame_mut(&mut self, id: FrameId) -> Option<&mut FrameRecord> {
        let mut frames = self.pages.iter_mut().flat_map(|p| &mut p.frames);
        frames.find(|f| f.id == id)
    }

    /// The page that has a frame.
    pub fn page_of_frame(&self, id: FrameId) -> Option<PageId> {
        let page = self
            .pages
            .iter()
            .find(|p| p.frames.iter().any(|f| f.id == id));
        page.map(|p| p.id)
    }

    pub fn remove_frame(&mut self, id: FrameId) -> Option<FrameRecord> {
        self.pages.iter_mut().find_map(|p| {
            let i = p.frames.iter().position(|f| f.id == id)?;
            Some(p.frames.remove(i))
        })
    }

    /// Saves a projection under a name. A preset with that name changes.
    pub fn save_preset(&mut self, name: &str, projection: &str) {
        match self.presets.iter_mut().find(|p| p.name == name) {
            Some(preset) => preset.projection = projection.to_string(),
            None => self.presets.push(ProjectionPreset {
                name: name.to_string(),
                projection: projection.to_string(),
            }),
        }
    }

    /// Removes the preset with this name. Returns true if there was one. A
    /// map keeps its projection.
    pub fn remove_preset(&mut self, name: &str) -> bool {
        let before = self.presets.len();
        self.presets.retain(|p| p.name != name);
        self.presets.len() != before
    }

    pub fn map(&self, id: MapId) -> Option<&MapRecord> {
        self.maps.iter().find(|m| m.id == id)
    }

    pub fn map_mut(&mut self, id: MapId) -> Option<&mut MapRecord> {
        self.maps.iter_mut().find(|m| m.id == id)
    }

    /// Adds a map with an Equal Earth projection at 0, 0 that shows each
    /// layer, after the last map. Its name is the first free name of "Map",
    /// "Map 2", "Map 3", and so on.
    pub fn add_map(&mut self) -> MapId {
        let name = self.free_name("Map");
        self.push_map(MapRecord {
            id: MapId(0),
            name,
            projection: EQUAL_EARTH.to_string(),
            hidden: BTreeSet::new(),
        })
    }

    /// The first map that no frame shows.
    pub fn unshown_map(&self) -> Option<MapId> {
        let free = self
            .maps
            .iter()
            .find(|m| self.frames_of_map(m.id).next().is_none());
        free.map(|m| m.id)
    }

    /// Removes each map that no frame shows. If no frame shows a map, the
    /// first map stays.
    pub fn prune_maps(&mut self) {
        let shown: Vec<bool> = (self.maps.iter())
            .map(|m| self.frames_of_map(m.id).next().is_some())
            .collect();
        let keep = shown.iter().any(|&s| s).then_some(0);
        let mut i = 0;
        self.maps.retain(|_| {
            i += 1;
            shown[i - 1] || keep.is_none() && i == 1
        });
    }

    fn push_map(&mut self, mut map: MapRecord) -> MapId {
        map.id = MapId(self.next_map_id);
        self.next_map_id += 1;
        self.maps.push(map);
        MapId(self.next_map_id - 1)
    }

    /// `base`, or `base` with the first free number from 2.
    fn free_name(&self, base: &str) -> String {
        let taken = |n: &str| self.maps.iter().any(|m| m.name == n);
        if !taken(base) {
            return base.to_string();
        }
        (2..)
            .map(|i| format!("{base} {i}"))
            .find(|n| !taken(n))
            .expect("a free name")
    }

    /// Each frame of a map, with its page, in atlas order.
    pub fn frames_of_map(&self, id: MapId) -> impl Iterator<Item = (&PageRecord, &FrameRecord)> {
        let frames = self
            .pages
            .iter()
            .flat_map(|p| p.frames.iter().map(move |f| (p, f)));
        frames.filter(move |(_, f)| f.map == id)
    }
}

/// The rings of a shape as a little-endian blob: the number of rings, then
/// for each ring the number of vertices and the x and y of each vertex.
pub(crate) fn encode_rings(rings: &[Vec<[f64; 2]>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(rings.len() as u32).to_le_bytes());
    for ring in rings {
        out.extend_from_slice(&(ring.len() as u32).to_le_bytes());
        for [x, y] in ring {
            out.extend_from_slice(&x.to_le_bytes());
            out.extend_from_slice(&y.to_le_bytes());
        }
    }
    out
}

/// `None`: the blob is not a whole set of rings.
pub(crate) fn decode_rings(blob: &[u8]) -> Option<Vec<Vec<[f64; 2]>>> {
    let mut r = Reader(blob);
    let rings = read_rings(&mut r)?;
    r.0.is_empty().then_some(rings)
}

pub(crate) fn read_rings(r: &mut Reader) -> Option<Vec<Vec<[f64; 2]>>> {
    let n = r.u32()? as usize;
    let mut rings = Vec::with_capacity(n.min(r.0.len() / 4));
    for _ in 0..n {
        let m = r.u32()? as usize;
        let mut ring = Vec::with_capacity(m.min(r.0.len() / 16));
        for _ in 0..m {
            ring.push([r.f64()?, r.f64()?]);
        }
        rings.push(ring);
    }
    Some(rings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(map: MapId) -> FrameRecord {
        FrameRecord {
            id: FrameId(99),
            map,
            shape: FrameShape::Rectangle([10.0, 10.0, 100.0, 80.0]),
            center: [12.0, -3.5],
            scale: 50_000_000.0,
            rotation: 15.0,
            graticule: Graticule::default(),
        }
    }

    #[test]
    fn a_saved_preset_replaces_the_preset_of_its_name() {
        let mut atlas = Atlas::new(Units::Metric);
        atlas.save_preset("Home", "+proj=laea +lon_0=1 +lat_0=2");
        atlas.save_preset("Far", "+proj=eqearth +lon_0=0");
        atlas.save_preset("Home", "+proj=laea +lon_0=3 +lat_0=4");
        assert_eq!(atlas.presets.len(), 2);
        assert_eq!(atlas.presets[0].projection, "+proj=laea +lon_0=3 +lat_0=4");
        assert!(atlas.remove_preset("Home"));
        assert!(!atlas.remove_preset("Home"));
        assert_eq!(atlas.presets.len(), 1);
    }

    #[test]
    fn a_new_atlas_has_one_map_and_one_page_in_the_units_of_the_world() {
        let metric = Atlas::new(Units::Metric);
        assert_eq!(metric.maps.len(), 1);
        assert_eq!(metric.maps[0].projection, EQUAL_EARTH);
        let page = &metric.pages[0];
        assert_eq!(
            (page.width_mm, page.height_mm, page.units),
            (297.0, 210.0, PageUnits::Mm)
        );
        let page = &Atlas::new(Units::Imperial).pages[0];
        assert_eq!(
            (page.width_mm, page.height_mm, page.units),
            (279.4, 215.9, PageUnits::In)
        );
    }

    #[test]
    fn pages_go_after_a_page_and_the_last_page_stays() {
        let mut atlas = Atlas::new(Units::Metric);
        let first = atlas.pages[0].id;
        let end = atlas.add_page(None, 100.0, 200.0, PageUnits::In);
        let middle = atlas.add_page(Some(first), 300.0, 400.0, PageUnits::Mm);
        let ids: Vec<PageId> = atlas.pages.iter().map(|p| p.id).collect();
        assert_eq!(ids, [first, middle, end]);
        assert_eq!(atlas.page(end).unwrap().width_mm, 100.0);
        assert_eq!(atlas.page_index(end), Some(2));

        assert_eq!(atlas.remove_page(middle).unwrap().id, middle);
        assert!(atlas.remove_page(middle).is_none());
        assert!(atlas.remove_page(first).is_some());
        assert!(atlas.remove_page(end).is_none());
        assert_eq!(atlas.pages.len(), 1);
        // The ID of a removed page is not used again.
        assert!(atlas.add_page(None, 1.0, 1.0, PageUnits::Mm) > end);
    }

    #[test]
    fn new_maps_get_free_names_and_new_ids() {
        let mut atlas = Atlas::new(Units::Metric);
        let first = atlas.maps[0].id;
        let second = atlas.add_map();
        let third = atlas.add_map();
        let names: Vec<&str> = atlas.maps.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Map", "Map 2", "Map 3"]);
        assert!(first < second && second < third);
        atlas.map_mut(second).unwrap().name = "Map 4".to_string();
        let fourth = atlas.add_map();
        assert_eq!(atlas.map(fourth).unwrap().name, "Map 2");
    }

    #[test]
    fn a_map_that_no_frame_shows_goes_away_but_the_last_map_stays() {
        let mut atlas = Atlas::new(Units::Metric);
        let shown = atlas.maps[0].id;
        let page = atlas.pages[0].id;
        assert_eq!(atlas.unshown_map(), Some(shown));
        let frame_id = atlas.add_frame(page, frame(shown)).unwrap();
        assert_eq!(atlas.unshown_map(), None);

        let free = atlas.add_map();
        assert_eq!(atlas.unshown_map(), Some(free));
        atlas.prune_maps();
        assert_eq!(atlas.maps.len(), 1);
        // The ID of a removed map is not used again.
        assert!(atlas.add_map() > free);

        atlas.remove_frame(frame_id);
        atlas.prune_maps();
        assert_eq!(atlas.maps.len(), 1);
        assert_eq!(atlas.maps[0].id, shown);
    }

    #[test]
    fn frames_go_on_a_page_and_come_off_it() {
        let mut atlas = Atlas::new(Units::Metric);
        let map = atlas.maps[0].id;
        let first = atlas.pages[0].id;
        let second = atlas.add_page(None, 297.0, 210.0, PageUnits::Mm);
        assert!(atlas.add_frame(first, frame(MapId(7))).is_none());
        assert!(atlas.add_frame(PageId(7), frame(map)).is_none());
        let a = atlas.add_frame(first, frame(map)).unwrap();
        let b = atlas.add_frame(second, frame(map)).unwrap();
        let c = atlas.add_frame(first, frame(map)).unwrap();
        assert_ne!(a, b);
        assert_eq!(atlas.frame(a).unwrap().id, a);
        let on_first: Vec<FrameId> = atlas
            .page(first)
            .unwrap()
            .frames
            .iter()
            .map(|f| f.id)
            .collect();
        assert_eq!(on_first, [a, c]);
        assert_eq!(atlas.page_of_frame(b), Some(second));
        let all: Vec<(PageId, FrameId)> = atlas
            .frames_of_map(map)
            .map(|(p, f)| (p.id, f.id))
            .collect();
        assert_eq!(all, [(first, a), (first, c), (second, b)]);

        atlas.frame_mut(c).unwrap().scale = 2.0;
        assert_eq!(atlas.frame(c).unwrap().scale, 2.0);
        assert_eq!(atlas.remove_frame(a).unwrap().id, a);
        assert!(atlas.frame(a).is_none());
        assert!(atlas.remove_frame(a).is_none());
        // The frames of a removed page go with it.
        atlas.remove_page(second);
        assert!(atlas.frame(b).is_none());
    }

    #[test]
    fn the_rings_of_a_shape_with_holes_round_trip() {
        let rings = vec![
            vec![
                [0.0, 0.0],
                [100.0, 0.0],
                [50.0, 30.0],
                [100.0, 80.0],
                [0.0, 80.0],
            ],
            vec![[10.0, 10.0], [20.0, 10.0], [20.0, 20.0]],
            vec![[60.0, 60.0], [70.0, 60.0], [70.0, 70.0], [60.0, 70.5]],
        ];
        let blob = encode_rings(&rings);
        assert_eq!(decode_rings(&blob), Some(rings.clone()));
        assert_eq!(decode_rings(&blob[..blob.len() - 1]), None);
        let shape = FrameShape::from_rings("polygon", rings.clone()).unwrap();
        assert_eq!(shape, FrameShape::Polygon(rings));

        let rect = FrameShape::Rectangle([1.0, 2.0, 3.0, 4.0]);
        let back = FrameShape::from_rings(
            rect.kind_id(),
            decode_rings(&encode_rings(&rect.rings())).unwrap(),
        );
        assert_eq!(back, Some(rect));
        assert_eq!(
            FrameShape::from_rings("rectangle", vec![vec![[0.0, 0.0]]]),
            None
        );
        assert_eq!(FrameShape::from_rings("polygon", vec![]), None);
    }
}
