//! A change of one property in each selected rule of a style. The other
//! properties of each rule stay.

use vale_store::GeometryKind;
use vale_style::{Color, Rule, Stroke, StrokeKind, Symbolizer};

/// A change of one property.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    MinScale(Option<f64>),
    MaxScale(Option<f64>),
    /// Adds the stroke `Stroke` to each rule with no stroke, or removes each stroke.
    Stroke(Option<Stroke>),
    StrokeColor(Color),
    StrokeWidth(f64),
    /// The line type, with its default dash, gap, or start width.
    LineKind(StrokeKind),
    Dash(f64),
    Gap(f64),
    TaperStart(f64),
    /// Adds the fill color to each rule with no fill, or removes each fill.
    Fill(Option<Color>),
    FillColor(Color),
    /// The size of a marker or of a point symbol, in points.
    Size(f64),
}

/// Applies `change` to the rules of `picked`. A rule keeps its value where
/// the property does not apply, for example a stroke color in a rule with no
/// stroke. Returns true if a rule changed.
pub fn apply(rules: &mut [Rule], picked: &[usize], kind: GeometryKind, change: Change) -> bool {
    let mut changed = false;
    for &i in picked {
        let Some(rule) = rules.get_mut(i) else {
            continue;
        };
        let before = rule.clone();
        apply_one(rule, kind, change);
        changed |= *rule != before;
    }
    changed
}

fn apply_one(rule: &mut Rule, kind: GeometryKind, change: Change) {
    let stroke = rule.stroke();
    let edit = |rule: &mut Rule, f: &dyn Fn(&mut Stroke)| {
        if let Some(mut s) = stroke {
            f(&mut s);
            rule.set_stroke(Some(s));
        }
    };
    match change {
        Change::MinScale(v) => rule.min_scale = v,
        Change::MaxScale(v) => rule.max_scale = v,
        // An SVG symbol has no stroke and no fill.
        _ if rule.symbol().is_some() && !matches!(change, Change::Size(_)) => {}
        Change::Stroke(Some(s)) => {
            if stroke.is_none() {
                rule.set_stroke(Some(s));
            }
        }
        Change::Stroke(None) => rule.set_stroke(None),
        Change::StrokeColor(c) => edit(rule, &|s| s.color = c),
        Change::StrokeWidth(w) => edit(rule, &|s| s.width = w),
        Change::LineKind(k) => edit(rule, &|s| {
            if std::mem::discriminant(&s.kind) != std::mem::discriminant(&k) {
                s.kind = k;
            }
        }),
        Change::Dash(d) => edit(rule, &|s| {
            if let StrokeKind::Dashed { dash, .. } = &mut s.kind {
                *dash = d;
            }
        }),
        Change::Gap(g) => edit(rule, &|s| {
            if let StrokeKind::Dashed { gap, .. } = &mut s.kind {
                *gap = g;
            }
        }),
        Change::TaperStart(t) => edit(rule, &|s| {
            if let StrokeKind::Taper { start } = &mut s.kind {
                *start = t;
            }
        }),
        Change::Fill(Some(c)) => {
            if kind == GeometryKind::Polygon && rule.fill().is_none() {
                rule.set_fill(Some(c));
            }
        }
        Change::Fill(None) => {
            if kind == GeometryKind::Polygon {
                rule.set_fill(None);
            }
        }
        Change::FillColor(c) => {
            if rule.fill().is_some() {
                rule.set_fill(Some(c));
            }
        }
        Change::Size(size) => {
            if let Some(mut s) = rule.symbol().cloned() {
                s.size.fallback = size;
                rule.set_symbol(Some(s));
            } else if let Some(radius) = rule.symbolizers.iter_mut().find_map(|s| match s {
                Symbolizer::Marker { radius, .. } => Some(radius),
                _ => None,
            }) {
                radius.fallback = size / 2.0;
            }
        }
    }
}

/// The value of `get` in the rules of `picked` that have one. The second
/// value is true if the rules have different values. `None`: no rule has a
/// value.
pub fn shared<T: PartialEq>(
    rules: &[Rule],
    picked: &[usize],
    get: impl Fn(&Rule) -> Option<T>,
) -> Option<(T, bool)> {
    let mut values = picked.iter().filter_map(|&i| rules.get(i)).filter_map(get);
    let first = values.next()?;
    let mixed = values.any(|v| v != first);
    Some((first, mixed))
}

/// The size of the marker or of the point symbol of a rule, in points.
pub fn size(rule: &Rule) -> Option<f64> {
    if let Some(s) = rule.symbol() {
        return Some(s.size.fallback);
    }
    rule.symbolizers.iter().find_map(|s| match s {
        Symbolizer::Marker { radius, .. } => Some(2.0 * radius.fallback),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vale_style::LayerStyle;

    fn classes(kind: GeometryKind) -> Vec<Rule> {
        let mut style = LayerStyle::default_for(kind, 0, &[]);
        style.add_rule(kind);
        style.add_rule(kind);
        style.style.rules
    }

    #[test]
    fn a_stroke_color_keeps_the_fill_and_the_width_of_each_rule() {
        let kind = GeometryKind::Polygon;
        let mut rules = classes(kind);
        let fills: Vec<_> = rules.iter().map(Rule::fill).collect();
        apply(&mut rules, &[0, 1, 2], kind, Change::StrokeWidth(3.0));
        let wide = rules[2].stroke().map(|s| Stroke { width: 5.0, ..s });
        rules[2].set_stroke(wide);
        let red = Color::rgb(200, 0, 0);
        assert!(apply(
            &mut rules,
            &[0, 1, 2],
            kind,
            Change::StrokeColor(red)
        ));
        for rule in &rules {
            assert_eq!(rule.stroke().map(|s| s.color), Some(red));
        }
        let widths: Vec<_> = rules.iter().map(|r| r.stroke().unwrap().width).collect();
        assert_eq!(widths, vec![3.0, 3.0, 5.0]);
        assert_eq!(rules.iter().map(Rule::fill).collect::<Vec<_>>(), fills);
    }

    #[test]
    fn a_change_applies_only_to_the_picked_rules() {
        let kind = GeometryKind::Line;
        let mut rules = classes(kind);
        let before = rules[1].clone();
        apply(&mut rules, &[0, 2], kind, Change::StrokeWidth(4.0));
        assert_eq!(rules[1], before);
        assert_eq!(rules[0].stroke().unwrap().width, 4.0);
        assert_eq!(rules[2].stroke().unwrap().width, 4.0);
    }

    #[test]
    fn a_stroke_or_fill_switch_keeps_an_existing_value() {
        let kind = GeometryKind::Polygon;
        let mut rules = classes(kind);
        rules[0].set_stroke(None);
        let kept = rules[1].stroke();
        let new = Stroke::solid(Color::rgb(1, 2, 3), 2.0);
        apply(&mut rules, &[0, 1], kind, Change::Stroke(Some(new)));
        assert_eq!(rules[0].stroke(), Some(new));
        assert_eq!(rules[1].stroke(), kept);
        apply(&mut rules, &[0, 1], kind, Change::Fill(None));
        assert_eq!(rules[0].fill(), None);
        assert_eq!(rules[1].fill(), None);
    }

    #[test]
    fn a_dash_changes_only_dashed_strokes() {
        let kind = GeometryKind::Line;
        let mut rules = classes(kind);
        apply(
            &mut rules,
            &[0, 1],
            kind,
            Change::LineKind(StrokeKind::dashed()),
        );
        apply(&mut rules, &[0, 1, 2], kind, Change::Dash(7.0));
        let dash = |r: &Rule| match r.stroke().unwrap().kind {
            StrokeKind::Dashed { dash, .. } => Some(dash),
            _ => None,
        };
        assert_eq!(dash(&rules[0]), Some(7.0));
        assert_eq!(dash(&rules[1]), Some(7.0));
        assert_eq!(dash(&rules[2]), None);
    }

    #[test]
    fn a_size_sets_markers_and_symbols() {
        let kind = GeometryKind::Point;
        let mut rules = classes(kind);
        rules[1].set_symbol(Some(crate::ui::pad::rules::new_point_symbol("tree")));
        apply(&mut rules, &[0, 1], kind, Change::Size(9.0));
        assert_eq!(size(&rules[0]), Some(9.0));
        assert_eq!(size(&rules[1]), Some(9.0));
        // A symbol has no fill to color.
        let before = rules[1].clone();
        apply(
            &mut rules,
            &[1],
            kind,
            Change::FillColor(Color::rgb(9, 9, 9)),
        );
        assert_eq!(rules[1], before);
    }

    #[test]
    fn shared_values_report_a_mix() {
        let kind = GeometryKind::Line;
        let mut rules = classes(kind);
        let width = |r: &Rule| r.stroke().map(|s| s.width);
        apply(&mut rules, &[0, 1, 2], kind, Change::StrokeWidth(2.0));
        assert_eq!(shared(&rules, &[0, 1, 2], width), Some((2.0, false)));
        apply(&mut rules, &[2], kind, Change::StrokeWidth(3.0));
        assert_eq!(shared(&rules, &[0, 1, 2], width), Some((2.0, true)));
        rules[0].set_stroke(None);
        assert_eq!(shared(&rules, &[0], width), None);
    }
}
