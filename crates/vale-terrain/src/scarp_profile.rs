//! The measures of a profile across a scarp, and the reference of the
//! profiles across the Manihiki Scarp. The GEBCO example and the realism test
//! measure their profiles with the same code.

/// The profiles across the Manihiki Scarp, from GEBCO 2020.
/// `scripts/gebco-reference.sh manihiki-scarp` makes the file.
pub const MANIHIKI_SCARP: &str = include_str!("../data/gebco/manihiki-scarp.txt");

/// The kilometers between two samples of a profile.
pub const PROFILE_STEP_KM: f64 = 1.0;

/// The half length of the window around the steepest point of a profile.
pub const PROFILE_HALF_KM: f64 = 80.0;

/// The length of the box that smooths a profile before the search for its
/// steepest point.
const SMOOTH_KM: f64 = 20.0;

/// The fit moves the center of the scarp by this distance at most from the
/// steepest point.
const SHIFT_KM: f64 = 30.0;

/// The widths of the smoothstep that the fit tries.
const FIT_WIDTHS_KM: std::ops::RangeInclusive<u32> = 2..=120;

/// The values of the normal shape, at the 50% point plus `t` times the width
/// from 10% to 90%, for `t` from -`SHAPE_REACH` to `SHAPE_REACH`.
pub const SHAPE_SAMPLES: usize = 41;
const SHAPE_REACH: f64 = 2.0;

/// The width from 10% to 90% of a smoothstep, as a part of its full width.
pub const SMOOTHSTEP_10_90: f64 = 0.6084;

/// The measures of one profile.
#[derive(Clone, Debug, PartialEq)]
pub struct ScarpProfile {
    /// The level of the top less the level of the foot.
    pub drop_m: f64,
    /// The distance from the 90% point to the 10% point of the drop.
    pub width_km: f64,
    /// The place of the 50% point, from the first sample of the profile.
    pub trace_km: f64,
    /// `(z - foot) / drop` along the profile, in units of the width.
    pub shape: Vec<f64>,
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Measures a profile that steps down from the first sample to the last, with
/// a sample each `PROFILE_STEP_KM`. The profile must reach `PROFILE_HALF_KM`
/// past its steepest point on each side.
///
/// A least-squares fit of a smoothstep to the window around the steepest
/// point gives the top, the foot, the center, and the width. The fit keeps a
/// seamount or a trough next to the scarp from moving the measures far.
pub fn scarp_profile(z: &[f64]) -> ScarpProfile {
    let step = PROFILE_STEP_KM;
    let half = (PROFILE_HALF_KM / step).round() as usize;
    let box_half = (SMOOTH_KM / step / 2.0).round() as usize;
    let smooth: Vec<f64> = (0..z.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(box_half), (i + box_half).min(z.len() - 1));
            z[a..=b].iter().sum::<f64>() / (b - a + 1) as f64
        })
        .collect();
    let center = (half..z.len() - half - 1)
        .max_by(|&i, &j| {
            let fall = |k: usize| smooth[k] - smooth[k + 1];
            fall(i).total_cmp(&fall(j))
        })
        .expect("the profile reaches past its steepest point");
    let (a, b) = (center - half, center + half);
    let window = &z[a..=b];
    // For each center and width, the top and the foot are a linear fit.
    let mut best = (f64::INFINITY, 0.0, 0.0, 0.0, 1.0);
    let shift = (SHIFT_KM / step).round() as i64;
    for c in -shift..=shift {
        let at = (half as i64 + c) as f64 * step;
        for w in FIT_WIDTHS_KM {
            let w = w as f64;
            let (mut su, mut suu, mut sz, mut suz) = (0.0, 0.0, 0.0, 0.0);
            for (i, &v) in window.iter().enumerate() {
                let u = 1.0 - smoothstep((i as f64 * step - at) / w + 0.5);
                su += u;
                suu += u * u;
                sz += v;
                suz += u * v;
            }
            let n = window.len() as f64;
            let det = n * suu - su * su;
            if det.abs() < 1e-9 {
                continue;
            }
            let drop = (n * suz - su * sz) / det;
            let foot = (sz - drop * su) / n;
            let error: f64 = window
                .iter()
                .enumerate()
                .map(|(i, &v)| {
                    let u = 1.0 - smoothstep((i as f64 * step - at) / w + 0.5);
                    (v - foot - drop * u).powi(2)
                })
                .sum();
            if error < best.0 {
                best = (error, foot, drop, at, w);
            }
        }
    }
    let (_, foot, drop, at, w) = best;
    let trace_km = a as f64 * step + at;
    let width_km = SMOOTHSTEP_10_90 * w;
    let sample = |x: f64| {
        let f = (x / step).clamp(0.0, (z.len() - 1) as f64);
        let i = (f.floor() as usize).min(z.len() - 2);
        z[i] + (z[i + 1] - z[i]) * (f - i as f64)
    };
    let shape = (0..SHAPE_SAMPLES)
        .map(|n| {
            let t = -SHAPE_REACH + 2.0 * SHAPE_REACH * n as f64 / (SHAPE_SAMPLES - 1) as f64;
            (sample(trace_km + t * width_km) - foot) / drop
        })
        .collect();
    ScarpProfile {
        drop_m: drop,
        width_km,
        trace_km,
        shape,
    }
}

/// The profiles of a reference, in order along the scarp.
#[derive(Clone, Debug, PartialEq)]
pub struct ScarpReference {
    pub profiles: Vec<ScarpProfile>,
    /// The kilometers between two profiles.
    pub spacing_km: f64,
}

impl ScarpReference {
    /// Reads the text that `gebco_profiles` writes. A line is `key = values`.
    /// A `profile` line holds the drop, the width, the trace, and the shape.
    pub fn parse(text: &str) -> Result<ScarpReference, String> {
        let mut profiles = Vec::new();
        let mut spacing_km = None;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line.split_once('=').ok_or(format!("no `=` in `{line}`"))?;
            let numbers = || -> Result<Vec<f64>, String> {
                value
                    .split(',')
                    .map(|v| {
                        v.trim()
                            .parse::<f64>()
                            .map_err(|e| format!("{e} in `{line}`"))
                    })
                    .collect()
            };
            match key.trim() {
                "spacing_km" => spacing_km = numbers()?.first().copied(),
                "profile" => {
                    let v = numbers()?;
                    if v.len() != 3 + SHAPE_SAMPLES {
                        return Err(format!("a profile has {} values", v.len()));
                    }
                    profiles.push(ScarpProfile {
                        drop_m: v[0],
                        width_km: v[1],
                        trace_km: v[2],
                        shape: v[3..].to_vec(),
                    });
                }
                _ => {}
            }
        }
        if profiles.len() < 3 {
            return Err("a reference has 3 profiles at least".into());
        }
        Ok(ScarpReference {
            profiles,
            spacing_km: spacing_km.ok_or("no `spacing_km`")?,
        })
    }

    /// Writes the lines that `parse` reads.
    pub fn lines(&self) -> String {
        let mut out = format!("spacing_km = {:.2}\n", self.spacing_km);
        for p in &self.profiles {
            let mut v = vec![
                format!("{:.1}", p.drop_m),
                format!("{:.2}", p.width_km),
                format!("{:.2}", p.trace_km),
            ];
            v.extend(p.shape.iter().map(|s| format!("{s:.3}")));
            out += &format!("profile = {}\n", v.join(", "));
        }
        out
    }
}

/// The middle value of a list.
pub fn median(values: impl IntoIterator<Item = f64>) -> f64 {
    let mut v: Vec<f64> = values.into_iter().collect();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(drop: f64, width_km: f64, at_km: f64) -> Vec<f64> {
        (0..=240)
            .map(|i| {
                let t = ((i as f64 - at_km) / width_km + 0.5).clamp(0.0, 1.0);
                -drop * t * t * (3.0 - 2.0 * t)
            })
            .collect()
    }

    #[test]
    fn a_smoothstep_profile_gives_its_drop_and_width() {
        let p = scarp_profile(&ramp(3000.0, 20.0, 110.0));
        assert!((p.drop_m - 3000.0).abs() < 1.0, "{p:?}");
        assert!((p.width_km - SMOOTHSTEP_10_90 * 20.0).abs() < 0.01, "{p:?}");
        assert!((p.trace_km - 110.0).abs() < 0.1, "{p:?}");
        assert!((p.shape[SHAPE_SAMPLES / 2] - 0.5).abs() < 0.01);
    }

    #[test]
    fn a_reference_reads_back_its_lines() {
        let p = scarp_profile(&ramp(2000.0, 30.0, 120.0));
        let r = ScarpReference {
            profiles: vec![p.clone(), p.clone(), p],
            spacing_km: 111.2,
        };
        let back = ScarpReference::parse(&r.lines()).unwrap();
        assert_eq!(back.profiles.len(), 3);
        assert!((back.profiles[0].width_km - r.profiles[0].width_km).abs() < 0.01);
    }
}
