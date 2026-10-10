//! Projection presets and the PROJ string that each one makes.

/// The projections that the app offers.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ProjectionKind {
    Equirectangular,
    EqualEarth,
    Mercator,
    LambertAzimuthal,
    Orthographic,
    Stereographic,
}

impl ProjectionKind {
    pub const ALL: [ProjectionKind; 6] = [
        ProjectionKind::Equirectangular,
        ProjectionKind::EqualEarth,
        ProjectionKind::Mercator,
        ProjectionKind::LambertAzimuthal,
        ProjectionKind::Orthographic,
        ProjectionKind::Stereographic,
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
            ProjectionKind::Stereographic => "stere",
        }
    }

    /// True for the three projections that have a latitude of origin.
    pub fn is_azimuthal(self) -> bool {
        matches!(
            self,
            ProjectionKind::LambertAzimuthal
                | ProjectionKind::Orthographic
                | ProjectionKind::Stereographic
        )
    }
}

/// A projection and its center, in degrees.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ProjectionSpec {
    pub kind: ProjectionKind,
    pub lon0: f64,
    pub lat0: f64,
}

impl Default for ProjectionSpec {
    fn default() -> Self {
        ProjectionSpec {
            kind: ProjectionKind::EqualEarth,
            lon0: 0.0,
            lat0: 0.0,
        }
    }
}

impl ProjectionSpec {
    /// Wraps `lon0` into -180..=180, clamps `lat0`, and sets `lat0` to 0 for a kind that is not azimuthal.
    pub fn normalized(self) -> Self {
        let mut lon0 = (self.lon0 + 180.0).rem_euclid(360.0) - 180.0;
        if lon0 == -180.0 && self.lon0 > 0.0 {
            lon0 = 180.0;
        }
        let lat0 = if self.kind.is_azimuthal() {
            self.lat0.clamp(-90.0, 90.0)
        } else {
            0.0
        };
        ProjectionSpec {
            kind: self.kind,
            lon0,
            lat0,
        }
    }

    /// The PROJ string of the normalized spec, on a sphere of this radius.
    pub fn proj_string(&self, radius_m: f64) -> String {
        let n = self.normalized();
        let mut s = format!("+proj={} +lon_0={}", n.kind.proj_name(), n.lon0);
        if n.kind.is_azimuthal() {
            s.push_str(&format!(" +lat_0={}", n.lat0));
        }
        s.push_str(&format!(" +R={radius_m} +no_defs"));
        s
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
        let o = ProjectionSpec {
            kind: ProjectionKind::Orthographic,
            lon0: 10.0,
            lat0: 45.0,
        };
        assert_eq!(
            o.proj_string(1000.0),
            "+proj=ortho +lon_0=10 +lat_0=45 +R=1000 +no_defs"
        );
    }

    #[test]
    fn normalizing() {
        let a = ProjectionSpec {
            lon0: 190.0,
            ..Default::default()
        };
        assert_eq!(a.normalized().lon0, -170.0);
        let m = ProjectionSpec {
            kind: ProjectionKind::Mercator,
            lon0: 0.0,
            lat0: 40.0,
        };
        assert_eq!(m.normalized().lat0, 0.0);
        let p = ProjectionSpec {
            kind: ProjectionKind::Stereographic,
            lon0: 0.0,
            lat0: 120.0,
        };
        assert_eq!(p.normalized().lat0, 90.0);
    }
}
