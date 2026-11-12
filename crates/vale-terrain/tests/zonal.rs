//! The values of a heightmap under points, polygons, and lines.

use std::time::Instant;

use vale_terrain::math::{V3, cross, dot, lonlat_to_dir};
use vale_terrain::{
    FACES, Heightmap, TexelRect, face_dir, line_zonal, polygon_zonal, rects_meet, unwarp,
};

/// The direction of texel coordinates on a face, with texel centers at whole
/// numbers.
fn dir_at(n: usize, face: usize, fx: f64, fy: f64) -> V3 {
    let flat = |f: f64| unwarp((f + 0.5) / n as f64 * 2.0 - 1.0);
    face_dir(face, flat(fx), flat(fy))
}

/// The area of a ring on the unit sphere, in steradians, from a fan of
/// triangles. The ring must be smaller than a hemisphere.
fn ring_area(ring: &[[f64; 2]]) -> f64 {
    let d: Vec<V3> = ring.iter().map(|p| lonlat_to_dir(p[0], p[1])).collect();
    let mut sum = 0.0;
    for i in 1..d.len() - 1 {
        let (a, b, c) = (d[0], d[i], d[i + 1]);
        let num = dot(a, cross(b, c));
        let den = 1.0 + dot(a, b) + dot(b, c) + dot(c, a);
        sum += 2.0 * num.atan2(den);
    }
    sum.abs()
}

fn square(lon0: f64, lat0: f64, lon1: f64, lat1: f64) -> Vec<[f64; 2]> {
    vec![[lon0, lat0], [lon1, lat0], [lon1, lat1], [lon0, lat1]]
}

#[test]
fn bilinear_is_linear_between_texel_centers() {
    let n = 64;
    let mut map = Heightmap::new(n, 0);
    for y in 0..n {
        for x in 0..n {
            map.set(0, x, y, (100 * x + 37 * y) as u16);
        }
    }
    for &(fx, fy) in &[(10.3, 20.7), (0.0, 0.0), (31.5, 40.25), (62.9, 1.1)] {
        let got = map.sample_bilinear(dir_at(n, 0, fx, fy));
        let want = 100.0 * fx + 37.0 * fy;
        assert!((got - want).abs() < 1e-6, "{fx},{fy}: {got} != {want}");
    }
    // At a texel center, the value is the texel.
    assert_eq!(map.sample_bilinear(dir_at(n, 0, 5.0, 6.0)), 722.0);
}

#[test]
fn bilinear_samples_are_fast() {
    let map = Heightmap::new(2048, 1000);
    let start = Instant::now();
    let mut sum = 0.0;
    for i in 0..50_000 {
        let t = i as f64;
        sum += map.sample_bilinear(lonlat_to_dir(
            t * 0.0137 % 360.0 - 180.0,
            t * 0.0071 % 170.0 - 85.0,
        ));
    }
    assert_eq!(sum, 50_000.0 * 1000.0);
    assert!(start.elapsed().as_secs_f64() < 1.0);
}

#[test]
fn the_polygon_mean_is_weighted_by_area() {
    let n = 64;
    let mut map = Heightmap::new(n, 1000);
    let band = |x: usize| !(4..n - 4).contains(&x);
    for y in 0..n {
        for x in 0..n {
            if band(x) || band(y) {
                map.set(0, x, y, 60_000);
            }
        }
    }
    // The +X face: its corners are at longitude 45 and latitude 35.26.
    let lat = (1.0 / 3f64.sqrt()).asin().to_degrees();
    let zonal = polygon_zonal(&map, &[square(-45.0, -lat, 45.0, lat)]).unwrap();

    let (mut sum, mut weight, mut plain) = (0.0, 0.0, 0.0);
    for y in 0..n {
        for x in 0..n {
            let v = if band(x) || band(y) { 60_000.0 } else { 1000.0 };
            let w = map.texel_solid_angle(x, y);
            sum += w * v;
            weight += w;
            plain += v;
        }
    }
    let (want, plain) = (sum / weight, plain / (n * n) as f64);
    assert!((zonal.mean - want).abs() < 1e-6, "{} != {want}", zonal.mean);
    assert!((zonal.weight - weight).abs() < 1e-9);
    assert!((want - plain).abs() > 100.0);
    assert_eq!((zonal.min, zonal.max), (1000.0, 60_000.0));
    let face = TexelRect {
        x0: 0,
        y0: 0,
        x1: n,
        y1: n,
    };
    assert_eq!(zonal.rects, [Some(face), None, None, None, None, None]);
}

#[test]
fn a_polygon_with_no_texel_center_gives_none() {
    let map = Heightmap::new(64, 1000);
    assert_eq!(polygon_zonal(&map, &[square(0.0, 0.0, 0.01, 0.01)]), None);
    assert_eq!(polygon_zonal(&map, &[]), None);
}

/// Checks the mean and the area of a polygon on a constant map.
fn check_area(rings: &[Vec<[f64; 2]>], area: f64) {
    let map = Heightmap::new(256, 5000);
    let zonal = polygon_zonal(&map, rings).unwrap();
    assert!((zonal.mean - 5000.0).abs() < 1e-9, "{}", zonal.mean);
    assert_eq!((zonal.min, zonal.max), (5000.0, 5000.0));
    let error = (zonal.weight - area).abs() / area;
    assert!(error < 0.03, "{} != {area}", zonal.weight);
}

#[test]
fn a_polygon_around_a_pole() {
    for lat in [80.0, -60.0] {
        let ring: Vec<[f64; 2]> = (0..36).map(|i| [i as f64 * 10.0 - 180.0, lat]).collect();
        let area = ring_area(&ring);
        check_area(&[ring], area);
    }
}

#[test]
fn a_polygon_across_face_edges() {
    // Across the edge of the +X face and the +Y face.
    let ring = square(40.0, -5.0, 50.0, 5.0);
    check_area(std::slice::from_ref(&ring), ring_area(&ring));
    // Across the edges of five faces.
    let ring = square(-60.0, -50.0, 60.0, 50.0);
    check_area(std::slice::from_ref(&ring), ring_area(&ring));
    // Across the antimeridian and a cube corner.
    let ring = square(170.0, 30.0, 200.0, 40.0);
    check_area(std::slice::from_ref(&ring), ring_area(&ring));
}

#[test]
fn a_hole_is_not_inside() {
    let (outer, hole) = (
        square(-20.0, -20.0, 20.0, 20.0),
        square(-5.0, -5.0, 5.0, 5.0),
    );
    check_area(
        &[outer.clone(), hole.clone()],
        ring_area(&outer) - ring_area(&hole),
    );

    let mut map = Heightmap::new(256, 5000);
    map.set(0, 128, 128, 9000);
    let zonal = polygon_zonal(&map, &[outer, hole]).unwrap();
    assert_eq!(zonal.max, 5000.0);
}

#[test]
fn the_line_values() {
    let n = 64;
    let mut map = Heightmap::new(n, 0);
    for y in 0..n {
        for x in 0..n {
            map.set(0, x, y, (100 * x) as u16);
        }
    }
    // On the equator of the +X face, the texel coordinate is linear in the
    // longitude.
    let level = |lon: f64| 100.0 * ((lon / 45.0 + 1.0) * 0.5 * n as f64 - 0.5);
    let zonal = line_zonal(&map, &[[-10.0, 0.0], [0.0, 0.0], [10.0, 0.0]]).unwrap();
    assert!((zonal.mean - level(0.0)).abs() < 1e-6, "{}", zonal.mean);
    assert!((zonal.min - level(-10.0)).abs() < 1e-6);
    assert!((zonal.max - level(10.0)).abs() < 1e-6);
    assert!((zonal.weight - 20f64.to_radians()).abs() < 1e-12);
    assert!(zonal.rects[0].is_some());
    assert!(zonal.rects[1..].iter().all(Option::is_none));

    let point = line_zonal(&map, &[[0.0, 0.0]]).unwrap();
    assert!((point.mean - level(0.0)).abs() < 1e-6);
    assert_eq!(point.weight, 0.0);
    assert_eq!(line_zonal(&map, &[]), None);
}

#[test]
fn the_changed_tiles_after_a_snapshot() {
    let mut map = Heightmap::new(512, 1000);
    map.set(2, 10, 10, 3000);
    let old = map.snapshot();
    assert_eq!(map.changed_tiles(&old), Some(vec![]));

    map.set(1, 300, 10, 7);
    let tile = TexelRect {
        x0: 256,
        y0: 0,
        x1: 512,
        y1: 256,
    };
    let changed = map.changed_tiles(&old).unwrap();
    assert_eq!(changed, vec![(1, tile)]);

    // A write to a tile that the snapshot shares copies the tile.
    map.set(2, 11, 10, 3000);
    assert_eq!(map.changed_tiles(&old).unwrap().len(), 2);

    let mut rects = [None; FACES];
    rects[1] = Some(TexelRect {
        x0: 400,
        y0: 200,
        x1: 410,
        y1: 210,
    });
    assert!(rects_meet(&rects, &changed));
    rects[1] = Some(TexelRect {
        x0: 0,
        y0: 0,
        x1: 256,
        y1: 256,
    });
    assert!(!rects_meet(&rects, &changed));

    map.fill(5);
    assert_eq!(map.changed_tiles(&old), None);
}
