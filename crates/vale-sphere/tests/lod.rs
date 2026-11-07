use std::time::Instant;

use vale_sphere::LonLat;
use vale_sphere::lod::*;

type V3 = [f64; 3];

fn unit(p: LonLat) -> V3 {
    let (lon, lat) = (p[0].to_radians(), p[1].to_radians());
    [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()]
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}

fn angle(a: V3, b: V3) -> f64 {
    norm(cross(a, b)).atan2(dot(a, b))
}

/// The angle from `p` to the great-circle segment from `a` to `b`.
fn seg_dist(p: V3, a: V3, b: V3) -> f64 {
    let n = cross(a, b);
    let nn = norm(n);
    if nn > 1e-15 {
        let n = [n[0] / nn, n[1] / nn, n[2] / nn];
        let h = dot(p, n);
        let q = [p[0] - h * n[0], p[1] - h * n[1], p[2] - h * n[2]];
        let ql = norm(q);
        if ql > 0.0 {
            let q = [q[0] / ql, q[1] / ql, q[2] / ql];
            if (angle(a, q) + angle(q, b) - angle(a, b)).abs() < 1e-12 {
                return h.abs().asin();
            }
        }
    }
    angle(p, a).min(angle(p, b))
}

/// Points are equal when they are the same point on the sphere.
fn same(a: LonLat, b: LonLat) -> bool {
    angle(unit(a), unit(b)) < 1e-12
}

/// Whether two great-circle segments shorter than 90° cross at a point
/// inside both. The test is independent of the library.
fn segs_cross(a: V3, b: V3, c: V3, d: V3) -> bool {
    let s = |x: V3, y: V3, z: V3| dot(cross(x, y), z);
    let (c1, c2) = (s(a, b, c), s(a, b, d));
    let (c3, c4) = (s(c, d, a), s(c, d, b));
    let eps = 1e-15;
    c1 * c2 < -eps * eps && c3 * c4 < -eps * eps && dot(a, c) > 0.0
}

/// The segments of a ring, as unit vectors, with the closing segment.
fn ring_segs(ring: &[LonLat]) -> Vec<(V3, V3)> {
    let mut r = ring.to_vec();
    if r.len() > 1 && r[0] == r[r.len() - 1] {
        r.pop();
    }
    (0..r.len())
        .map(|i| (unit(r[i]), unit(r[(i + 1) % r.len()])))
        .collect()
}

/// Brute force with a sort on x: a pair of segments that cross, if any.
fn any_crossing(rings: &[Vec<LonLat>]) -> Option<(usize, usize)> {
    let mut segs: Vec<(f64, f64, V3, V3, usize)> = Vec::new();
    for (r, ring) in rings.iter().enumerate() {
        for (a, b) in ring_segs(ring) {
            segs.push((a[0].min(b[0]), a[0].max(b[0]), a, b, r));
        }
    }
    segs.sort_by(|x, y| x.0.total_cmp(&y.0));
    for i in 0..segs.len() {
        for j in i + 1..segs.len() {
            if segs[j].0 > segs[i].1 + 1e-9 {
                break;
            }
            let (a, b, c, d) = (segs[i].2, segs[i].3, segs[j].2, segs[j].3);
            let shared = [(a, c), (a, d), (b, c), (b, d)]
                .iter()
                .any(|&(x, y)| angle(x, y) < 1e-12);
            if !shared && segs_cross(a, b, c, d) {
                return Some((segs[i].4, segs[j].4));
            }
        }
    }
    None
}

/// Deterministic noise in -1..1.
fn noise(seed: u64) -> f64 {
    let mut x = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0x2545_f491_4f6c_dd1d;
    x ^= x >> 31;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 29;
    (x >> 11) as f64 / (1u64 << 52) as f64 - 1.0
}

/// The points of a jagged edge from `a` to `b`, without `b`. The noise is
/// across the edge and goes to 0 at both ends.
fn jagged(a: LonLat, b: LonLat, n: usize, amp: f64, seed: u64) -> Vec<LonLat> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l = (dx * dx + dy * dy).sqrt();
    let (nx, ny) = (-dy / l, dx / l);
    (0..n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let o = if i == 0 {
                0.0
            } else {
                amp * (std::f64::consts::PI * t).sin() * noise(seed * 1000 + i as u64)
            };
            [a[0] + dx * t + nx * o, a[1] + dy * t + ny * o]
        })
        .collect()
}

/// A grid of cells with jagged shared edges. Each edge has one seed, so two
/// neighbors get the same points.
struct Mosaic {
    nx: usize,
    ny: usize,
    cell: f64,
    origin: LonLat,
    per_edge: usize,
}

impl Mosaic {
    fn corner(&self, i: usize, j: usize) -> LonLat {
        [
            self.origin[0] + i as f64 * self.cell,
            self.origin[1] + j as f64 * self.cell,
        ]
    }

    /// The points of the edge from corner `p` to corner `q`, without `q`.
    fn edge(&self, p: (usize, usize), q: (usize, usize)) -> Vec<LonLat> {
        let (lo, hi, rev) = if p < q { (p, q, false) } else { (q, p, true) };
        let horizontal = lo.1 == hi.1;
        let seed = ((lo.0 * 1000 + lo.1) * 2 + horizontal as usize) as u64;
        let mut pts = jagged(
            self.corner(lo.0, lo.1),
            self.corner(hi.0, hi.1),
            self.per_edge,
            0.15 * self.cell,
            seed,
        );
        if rev {
            pts.push(self.corner(hi.0, hi.1));
            pts.reverse();
            pts.pop();
        }
        pts
    }

    fn ring(&self, i: usize, j: usize, closed: bool) -> Vec<LonLat> {
        let c = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
        let mut r: Vec<LonLat> = (0..4)
            .flat_map(|k| self.edge(c[k], c[(k + 1) % 4]))
            .collect();
        if closed {
            r.push(r[0]);
        }
        r
    }

    fn polygons(&self) -> Vec<Vec<Vec<LonLat>>> {
        let mut out = Vec::new();
        for j in 0..self.ny {
            for i in 0..self.nx {
                out.push(vec![self.ring(i, j, (i + j) % 2 == 0)]);
            }
        }
        out
    }
}

fn levels(shapes: &[Shape]) -> Vec<Level> {
    let mut out = Vec::new();
    build(shapes, |l| out.push(l));
    assert_eq!(out.len(), LEVELS);
    for (i, l) in out.iter().enumerate() {
        assert_eq!(l.index, i);
        assert_eq!(l.features.len(), shapes.len());
    }
    out
}

/// The rings of a feature at a level, with the full rings for `Same`.
fn rings_of(simple: &Simple, full: &[Vec<Vec<LonLat>>]) -> Vec<Vec<Vec<LonLat>>> {
    match simple {
        Simple::Same => full.to_vec(),
        Simple::Gone => Vec::new(),
        Simple::Polygons(p) => p.clone(),
        Simple::Lines(_) => panic!("lines for a polygon"),
    }
}

/// The part of a ring from point `p` to point `q`, in ring order.
fn run(ring: &[LonLat], p: LonLat, q: LonLat) -> Vec<LonLat> {
    let mut r = ring.to_vec();
    if r[0] == r[r.len() - 1] {
        r.pop();
    }
    let n = r.len();
    let s = r.iter().position(|&x| same(x, p)).expect("start corner");
    let mut out = vec![r[s]];
    for k in 1..=n {
        let x = r[(s + k) % n];
        out.push(x);
        if same(x, q) {
            return out;
        }
    }
    panic!("no end corner");
}

fn assert_same_run(a: &[LonLat], b: &[LonLat]) {
    assert_eq!(a.len(), b.len(), "{a:?} {b:?}");
    for (x, y) in a.iter().zip(b.iter().rev()) {
        assert!(same(*x, *y), "{x:?} {y:?}");
    }
}

fn all_rings(level: &Level, full: &[Vec<Vec<Vec<LonLat>>>]) -> Vec<Vec<LonLat>> {
    level
        .features
        .iter()
        .zip(full)
        .flat_map(|(s, f)| rings_of(s, f).into_iter().flatten())
        .collect()
}

/// The largest distance from a point of `fine` to the ring `coarse`.
fn ring_hausdorff(fine: &[LonLat], coarse: &[LonLat]) -> f64 {
    let segs = ring_segs(coarse);
    fine.iter()
        .map(|&p| {
            let p = unit(p);
            segs.iter()
                .map(|&(a, b)| seg_dist(p, a, b))
                .fold(f64::INFINITY, f64::min)
        })
        .fold(0.0, f64::max)
}

fn assert_cascade(full: &[Vec<Vec<Vec<LonLat>>>], levels: &[Level]) {
    let mut finer: Vec<Vec<LonLat>> = full.iter().flatten().flatten().cloned().collect();
    for level in levels {
        let coarse = all_rings(level, full);
        assert_eq!(coarse.len(), finer.len());
        for (f, c) in finer.iter().zip(&coarse) {
            let d = ring_hausdorff(f, c);
            assert!(
                d <= level.tolerance * (1.0 + 1e-9),
                "level {} moves {d}",
                level.index
            );
        }
        finer = coarse;
    }
}

fn mosaic() -> (Mosaic, Vec<Vec<Vec<LonLat>>>) {
    let m = Mosaic {
        nx: 20,
        ny: 20,
        cell: 1.0,
        origin: [10.0, 20.0],
        per_edge: 25,
    };
    let p = m.polygons();
    (m, p)
}

#[test]
fn two_polygons_share_a_jagged_edge_at_each_level() {
    let (a, b, c, d, e, f) = (
        [0.0, 0.0],
        [2.0, 0.0],
        [2.0, 2.0],
        [0.0, 2.0],
        [4.0, 0.0],
        [4.0, 2.0],
    );
    let shared = jagged(b, c, 60, 0.3, 7);
    let mut left = vec![a];
    left.extend(shared.iter().cloned());
    left.extend([c, d]);
    let mut back: Vec<LonLat> = shared.iter().skip(1).cloned().collect();
    back.reverse();
    let mut right = vec![b, e, f, c];
    right.extend(back);
    let full = [vec![left.clone()], vec![right.clone()]];
    let shapes = [Shape::Polygons(&full[0..1]), Shape::Polygons(&full[1..2])];
    let ls = levels(&shapes);
    let fulls = vec![vec![vec![left]], vec![vec![right]]];
    for l in &ls {
        let rings = all_rings(l, &fulls);
        assert_same_run(&run(&rings[0], b, c), &run(&rings[1], c, b));
    }
    assert!(matches!(ls[3].features[0], Simple::Polygons(_)));
    assert_cascade(&fulls, &ls);
}

#[test]
fn a_mosaic_shares_each_edge_and_has_no_crossing() {
    let (m, polys) = mosaic();
    let shapes: Vec<Shape> = polys
        .iter()
        .map(|p| Shape::Polygons(std::slice::from_ref(p)))
        .collect();
    let fulls: Vec<Vec<Vec<Vec<LonLat>>>> = polys.iter().map(|p| vec![p.clone()]).collect();
    let ls = levels(&shapes);
    for l in &ls {
        let rings = all_rings(l, &fulls);
        assert_eq!(rings.len(), m.nx * m.ny);
        for j in 0..m.ny {
            for i in 0..m.nx {
                let r = &rings[j * m.nx + i];
                if i + 1 < m.nx {
                    let (p, q) = (m.corner(i + 1, j), m.corner(i + 1, j + 1));
                    assert_same_run(&run(r, p, q), &run(&rings[j * m.nx + i + 1], q, p));
                }
                if j + 1 < m.ny {
                    let (p, q) = (m.corner(i + 1, j + 1), m.corner(i, j + 1));
                    assert_same_run(&run(r, p, q), &run(&rings[(j + 1) * m.nx + i], q, p));
                }
            }
        }
        assert_eq!(any_crossing(&rings), None, "level {}", l.index);
    }
    // The closure of each ring follows the input.
    for (s, p) in ls[3].features.iter().zip(&polys) {
        let r = &rings_of(s, std::slice::from_ref(p))[0][0];
        assert_eq!(r[0] == r[r.len() - 1], p[0][0] == p[0][p[0].len() - 1]);
    }
    assert_cascade(&fulls, &ls);
}

/// The polygon `0..10` by `0..10` with an outward bump in its bottom edge
/// and a narrow spike from its top edge into the bump, and islands in
/// notches of its right edge. A plain simplification at 64 km crosses both.
fn comb() -> Vec<Vec<Vec<LonLat>>> {
    let mut outer: Vec<LonLat> = vec![[0.0, 0.0]];
    for k in 1..20 {
        let x = k as f64 * 0.5;
        outer.push([x, -0.4 * (std::f64::consts::PI * x / 10.0).sin()]);
    }
    outer.push([10.0, 0.0]);
    let mut islands = Vec::new();
    for k in 0..4 {
        let y = 1.0 + 2.0 * k as f64;
        // A notch into the polygon, 0.4° deep and 1.6° wide.
        outer.extend([[10.0, y], [9.6, y + 0.8], [10.0, y + 1.6]]);
        islands.push(vec![vec![
            [9.95, y + 0.3],
            [10.6, y + 0.8],
            [9.95, y + 1.3],
            [9.7, y + 0.8],
        ]]);
    }
    outer.extend([
        [10.0, 10.0],
        [5.05, 10.0],
        [5.0, -0.2],
        [4.95, 10.0],
        [0.0, 10.0],
    ]);
    let mut all = vec![vec![outer]];
    all.extend(islands);
    all
}

#[test]
fn a_comb_has_no_crossing_at_any_level() {
    let full = comb();
    let shapes: Vec<Shape> = full
        .iter()
        .map(|p| Shape::Polygons(std::slice::from_ref(p)))
        .collect();
    let fulls: Vec<Vec<Vec<Vec<LonLat>>>> = full.iter().map(|p| vec![p.clone()]).collect();
    assert_eq!(
        any_crossing(
            &fulls
                .iter()
                .flatten()
                .flatten()
                .cloned()
                .collect::<Vec<_>>()
        ),
        None
    );
    let ls = levels(&shapes);
    for l in &ls {
        let rings = all_rings(l, &fulls);
        assert_eq!(rings.len(), full.len());
        assert_eq!(any_crossing(&rings), None, "level {}", l.index);
    }
    // At 64 km the bump and the notches are within the tolerance, but the
    // level keeps points of them where a straight edge would cross.
    let outer = &all_rings(&ls[3], &fulls)[0];
    assert!(outer.iter().any(|p| p[1] < -0.2), "{outer:?}");
    assert!(outer.iter().any(|p| p[0] == 9.6), "{outer:?}");
    assert_cascade(&fulls, &ls);
}

#[test]
fn choose_picks_the_coarsest_level_below_a_pixel() {
    let all = [true; LEVELS];
    assert_eq!(choose(tolerance(0) * 0.5, all), None);
    assert_eq!(choose(tolerance(0), all), None);
    assert_eq!(choose(tolerance(0) * 1.5, all), Some(0));
    assert_eq!(choose(tolerance(2) * 1.01, all), Some(2));
    assert_eq!(choose(1.0, all), Some(3));
    assert_eq!(choose(1.0, [true, true, false, false]), Some(1));
    assert_eq!(
        choose(tolerance(2) * 2.0, [false, false, false, true]),
        None
    );
    assert_eq!(choose(1.0, [false; LEVELS]), None);
    assert!((tolerance(3) - 64.0 / 6371.0).abs() < 1e-15);
}

fn square(c: LonLat, half: f64) -> Vec<LonLat> {
    vec![
        [c[0] - half, c[1] - half],
        [c[0] + half, c[1] - half],
        [c[0] + half, c[1] + half],
        [c[0] - half, c[1] + half],
    ]
}

#[test]
fn small_polygons_and_holes_go_at_coarse_levels() {
    // The hole is about 47 km across and the small polygon about 3 km.
    let big = vec![square([0.0, 0.0], 5.0), square([1.0, 1.0], 0.15)];
    let small = vec![square([20.0, 0.0], 0.01)];
    let polys = [big.clone(), small];
    let shapes = [Shape::Polygons(&polys[0..1]), Shape::Polygons(&polys[1..2])];
    let ls = levels(&shapes);
    for l in &ls[..3] {
        assert_eq!(l.features[0], Simple::Same, "level {}", l.index);
    }
    assert_eq!(
        ls[3].features[0],
        Simple::Polygons(vec![vec![big[0].clone()]])
    );
    assert_eq!(ls[0].features[1], Simple::Same);
    for l in &ls[1..] {
        assert_eq!(l.features[1], Simple::Gone);
    }
}

fn circle(c: LonLat, r: f64, n: usize, closed: bool) -> Vec<LonLat> {
    let mut v: Vec<LonLat> = (0..n)
        .map(|i| {
            let t = i as f64 / n as f64 * std::f64::consts::TAU;
            let lon = c[0] + r * t.cos() + 0.02 * noise(i as u64);
            [
                ((lon + 180.0).rem_euclid(360.0)) - 180.0,
                c[1] + r * t.sin(),
            ]
        })
        .collect();
    if closed {
        v.push(v[0]);
    }
    v
}

fn single_ring(s: &Simple) -> Vec<LonLat> {
    match s {
        Simple::Polygons(p) => p[0][0].clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_island_keeps_its_closure_and_its_points() {
    for closed in [false, true] {
        let ring = circle([30.0, 10.0], 2.0, 400, closed);
        let polys = [vec![ring.clone()]];
        let ls = levels(&[Shape::Polygons(&polys)]);
        for l in &ls {
            let r = single_ring(&l.features[0]);
            assert_eq!(r[0] == r[r.len() - 1], closed);
            assert!(r.len() >= if closed { 4 } else { 3 });
            assert!(r.iter().all(|p| ring.contains(p)));
            // The levels move the ring by at most 1 + 1/4 + 1/16 + ... of the tolerance.
            assert!(ring_hausdorff(&ring, &r) <= l.tolerance * 4.0 / 3.0);
        }
        assert!(single_ring(&ls[3].features[0]).len() < 100);
    }
}

#[test]
fn a_ring_across_the_antimeridian_keeps_short_steps() {
    let ring = circle([180.0, 0.0], 5.0, 500, true);
    let around: Vec<LonLat> = (0..720).map(|i| [-180.0 + i as f64 * 0.5, 60.0]).collect();
    let polys = [vec![ring.clone()], vec![around.clone()]];
    let ls = levels(&[Shape::Polygons(&polys[0..1]), Shape::Polygons(&polys[1..2])]);
    for l in &ls {
        for (s, src) in l.features.iter().zip([&ring, &around]) {
            let r = single_ring(s);
            assert!(r.iter().all(|p| src.contains(p)));
            for w in ring_segs(&r) {
                let lon = |v: V3| v[1].atan2(v[0]).to_degrees();
                let d = (lon(w.0) - lon(w.1)).abs();
                assert!(d.min(360.0 - d) <= 45.0 + 1e-9, "{d}");
            }
        }
        assert!(single_ring(&l.features[1]).len() >= 8);
        assert!(single_ring(&l.features[1]).len() < 720);
    }
    assert_cascade(&[vec![vec![ring]], vec![vec![around]]], &ls);
}

#[test]
fn lines_and_points_simplify_or_stay() {
    let line: Vec<LonLat> = (0..200)
        .map(|i| [i as f64 * 0.05, 0.1 * noise(i)])
        .collect();
    let short = vec![[50.0, 0.0], [50.01, 0.02], [50.02, 0.0]];
    let lines = [line.clone(), short.clone()];
    let ls = levels(&[
        Shape::Points,
        Shape::Lines(&lines[..1]),
        Shape::Lines(&lines),
    ]);
    for l in &ls {
        assert_eq!(l.features[0], Simple::Same);
    }
    match &ls[3].features[1] {
        Simple::Lines(v) => {
            assert_eq!(v.len(), 1);
            assert_eq!(v[0][0], line[0]);
            assert_eq!(v[0][v[0].len() - 1], line[199]);
            assert!(v[0].len() < 100);
        }
        other => panic!("{other:?}"),
    }
    // The 2 km line goes at 4 km.
    match &ls[1].features[2] {
        Simple::Lines(v) => assert_eq!(v.len(), 1),
        other => panic!("{other:?}"),
    }
    let s = levels(&[Shape::Lines(&lines[1..])]);
    assert_eq!(s[0].features[0], Simple::Same);
    assert_eq!(s[1].features[0], Simple::Gone);
}

#[test]
fn edges_at_plus_and_minus_180_join() {
    let edge = [
        [180.0, -10.0],
        [179.7, -5.0],
        [180.0, 0.0],
        [179.6, 5.0],
        [180.0, 10.0],
    ];
    let mut west = vec![[170.0, -10.0]];
    west.extend(edge);
    west.push([170.0, 10.0]);
    let mut east = vec![[-170.0, -10.0], [-170.0, 10.0]];
    east.extend(
        edge.iter()
            .rev()
            .map(|&p| if p[0] == 180.0 { [-180.0, p[1]] } else { p }),
    );
    let polys = [vec![west.clone()], vec![east.clone()]];
    let ls = levels(&[Shape::Polygons(&polys[0..1]), Shape::Polygons(&polys[1..2])]);
    let fulls = vec![vec![vec![west.clone()]], vec![vec![east.clone()]]];
    for l in &ls {
        let rings = all_rings(l, &fulls);
        let (p, q) = ([180.0, -10.0], [180.0, 10.0]);
        assert_same_run(&run(&rings[0], p, q), &run(&rings[1], q, p));
        // Each ring keeps its own values.
        assert!(rings[0].iter().all(|p| west.contains(p)));
        assert!(rings[1].iter().all(|p| east.contains(p)));
    }
    // At 64 km the shared edge keeps only its farthest point, because the
    // other arcs between its two ends need it.
    assert_eq!(
        run(&all_rings(&ls[3], &fulls)[0], [180.0, -10.0], [180.0, 10.0]).len(),
        3
    );
}

#[test]
#[ignore = "timing"]
fn manual_build_time_for_700k_points() {
    let m = Mosaic {
        nx: 70,
        ny: 70,
        cell: 0.5,
        origin: [-20.0, -20.0],
        per_edge: 36,
    };
    let polys = m.polygons();
    let points: usize = polys.iter().flatten().map(Vec::len).sum();
    let shapes: Vec<Shape> = polys
        .iter()
        .map(|p| Shape::Polygons(std::slice::from_ref(p)))
        .collect();
    let start = Instant::now();
    let mut times = Vec::new();
    build(&shapes, |l| {
        let count: usize = l
            .features
            .iter()
            .map(|f| match f {
                Simple::Polygons(p) => p.iter().flatten().map(Vec::len).sum(),
                _ => 0,
            })
            .sum();
        times.push((l.index, start.elapsed(), count));
    });
    println!("{points} points");
    for (i, t, n) in times {
        println!("level {i} ready after {t:?}, {n} points");
    }
}
