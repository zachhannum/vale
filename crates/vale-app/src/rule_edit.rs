//! A change of one property in each selected rule of a style. The other
//! properties of each rule stay.

use vale_store::GeometryKind;
use vale_style::{Color, DashPattern, Rule, Stroke, StrokeKind, Symbolizer};

/// A change of one property.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    MinScale(Option<f64>),
    MaxScale(Option<f64>),
    /// Adds the stroke `Stroke` to each rule with no stroke, or removes each stroke.
    Stroke(Option<Stroke>),
    StrokeColor(Color),
    StrokeWidth(f64),
    /// Whether the stroke follows the mask of the style. A marker stroke has no such value.
    StrokeAppliesMask(bool),
    /// The line type, with its default pattern, start width, or smallest width.
    LineKind(StrokeKind),
    /// The dash pattern of each dashed stroke.
    DashPattern(DashPattern),
    /// The lengths of each dash pattern. Each pattern keeps its ends.
    DashLengths(DashPattern),
    /// The ends of each dash pattern.
    DashRound(bool),
    TaperStart(f64),
    FlowMin(f64),
    /// Adds the fill color to each rule with no fill, or removes each fill.
    Fill(Option<Color>),
    FillColor(Color),
    /// Whether the fill follows the mask of the style. A marker fill has no such value.
    FillAppliesMask(bool),
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
        Change::StrokeAppliesMask(v) => rule.set_stroke_applies_mask(v),
        Change::LineKind(k) => edit(rule, &|s| {
            if std::mem::discriminant(&s.kind) != std::mem::discriminant(&k) {
                s.kind = k;
            }
        }),
        Change::DashPattern(p) => edit(rule, &|s| {
            if let StrokeKind::Dashed(d) = &mut s.kind {
                *d = p;
            }
        }),
        Change::DashLengths(p) => edit(rule, &|s| {
            if let StrokeKind::Dashed(d) = &mut s.kind {
                *d = d.with_lengths(&p);
            }
        }),
        Change::DashRound(r) => edit(rule, &|s| {
            if let StrokeKind::Dashed(d) = &mut s.kind {
                d.round = r;
            }
        }),
        Change::TaperStart(t) => edit(rule, &|s| {
            if let StrokeKind::Taper { start } = &mut s.kind {
                *start = t;
            }
        }),
        Change::FlowMin(m) => edit(rule, &|s| {
            if let StrokeKind::Flow { min } = &mut s.kind {
                *min = m;
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
        Change::FillAppliesMask(v) => rule.set_fill_applies_mask(v),
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
    fn a_dash_pattern_changes_only_dashed_strokes() {
        let kind = GeometryKind::Line;
        let mut rules = classes(kind);
        apply(
            &mut rules,
            &[0, 1],
            kind,
            Change::LineKind(StrokeKind::dashed()),
        );
        let pattern = |r: &Rule| match r.stroke().unwrap().kind {
            StrokeKind::Dashed(p) => Some(p),
            _ => None,
        };
        let custom = DashPattern::parse("7 2", false).unwrap();
        apply(&mut rules, &[0, 1, 2], kind, Change::DashLengths(custom));
        assert_eq!(pattern(&rules[0]).unwrap().lengths(), &[7.0, 2.0]);
        apply(
            &mut rules,
            &[0, 1, 2],
            kind,
            Change::DashPattern(DashPattern::DASH_DOT),
        );
        assert_eq!(pattern(&rules[0]), Some(DashPattern::DASH_DOT));
        assert_eq!(pattern(&rules[1]), Some(DashPattern::DASH_DOT));
        assert_eq!(pattern(&rules[2]), None);
        apply(&mut rules, &[0, 1], kind, Change::DashRound(false));
        assert!(!pattern(&rules[1]).unwrap().round);
        // Choosing Dashed again keeps the pattern.
        apply(
            &mut rules,
            &[0],
            kind,
            Change::LineKind(StrokeKind::dashed()),
        );
        assert_eq!(
            pattern(&rules[0]).unwrap().lengths(),
            DashPattern::DASH_DOT.lengths()
        );
    }

    #[test]
    fn dash_lengths_keep_the_ends_of_each_pattern() {
        let kind = GeometryKind::Line;
        let mut rules = classes(kind);
        let dashed = Change::LineKind(StrokeKind::dashed());
        apply(&mut rules, &[0, 1], kind, dashed);
        apply(
            &mut rules,
            &[1],
            kind,
            Change::DashPattern(DashPattern::DOTTED),
        );
        let lengths = DashPattern::parse("6 2.5 0 2.5", false).unwrap();
        apply(&mut rules, &[0, 1, 2], kind, Change::DashLengths(lengths));
        let pattern = |r: &Rule| match r.stroke().unwrap().kind {
            StrokeKind::Dashed(p) => Some(p),
            _ => None,
        };
        let (a, b) = (pattern(&rules[0]).unwrap(), pattern(&rules[1]).unwrap());
        assert_eq!(a.lengths(), lengths.lengths());
        assert_eq!(b.lengths(), lengths.lengths());
        assert!(!a.round && b.round);
        assert_eq!(pattern(&rules[2]), None);
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

    #[test]
    fn an_apply_mask_change_keeps_the_color_and_skips_markers() {
        let kind = GeometryKind::Polygon;
        let mut rules = classes(kind);
        let colors: Vec<_> = rules.iter().map(|r| (r.fill(), r.stroke())).collect();
        assert!(apply(
            &mut rules,
            &[0, 1],
            kind,
            Change::FillAppliesMask(false)
        ));
        assert!(apply(
            &mut rules,
            &[0],
            kind,
            Change::StrokeAppliesMask(false)
        ));
        let fill = |r: &Rule| r.fill_applies_mask();
        let stroke = |r: &Rule| r.stroke_applies_mask();
        assert_eq!(shared(&rules, &[0, 1], fill), Some((false, false)));
        assert_eq!(shared(&rules, &[0, 1, 2], fill), Some((false, true)));
        assert_eq!(shared(&rules, &[0, 1], stroke), Some((false, true)));
        let after: Vec<_> = rules.iter().map(|r| (r.fill(), r.stroke())).collect();
        assert_eq!(after, colors);

        let kind = GeometryKind::Point;
        let mut rules = classes(kind);
        assert!(!apply(
            &mut rules,
            &[0],
            kind,
            Change::StrokeAppliesMask(false)
        ));
        assert!(!apply(
            &mut rules,
            &[0],
            kind,
            Change::FillAppliesMask(false)
        ));
    }
}
