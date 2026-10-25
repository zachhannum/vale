//! Projection presets and the PROJ string that each one makes.

use std::fmt;

use crate::frame::{Frame, wrap180};

/// The projections that the app offers.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ProjectionKind {
    Equirectangular,
    EqualEarth,
    Mercator,
    TransverseMercator,
    LambertAzimuthal,
    LambertConformalConic,
    AlbersEqualArea,
    Orthographic,
    Stereographic,
}

impl ProjectionKind {
    pub const ALL: [ProjectionKind; 9] = [
        ProjectionKind::Equirectangular,
        ProjectionKind::EqualEarth,
        ProjectionKind::Mercator,
        ProjectionKind::TransverseMercator,
        ProjectionKind::LambertAzimuthal,
        ProjectionKind::LambertConformalConic,
        ProjectionKind::AlbersEqualArea,
        ProjectionKind::Orthographic,
        ProjectionKind::Stereographic,
    ];

    /// A name for people.
    pub fn name(self) -> &'static str {
        match self {
            ProjectionKind::Equirectangular => "Equirectangular",
            ProjectionKind::EqualEarth => "Equal Earth",
            ProjectionKind::Mercator => "Mercator",
            ProjectionKind::TransverseMercator => "Transverse Mercator",
            ProjectionKind::LambertAzimuthal => "Lambert azimuthal",
            ProjectionKind::LambertConformalConic => "Lambert conformal conic",
            ProjectionKind::AlbersEqualArea => "Albers equal-area conic",
            ProjectionKind::Orthographic => "Orthographic",
            ProjectionKind::Stereographic => "Stereographic",
        }
    }

    /// A stable name for the command line.
    pub fn id(self) -> &'static str {
        match self {
            ProjectionKind::Equirectangular => "equirectangular",
            ProjectionKind::EqualEarth => "equal-earth",
            ProjectionKind::Mercator => "mercator",
            ProjectionKind::TransverseMercator => "transverse-mercator",
            ProjectionKind::LambertAzimuthal => "lambert-azimuthal",
            ProjectionKind::LambertConformalConic => "lambert-conformal-conic",
            ProjectionKind::AlbersEqualArea => "albers",
            ProjectionKind::Orthographic => "orthographic",
            ProjectionKind::Stereographic => "stereographic",
        }
    }

    /// The kind with this `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }

    /// The name in PROJ.
    pub fn proj_name(self) -> &'static str {
        match self {
            ProjectionKind::Equirectangular => "eqc",
            ProjectionKind::EqualEarth => "eqearth",
            ProjectionKind::Mercator => "merc",
            ProjectionKind::TransverseMercator => "tmerc",
            ProjectionKind::LambertAzimuthal => "laea",
            ProjectionKind::LambertConformalConic => "lcc",
            ProjectionKind::AlbersEqualArea => "aea",
            ProjectionKind::Orthographic => "ortho",
            ProjectionKind::Stereographic => "stere",
        }
    }

    /// The kind with this PROJ name.
    pub fn from_proj_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.proj_name() == name)
    }

    /// True for the three projections that have a latitude of origin at the center of a disk.
    pub fn is_azimuthal(self) -> bool {
        matches!(
            self,
            ProjectionKind::LambertAzimuthal
                | ProjectionKind::Orthographic
                | ProjectionKind::Stereographic
        )
    }

    /// True for the two projections that have standard parallels.
    pub fn is_conic(self) -> bool {
        matches!(
            self,
            ProjectionKind::LambertConformalConic | ProjectionKind::AlbersEqualArea
        )
    }

    /// True for the three projections that have an oblique aspect.
    pub fn has_oblique(self) -> bool {
        matches!(
            self,
            ProjectionKind::Equirectangular | ProjectionKind::EqualEarth | ProjectionKind::Mercator
        )
    }

    /// The spec of the preset of this kind.
    pub fn preset(self) -> ProjectionSpec {
        ProjectionSpec::default().with_kind(self)
    }
}

/// A projection and its parameters, in degrees.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ProjectionSpec {
    pub kind: ProjectionKind,
    /// The central meridian.
    pub lon0: f64,
    /// The latitude of the center. A cylindrical kind uses it only in an oblique aspect.
    pub lat0: f64,
    /// The first standard parallel of a conic kind.
    pub lat1: f64,
    /// The second standard parallel of a conic kind.
    pub lat2: f64,
    /// The angle of the center line of an oblique aspect, counterclockwise from east.
    /// `None` is the normal aspect.
    pub oblique: Option<f64>,
}

impl Default for ProjectionSpec {
    fn default() -> Self {
        ProjectionSpec::new(ProjectionKind::EqualEarth, 0.0, 0.0)
    }
}

/// An error from reading a PROJ string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpecError(pub String);

impl fmt::Display for SpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported PROJ string: {}", self.0)
    }
}

impl std::error::Error for SpecError {}

/// A number for a PROJ string, rounded to 1e-9 degrees.
fn num(x: f64) -> String {
    format!("{}", round9(x))
}

fn round9(x: f64) -> f64 {
    (x * 1e9).round() / 1e9 + 0.0
}

/// The center and the rotation of an oblique aspect come back from the rotation with
/// a small error. A coarser rounding removes it.
fn round7(x: f64) -> f64 {
    (x * 1e7).round() / 1e7 + 0.0
}

/// The southern and northern clip latitude of a conformal cone with these parallels.
pub(crate) fn lcc_lat_range(lat1: f64, lat2: f64) -> (f64, f64) {
    if lat1 + lat2 >= 0.0 {
        (-45.0, 90.0)
    } else {
        (-90.0, 45.0)
    }
}

impl ProjectionSpec {
    /// A spec in the normal aspect with no standard parallels. `normalized` gives a conic kind its parallels.
    pub const fn new(kind: ProjectionKind, lon0: f64, lat0: f64) -> Self {
        ProjectionSpec {
            kind,
            lon0,
            lat0,
            lat1: 0.0,
            lat2: 0.0,
            oblique: None,
        }
    }

    /// True when the projection uses `lat0`.
    pub fn uses_lat0(&self) -> bool {
        !self.kind.has_oblique() || self.oblique.is_some()
    }

    /// This spec in another kind. The center stays where the new kind allows it,
    /// and the other parameters take the values of the preset.
    pub fn with_kind(self, kind: ProjectionKind) -> Self {
        if kind == self.kind {
            return self;
        }
        let keep_lat0 = self.uses_lat0();
        let lat0 = match kind {
            _ if keep_lat0 && !kind.has_oblique() => self.lat0,
            ProjectionKind::Stereographic => 90.0,
            k if k.is_conic() => 40.0,
            _ => 0.0,
        };
        ProjectionSpec::new(kind, self.lon0, lat0).normalized()
    }

    /// Wraps `lon0` into -180..=180, clamps the latitudes, and sets the parameters that
    /// the kind does not use to their defaults.
    pub fn normalized(self) -> Self {
        let mut lon0 = wrap180(self.lon0);
        if lon0 == -180.0 && self.lon0 > 0.0 {
            lon0 = 180.0;
        }
        let kind = self.kind;
        let oblique = if kind.has_oblique() {
            self.oblique.map(|r| {
                let r = wrap180(r);
                if r == -180.0 { 180.0 } else { r }
            })
        } else {
            None
        };
        let (mut lat1, mut lat2) = (0.0, 0.0);
        if kind.is_conic() {
            lat1 = self.lat1.clamp(-89.0, 89.0);
            lat2 = self.lat2.clamp(-89.0, 89.0);
            // A cone needs parallels that are not symmetric about the equator.
            if (lat1 + lat2).abs() < 0.5 {
                let c = if self.lat0.abs() >= 15.0 {
                    self.lat0
                } else if self.lat0 < 0.0 {
                    -40.0
                } else {
                    40.0
                };
                lat1 = (c - 15.0).clamp(-89.0, 89.0);
                lat2 = (c + 15.0).clamp(-89.0, 89.0);
            }
        }
        let lat0 = match kind {
            ProjectionKind::LambertConformalConic => {
                let (lo, hi) = lcc_lat_range(lat1, lat2);
                self.lat0.clamp(lo, hi)
            }
            k if k.has_oblique() => match oblique {
                Some(_) => self.lat0.clamp(-89.0, 89.0),
                None => 0.0,
            },
            _ => self.lat0.clamp(-90.0, 90.0),
        };
        ProjectionSpec {
            kind,
            lon0,
            lat0,
            lat1,
            lat2,
            oblique,
        }
    }

    /// The rotation of the sphere of an oblique aspect, or `None` for the normal aspect.
    pub(crate) fn oblique_frame(&self) -> Option<Frame> {
        let n = self.normalized();
        n.oblique.map(|r| Frame::oblique([n.lon0, n.lat0], r))
    }

    /// The PROJ string of the normalized spec, with no sphere.
    pub fn definition(&self) -> String {
        let n = self.normalized();
        if let Some(frame) = n.oblique_frame() {
            let [lon_0, lat_p, lon_p] = frame.ob_tran();
            return format!(
                "+proj=ob_tran +o_proj={} +o_lat_p={} +o_lon_p={} +lon_0={}",
                n.kind.proj_name(),
                num(lat_p),
                num(lon_p),
                num(lon_0)
            );
        }
        let mut s = format!("+proj={} +lon_0={}", n.kind.proj_name(), num(n.lon0));
        if n.uses_lat0() {
            s.push_str(&format!(" +lat_0={}", num(n.lat0)));
        }
        if n.kind.is_conic() {
            s.push_str(&format!(" +lat_1={} +lat_2={}", num(n.lat1), num(n.lat2)));
        }
        s
    }

    /// The PROJ string of the normalized spec, on a sphere of this radius.
    pub fn proj_string(&self, radius_m: f64) -> String {
        format!("{} +R={radius_m} +no_defs", self.definition())
    }

    /// Reads a PROJ string that [`ProjectionSpec::definition`] or
    /// [`ProjectionSpec::proj_string`] made. The sphere of the string is ignored.
    pub fn from_proj_string(s: &str) -> Result<Self, SpecError> {
        let err = || SpecError(s.to_string());
        let mut proj = None;
        let mut o_proj = None;
        let mut values: [Option<f64>; 7] = [None; 7];
        const KEYS: [&str; 7] = [
            "lon_0", "lat_0", "lat_1", "lat_2", "o_lat_p", "o_lon_p", "R",
        ];
        for token in s.split_whitespace() {
            let token = token.strip_prefix('+').ok_or_else(err)?;
            let (key, value) = token.split_once('=').unwrap_or((token, ""));
            match key {
                "proj" => proj = Some(value),
                "o_proj" => o_proj = Some(value),
                "no_defs" | "type" => {}
                _ => {
                    let i = KEYS.iter().position(|k| *k == key).ok_or_else(err)?;
                    let v: f64 = value.parse().map_err(|_| err())?;
                    if !v.is_finite() {
                        return Err(err());
                    }
                    values[i] = Some(v);
                }
            }
        }
        let [lon_0, lat_0, lat_1, lat_2, o_lat_p, o_lon_p, _] = values;
        let lon_0 = lon_0.unwrap_or(0.0);
        let spec = match proj.ok_or_else(err)? {
            "ob_tran" => {
                let kind = o_proj
                    .and_then(ProjectionKind::from_proj_name)
                    .filter(|k| k.has_oblique())
                    .ok_or_else(err)?;
                let frame =
                    Frame::from_ob_tran(lon_0, o_lat_p.unwrap_or(90.0), o_lon_p.unwrap_or(0.0));
                let (c, r) = frame.center_and_rotation();
                ProjectionSpec {
                    oblique: Some(round7(r)),
                    ..ProjectionSpec::new(kind, round7(c[0]), round7(c[1]))
                }
            }
            name => {
                if o_proj.is_some() || o_lat_p.is_some() || o_lon_p.is_some() {
                    return Err(err());
                }
                let kind = ProjectionKind::from_proj_name(name).ok_or_else(err)?;
                let lat1 = lat_1.unwrap_or(0.0);
                ProjectionSpec {
                    lat1,
                    lat2: lat_2.unwrap_or(lat1),
                    ..ProjectionSpec::new(kind, lon_0, lat_0.unwrap_or(0.0))
                }
            }
        };
        Ok(spec.normalized())
    }
}

/// A projection with a name, which the project saves.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomProjection {
    pub name: String,
    pub spec: ProjectionSpec,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proj_links() {
        proj::Proj::new("+proj=eqearth +R=1").unwrap();
    }

    #[test]
    fn id_round_trip() {
        for k in ProjectionKind::ALL {
            assert_eq!(ProjectionKind::from_id(k.id()), Some(k));
            assert_eq!(ProjectionKind::from_proj_name(k.proj_name()), Some(k));
        }
        assert_eq!(ProjectionKind::from_id("nope"), None);
    }

    #[test]
    fn strings() {
        let s = ProjectionSpec::default();
        assert_eq!(
            s.proj_string(6371000.0),
            "+proj=eqearth +lon_0=0 +R=6371000 +no_defs"
        );
        let o = ProjectionSpec::new(ProjectionKind::Orthographic, 10.0, 45.0);
        assert_eq!(
            o.proj_string(1000.0),
            "+proj=ortho +lon_0=10 +lat_0=45 +R=1000 +no_defs"
        );
        let c = ProjectionSpec {
            lat1: 30.0,
            lat2: 60.0,
            ..ProjectionSpec::new(ProjectionKind::LambertConformalConic, 10.0, 45.0)
        };
        assert_eq!(
            c.definition(),
            "+proj=lcc +lon_0=10 +lat_0=45 +lat_1=30 +lat_2=60"
        );
        let t = ProjectionSpec::new(ProjectionKind::TransverseMercator, -3.0, 0.0);
        assert_eq!(t.definition(), "+proj=tmerc +lon_0=-3 +lat_0=0");
        let b = ProjectionSpec {
            oblique: Some(0.0),
            ..ProjectionSpec::new(ProjectionKind::Mercator, 0.0, 0.0)
        };
        assert_eq!(
            b.definition(),
            "+proj=ob_tran +o_proj=merc +o_lat_p=90 +o_lon_p=0 +lon_0=0"
        );
    }

    fn samples() -> Vec<ProjectionSpec> {
        let mut v = Vec::new();
        for kind in ProjectionKind::ALL {
            for (lon0, lat0) in [(0.0, 0.0), (-73.5, 52.0), (120.0, -35.0)] {
                let s = ProjectionSpec::new(kind, lon0, lat0);
                v.push(s.normalized());
                if kind.is_conic() {
                    v.push(
                        ProjectionSpec {
                            lat1: lat0 - 10.0,
                            lat2: lat0 + 12.5,
                            ..s
                        }
                        .normalized(),
                    );
                }
                if kind.has_oblique() {
                    for r in [0.0, 35.0, -120.0, 90.0] {
                        v.push(
                            ProjectionSpec {
                                oblique: Some(r),
                                ..s
                            }
                            .normalized(),
                        );
                    }
                }
            }
        }
        v
    }

    #[test]
    fn a_definition_reads_back_as_the_same_spec() {
        for s in samples() {
            for text in [s.definition(), s.proj_string(6371000.0)] {
                let back = ProjectionSpec::from_proj_string(&text).unwrap();
                assert_eq!(back.kind, s.kind, "{text}");
                assert_eq!(back.oblique.is_some(), s.oblique.is_some(), "{text}");
                let close = |a: f64, b: f64| wrap180(a - b).abs() < 1e-6;
                assert!(close(back.lon0, s.lon0), "{text}: {back:?} {s:?}");
                assert!(close(back.lat0, s.lat0), "{text}: {back:?} {s:?}");
                assert!(
                    close(back.lat1, s.lat1) && close(back.lat2, s.lat2),
                    "{text}"
                );
                if let (Some(a), Some(b)) = (back.oblique, s.oblique) {
                    assert!(close(a, b), "{text}: {a} {b}");
                }
                assert_eq!(back, s, "{text}");
                assert_eq!(back.definition(), s.definition());
            }
        }
    }

    #[test]
    fn each_definition_is_a_projection_of_proj() {
        for s in samples() {
            proj::Proj::new(&s.proj_string(1000.0)).unwrap_or_else(|e| panic!("{s:?}: {e}"));
        }
    }

    #[test]
    fn other_proj_strings_are_refused() {
        for s in [
            "",
            "+proj=utm +zone=33",
            "+proj=merc +ellps=WGS84",
            "+proj=ob_tran +o_proj=laea +o_lat_p=10",
            "+proj=merc +lon_0=abc",
            "proj=merc",
            "+proj=merc +o_lat_p=10",
        ] {
            assert!(ProjectionSpec::from_proj_string(s).is_err(), "{s}");
        }
    }

    #[test]
    fn normalizing() {
        let a = ProjectionSpec {
            lon0: 190.0,
            ..Default::default()
        };
        assert_eq!(a.normalized().lon0, -170.0);
        let m = ProjectionSpec::new(ProjectionKind::Mercator, 0.0, 40.0);
        assert_eq!(m.normalized().lat0, 0.0);
        let p = ProjectionSpec::new(ProjectionKind::Stereographic, 0.0, 120.0);
        assert_eq!(p.normalized().lat0, 90.0);
        let c = ProjectionSpec::new(ProjectionKind::AlbersEqualArea, 0.0, 50.0).normalized();
        assert_eq!((c.lat1, c.lat2), (35.0, 65.0));
        let flat = ProjectionSpec {
            lat1: -20.0,
            lat2: 20.0,
            ..c
        };
        assert!(flat.normalized().lat1 + flat.normalized().lat2 > 1.0);
        let o = ProjectionSpec {
            oblique: Some(30.0),
            ..ProjectionSpec::new(ProjectionKind::Orthographic, 0.0, 10.0)
        };
        assert_eq!(o.normalized().oblique, None);
        for s in samples() {
            assert_eq!(s.normalized(), s);
        }
    }

    #[test]
    fn a_change_of_kind_keeps_the_center() {
        let o = ProjectionSpec::new(ProjectionKind::Orthographic, 20.0, 50.0);
        let c = o.with_kind(ProjectionKind::LambertConformalConic);
        assert_eq!((c.lon0, c.lat0, c.lat1, c.lat2), (20.0, 50.0, 35.0, 65.0));
        let m = c.with_kind(ProjectionKind::Mercator);
        assert_eq!((m.lon0, m.lat0, m.oblique), (20.0, 0.0, None));
        assert_eq!(m.with_kind(ProjectionKind::Stereographic).lat0, 90.0);
        assert_eq!(ProjectionKind::AlbersEqualArea.preset().lat0, 40.0);
    }
}
