//! Freehand lines on the sphere.

use crate::math::{V3, add, angle, normalize, slerp};

/// Turns raw pen samples into a line with even spacing and no jitter.
/// `step` is the distance between points, in radians.
pub fn smooth(raw: &[V3], step: f64) -> Vec<V3> {
    if raw.len() < 2 {
        return raw.to_vec();
    }
    // Put points at an even distance along the stroke.
    let mut even = vec![raw[0]];
    let mut need = step;
    for pair in raw.windows(2) {
        let len = angle(pair[0], pair[1]);
        let mut at = 0.0;
        while len - at >= need {
            at += need;
            even.push(slerp(pair[0], pair[1], at / len));
            need = step;
        }
        need -= len - at;
    }
    let last = raw[raw.len() - 1];
    if angle(even[even.len() - 1], last) > step * 0.25 {
        even.push(last);
    }
    // Average each point with its neighbors. The ends stay in place.
    for _ in 0..3 {
        let prev = even.clone();
        for i in 1..prev.len().saturating_sub(1) {
            let sum = add(add(prev[i - 1], prev[i + 1]), add(prev[i], prev[i]));
            even[i] = normalize(sum);
        }
    }
    even
}

/// The length of a line, in radians.
pub fn length(line: &[V3]) -> f64 {
    line.windows(2).map(|p| angle(p[0], p[1])).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::lonlat_to_dir;

    #[test]
    fn smooth_keeps_the_ends_and_the_length() {
        let raw: Vec<V3> = (0..=40)
            .map(|i| lonlat_to_dir(f64::from(i), if i % 2 == 0 { 10.0 } else { 10.3 }))
            .collect();
        let line = smooth(&raw, 0.01);
        assert!(angle(line[0], raw[0]) < 1e-12);
        assert!(angle(line[line.len() - 1], raw[40]) < 1e-9);
        let direct = angle(raw[0], raw[40]);
        assert!(length(&line) < length(&raw));
        assert!((length(&line) - direct).abs() / direct < 0.02);
    }
}
