//! Projection presets and the PROJ string that each one makes.

use crate::frame::{oblique_center, oblique_pole};

/// The projections that the app offers.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ProjectionKind {
    Equirectangular,
    EqualEarth,
    Mercator,
    LambertAzimuthal,
    Orthographic,
    Stereographic,
    PolarStereographic,
    LambertConformal,
    AlbersEqualArea,
    TransverseMercator,
}

impl ProjectionKind {
    pub const ALL: [ProjectionKind; 10] = [
        ProjectionKind::Equirectangular,
        ProjectionKind::EqualEarth,
        ProjectionKind::Mercator,
        ProjectionKind::LambertAzimuthal,
        ProjectionKind::Orthographic,
        ProjectionKind::Stereographic,
        ProjectionKind::PolarStereographic,
        ProjectionKind::LambertConformal,
        ProjectionKind::AlbersEqualArea,
        ProjectionKind::TransverseMercator,
    ];

    /// A name for people.
    pub fn name(self) -> &'static str {
        match self {
            ProjectionKind::Equirectangular => "Equirectangular",
            ProjectionKind::EqualEarth => "Equal Earth",
            ProjectionKind::Mercator => "Mercator",
            ProjectionKind::LambertAzimuthal => "Lambert azimuthal",
            ProjectionKind::Orthographic => "Orthographic",
            ProjectionKind::Stereographic => "Stereographic",
            ProjectionKind::PolarStereographic => "Polar stereographic",
            ProjectionKind::LambertConformal => "Lambert conformal conic",
            ProjectionKind::AlbersEqualArea => "Albers equal-area conic",
            ProjectionKind::TransverseMercator => "Transverse Mercator",
        }
    }

    /// A stable name for the command line.
    pub fn id(self) -> &'static str {
        match self {
            ProjectionKind::Equirectangular => "equirectangular",
            ProjectionKind::EqualEarth => "equal-earth",
            ProjectionKind::Mercator => "mercator",
            ProjectionKind::LambertAzimuthal => "lambert-azimuthal",
            ProjectionKind::Orthographic => "orthographic",
            ProjectionKind::Stereographic => "stereographic",
            ProjectionKind::PolarStereographic => "polar-stereographic",
            ProjectionKind::LambertConformal => "lambert-conformal",
            ProjectionKind::AlbersEqualArea => "albers",
            ProjectionKind::TransverseMercator => "transverse-mercator",
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
            ProjectionKind::LambertAzimuthal => "laea",
            ProjectionKind::Orthographic => "ortho",
            ProjectionKind::Stereographic | ProjectionKind::PolarStereographic => "stere",
            ProjectionKind::LambertConformal => "lcc",
            ProjectionKind::AlbersEqualArea => "aea",
            ProjectionKind::TransverseMercator => "tmerc",
        }
    }

    /// True for the projections that have a center on the sphere, and not only a central meridian.
    pub fn is_azimuthal(self) -> bool {
        matches!(
            self,
            ProjectionKind::LambertAzimuthal
                | ProjectionKind::Orthographic
                | ProjectionKind::Stereographic
                | ProjectionKind::PolarStereographic
        )
    }

    /// True for the two conic projections, which have two standard parallels.
    pub fn is_conic(self) -> bool {
        matches!(
            self,
            ProjectionKind::LambertConformal | ProjectionKind::AlbersEqualArea
        )
    }

    /// True for the projections that have a latitude of origin.
    pub fn has_origin_latitude(self) -> bool {
        self.is_azimuthal() || self.is_conic() || self == ProjectionKind::TransverseMercator
    }

    /// True when the projection can have an oblique aspect. An azimuthal
    /// projection is already oblique when its center is not on the equator.
    pub fn can_tilt(self) -> bool {
        !self.is_azimuthal()
    }
}

/// A projection with its center and parameters, in degrees.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ProjectionSpec {
    pub kind: ProjectionKind,
    /// The central meridian, or the longitude of the center.
    pub lon0: f64,
    /// The latitude of origin, or the latitude of the center.
    pub lat0: f64,
    /// The standard parallels of a conic projection.
    pub lat1: f64,
    pub lat2: f64,
    /// An oblique aspect: the azimuth of the center line at the center. The
    /// normal aspect has an azimuth of 90, which is east.
    pub azimuth: Option<f64>,
}

impl Default for ProjectionSpec {
    fn default() -> Self {
        ProjectionSpec::new(ProjectionKind::EqualEarth, 0.0, 0.0)
    }
}

/// The offset of the second parallel when it would mirror the first.
const PARALLEL_GAP: f64 = 1.0;

impl ProjectionSpec {
    /// A projection with the standard parallels of its kind and the normal aspect.
    pub fn new(kind: ProjectionKind, lon0: f64, lat0: f64) -> Self {
        let (lat1, lat2) = if kind.is_conic() {
            (30.0, 60.0)
        } else {
            (0.0, 0.0)
        };
        ProjectionSpec {
            kind,
            lon0,
            lat0,
            lat1,
            lat2,
            azimuth: None,
        }
    }

    /// The same center in another kind. The parallels of a conic projection
    /// stay, and another kind gets the parallels of its preset. The azimuth
    /// stays when the new kind can tilt.
    pub fn with_kind(self, kind: ProjectionKind) -> Self {
        let preset = ProjectionSpec::new(kind, self.lon0, self.lat0);
        let keep = self.kind.is_conic() && kind.is_conic();
        ProjectionSpec {
            lat1: if keep { self.lat1 } else { preset.lat1 },
            lat2: if keep { self.lat2 } else { preset.lat2 },
            azimuth: self.azimuth,
            ..preset
        }
        .normalized()
    }

    /// Wraps `lon0` into -180..=180 and clamps the latitudes. A parameter that
    /// the kind does not use is reset. A polar stereographic projection
    /// centers on the pole that `lat0` is nearer to, and on the north pole at 0.
    pub fn normalized(self) -> Self {
        let wrap = |d: f64| {
            let w = (d + 180.0).rem_euclid(360.0) - 180.0;
            if w == -180.0 && d > 0.0 { 180.0 } else { w }
        };
        let kind = self.kind;
        let azimuth = self.azimuth.filter(|_| kind.can_tilt()).map(wrap);
        let lat0 = if kind == ProjectionKind::PolarStereographic {
            if self.lat0 < 0.0 { -90.0 } else { 90.0 }
        } else if kind.has_origin_latitude() || azimuth.is_some() {
            self.lat0.clamp(-90.0, 90.0)
        } else {
            0.0
        };
        let (mut lat1, mut lat2) = (0.0, 0.0);
        if kind.is_conic() {
            lat1 = self.lat1.clamp(-89.0, 89.0);
            lat2 = self.lat2.clamp(-89.0, 89.0);
            if (lat1 + lat2).abs() < 1e-6 {
                lat2 = if lat1 < 0.0 {
                    lat1 - PARALLEL_GAP
                } else {
                    lat1 + PARALLEL_GAP
                };
                if lat1 == 0.0 {
                    lat2 = PARALLEL_GAP;
                }
            }
        }
        ProjectionSpec {
            kind,
            lon0: wrap(self.lon0),
            lat0,
            lat1,
            lat2,
            azimuth,
        }
    }

    /// The PROJ string of the normalized spec, with no radius. This is the
    /// string that a project stores.
    pub fn to_proj(&self) -> String {
        let n = self.normalized();
        let mut s = match n.azimuth {
            Some(az) => {
                let (lon_0, lat_p, lon_p) = oblique_pole(n.lon0, n.lat0, az);
                format!(
                    "+proj=ob_tran +o_proj={} +lon_0={} +o_lat_p={} +o_lon_p={}",
                    n.kind.proj_name(),
                    lon_0,
                    lat_p,
                    lon_p
                )
            }
            None => {
                let mut s = format!("+proj={} +lon_0={}", n.kind.proj_name(), n.lon0);
                if n.kind.has_origin_latitude() {
                    s.push_str(&format!(" +lat_0={}", n.lat0));
                }
                s
            }
        };
        if n.kind.is_conic() {
            s.push_str(&format!(" +lat_1={} +lat_2={}", n.lat1, n.lat2));
        }
        s
    }

    /// The PROJ string of the normalized spec, on a sphere of this radius.
    pub fn proj_string(&self, radius_m: f64) -> String {
        format!("{} +R={radius_m} +no_defs", self.to_proj())
    }

    /// Reads a string from `to_proj`. A radius and `+no_defs` are skipped.
    /// `None`: the string is not a projection of the app.
    pub fn from_proj(text: &str) -> Option<Self> {
        let mut proj = None;
        let mut o_proj = None;
        let (mut lon0, mut lat0, mut lat1, mut lat2) = (0.0, 0.0, 0.0, 0.0);
        let (mut lat_p, mut lon_p) = (None, None);
        for word in text.split_whitespace() {
            let word = word.strip_prefix('+')?;
            let (key, value) = word.split_once('=').unwrap_or((word, ""));
            let number = || value.parse::<f64>().ok().filter(|v| v.is_finite());
            match key {
                "proj" => proj = Some(value),
                "o_proj" => o_proj = Some(value),
                "lon_0" => lon0 = number()?,
                "lat_0" => lat0 = number()?,
                "lat_1" => lat1 = number()?,
                "lat_2" => lat2 = number()?,
                "o_lat_p" => lat_p = Some(number()?),
                "o_lon_p" => lon_p = Some(number()?),
                "R" | "no_defs" => {}
                _ => return None,
            }
        }
        let name = match (proj?, o_proj) {
            ("ob_tran", Some(name)) => name,
            ("ob_tran", None) => return None,
            (name, None) => name,
            _ => return None,
        };
        let oblique = proj == Some("ob_tran");
        let kind = match name {
            "stere" if oblique => return None,
            "stere" if lat0.abs() == 90.0 => ProjectionKind::PolarStereographic,
            "stere" => ProjectionKind::Stereographic,
            name => ProjectionKind::ALL
                .into_iter()
                .find(|k| k.proj_name() == name && *k != ProjectionKind::PolarStereographic)?,
        };
        let mut spec = ProjectionSpec {
            kind,
            lon0,
            lat0,
            lat1,
            lat2,
            azimuth: None,
        };
        if oblique {
            let (lon_c, lat_c, azimuth) = oblique_center(lon0, lat_p?, lon_p?);
            spec.lon0 = lon_c;
            spec.lat0 = lat_c;
            spec.azimuth = Some(azimuth);
        }
        Some(spec.normalized())
    }
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
    }

    #[test]
    fn proj_strings_round_trip() {
        for kind in ProjectionKind::ALL {
            for azimuth in [None, Some(35.0)] {
                let spec = ProjectionSpec {
                    azimuth,
                    ..ProjectionSpec::new(kind, -20.5, 41.0)
                }
                .normalized();
                let text = spec.to_proj();
                let back = ProjectionSpec::from_proj(&text).unwrap();
                assert_eq!(back.kind, spec.kind, "{text}");
                assert!((back.lon0 - spec.lon0).abs() < 1e-9, "{text} {back:?}");
                assert!((back.lat0 - spec.lat0).abs() < 1e-9, "{text} {back:?}");
                assert_eq!((back.lat1, back.lat2), (spec.lat1, spec.lat2));
                assert_eq!(back.azimuth.is_some(), spec.azimuth.is_some());
                if let (Some(a), Some(b)) = (back.azimuth, spec.azimuth) {
                    assert!((a - b).abs() < 1e-9, "{text} {back:?}");
                }
                let with_radius = spec.proj_string(1000.0);
                assert!(ProjectionSpec::from_proj(&with_radius).is_some());
                proj::Proj::new(&with_radius).unwrap();
            }
        }
    }

    #[test]
    fn foreign_strings() {
        for text in [
            "",
            "+proj=utm +zone=33",
            "+proj=eqearth +lon_0=x",
            "+proj=eqearth +datum=WGS84",
            "+proj=ob_tran +o_proj=eqearth",
            "proj=eqearth",
        ] {
            assert_eq!(ProjectionSpec::from_proj(text), None, "{text}");
        }
    }

    #[test]
    fn conic_parallels() {
        let s = ProjectionSpec::new(ProjectionKind::LambertConformal, 0.0, 40.0);
        assert_eq!((s.lat1, s.lat2), (30.0, 60.0));
        assert_eq!(
            s.to_proj(),
            "+proj=lcc +lon_0=0 +lat_0=40 +lat_1=30 +lat_2=60"
        );
        let mirrored = ProjectionSpec {
            lat1: 20.0,
            lat2: -20.0,
            ..s
        };
        let n = mirrored.normalized();
        assert!((n.lat1 + n.lat2).abs() > 0.5);
        proj::Proj::new(&mirrored.proj_string(1.0)).unwrap();
        // Another kind has no parallels.
        let m = ProjectionSpec {
            lat1: 5.0,
            ..ProjectionSpec::new(ProjectionKind::Mercator, 0.0, 0.0)
        };
        assert_eq!(m.normalized().lat1, 0.0);
    }

    #[test]
    fn polar_stereographic_keeps_a_pole() {
        let n = ProjectionSpec::new(ProjectionKind::PolarStereographic, 30.0, 10.0).normalized();
        assert_eq!(n.lat0, 90.0);
        let s = ProjectionSpec::new(ProjectionKind::PolarStereographic, 30.0, -10.0).normalized();
        assert_eq!(s.to_proj(), "+proj=stere +lon_0=30 +lat_0=-90");
    }

    #[test]
    fn oblique_strings() {
        let o = ProjectionSpec {
            azimuth: Some(45.0),
            ..ProjectionSpec::new(ProjectionKind::EqualEarth, 10.0, 20.0)
        };
        assert!(
            o.to_proj()
                .starts_with("+proj=ob_tran +o_proj=eqearth +lon_0=")
        );
        // The normal aspect is the rotation of nothing.
        let n = ProjectionSpec {
            azimuth: Some(90.0),
            ..ProjectionSpec::new(ProjectionKind::EqualEarth, 10.0, 0.0)
        };
        assert!(n.to_proj().contains("+o_lat_p=90 "), "{}", n.to_proj());
        // An azimuthal projection has no tilt.
        let a = ProjectionSpec {
            azimuth: Some(45.0),
            ..ProjectionSpec::new(ProjectionKind::Orthographic, 10.0, 20.0)
        };
        assert_eq!(a.normalized().azimuth, None);
    }

    #[test]
    fn a_new_kind_gets_its_parallels() {
        let a = ProjectionSpec::new(ProjectionKind::EqualEarth, 10.0, 0.0);
        let l = a.with_kind(ProjectionKind::LambertConformal);
        assert_eq!((l.lon0, l.lat1, l.lat2), (10.0, 30.0, 60.0));
        let custom = ProjectionSpec {
            lat1: 40.0,
            lat2: 50.0,
            ..l
        };
        let b = custom.with_kind(ProjectionKind::AlbersEqualArea);
        assert_eq!((b.lat1, b.lat2), (40.0, 50.0));
        let m = b.with_kind(ProjectionKind::Mercator);
        assert_eq!((m.lat1, m.lat2), (0.0, 0.0));
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
    }
}
