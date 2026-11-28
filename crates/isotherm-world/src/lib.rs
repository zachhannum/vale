//! The planet record and the terrain of an Isotherm world.

pub use vale_terrain::Heightmap;

/// The bulk properties of a planet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Planet {
    /// The mean radius, in meters.
    pub radius_m: f64,
    /// The surface gravity, in m/s².
    pub gravity: f64,
    /// The length of the solar day, in seconds.
    pub day_s: f64,
    /// The angle between the axis of rotation and the normal of the orbit, in degrees.
    pub tilt_deg: f64,
}

impl Planet {
    pub const EARTH: Planet = Planet {
        radius_m: 6_371_000.0,
        gravity: 9.81,
        day_s: 86_400.0,
        tilt_deg: 23.44,
    };
}

/// A planet and its heightmap on a cube map.
pub struct World {
    pub planet: Planet,
    pub heights: Heightmap,
}

impl World {
    /// A world with the whole surface at sea level.
    pub fn flat(planet: Planet, face_size: usize) -> World {
        World {
            planet,
            heights: Heightmap::new(face_size, vale_terrain::meters_to_level(0.0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_world_is_at_sea_level() {
        let world = World::flat(Planet::EARTH, 8);
        assert_eq!(world.heights.face_size(), 8);
        let level = world.heights.get(0, 3, 3);
        assert!(vale_terrain::level_to_meters(level).abs() < 1.0);
    }
}
