use vale_terrain::{Heightmap, Limits, Ridge, RidgeLine, meters_to_level};
#[test]
fn time_cpu_ridge() {
    let km = 6371.0;
    for (n, radius_km) in [(2048, 300.0), (2048, 600.0), (8192, 300.0), (8192, 600.0)] {
        let spec = Ridge {
            radius: radius_km / km,
            crest: meters_to_level(-700.0),
            segment: 400.0 / km,
            offset: 100.0 / km,
            spread: 0.5 * 25.0 / km,
            magma: 0.0,
            world_km: km,
            seed: 7,
            limits: Limits::default(),
        };
        let mut line = RidgeLine::new(spec, n);
        for i in 0..=400 {
            let t = f64::from(i) / 400.0;
            let (lon, lat) = (
                (-30.0 + 50.0 * t).to_radians(),
                (10.0 * (t * 6.0).sin()).to_radians(),
            );
            line.push(
                [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()],
                1.0,
            );
        }
        let mut map = Heightmap::new(n, meters_to_level(-4000.0));
        map.begin_stroke();
        let start = std::time::Instant::now();
        let (mut from, mut plans, mut texels) = (0, 0, 0usize);
        while from < line.len() {
            let plan = line.plan(from);

            texels += map.ridge(&plan);
            from = plan.end;
            plans += 1;
        }
        eprintln!(
            "TIME n {n} r {radius_km} points {} plans {plans} texels {:.1}M {:?}",
            line.len(),
            texels as f64 / 1e6,
            start.elapsed()
        );
    }
}
