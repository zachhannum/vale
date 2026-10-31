//! The scale bar of a page: a bar, marks, numbers, and unit labels for one
//! or two units, with vector paths and glyphs.

use kurbo::{Affine, BezPath, Point, Rect, Shape};
use vale_labeler::Fonts;
use vale_render::{DisplayList, FillRule, Item, Rgba};
use vale_store::{
    ScaleBarItem, ScaleBarKind, ScaleFit, ScalePosition, ScaleUnit, TickFrequency, UnitPosition,
};

use crate::atlas::PT_PER_MM;
use crate::furniture::{PAD, Painted, TextInk, rgb};

/// The largest of 1, 2, 2.5, and 5 times a power of ten that is at most
/// `v`. `None` if `v` is not a positive number.
pub fn nice_division(v: f64) -> Option<f64> {
    if !(v.is_finite() && v > 0.0) {
        return None;
    }
    let p = 10f64.powf(v.log10().floor());
    let nice = [5.0, 2.5, 2.0, 1.0].iter().map(|m| m * p);
    Some(
        nice.into_iter()
            .find(|c| *c <= v * (1.0 + 1e-12))
            .unwrap_or(p),
    )
}

/// One unit of a drawn scale bar.
#[derive(Clone, Debug, PartialEq)]
pub struct BarRow {
    pub unit: ScaleUnit,
    /// The length of a division in the unit.
    pub value: f64,
    /// The divisions right of zero.
    pub divisions: u32,
    /// The left end, the zero, and the right end of the bar, in target
    /// points.
    pub start: f64,
    pub zero: f64,
    pub end: f64,
    /// The numbers that are drawn, from left to right, with the x of each.
    pub numbers: Vec<(String, f64)>,
}

/// A drawn scale bar.
#[derive(Clone, Debug, Default)]
pub struct Drawing {
    pub items: Vec<Item>,
    /// The first unit, then the second unit of a double scale.
    pub rows: Vec<BarRow>,
    /// The box of the drawing.
    pub bounds: Rect,
}

/// The rank of a place on the bar. A number of a lower rank gives way to a
/// number of a higher rank.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    End,
    Zero,
    Division,
    Sub,
}

/// A place on the bar, in divisions from zero.
#[derive(Copy, Clone, Debug)]
struct Tick {
    at: f64,
    rank: Rank,
}

/// The places of the marks or the numbers of `freq`.
fn ticks(bar: &ScaleBarItem, divisions: u32, freq: TickFrequency) -> Vec<Tick> {
    let (b, n) = (i32::from(bar.before_zero), divisions as i32);
    let boundary = |k: i32| Tick {
        at: f64::from(k),
        rank: if k == -b || k == n {
            Rank::End
        } else if k == 0 {
            Rank::Zero
        } else {
            Rank::Division
        },
    };
    let mut out = Vec::new();
    match freq {
        TickFrequency::None => {}
        TickFrequency::Ends => out.extend([boundary(-b), boundary(n)]),
        TickFrequency::EndsAndZero => out.extend([boundary(-b), boundary(0), boundary(n)]),
        _ => out.extend((-b..=n).map(boundary)),
    }
    let s = bar.subdivisions;
    let parts = |k: i32| (1..s).map(move |j| f64::from(k) + f64::from(j) / f64::from(s));
    let sub = |at: f64| Tick {
        at,
        rank: Rank::Sub,
    };
    match freq {
        TickFrequency::DivisionsAndFirstMidpoint => out.push(sub(f64::from(-b) + 0.5)),
        TickFrequency::DivisionsAndFirstSubdivisions => out.extend(parts(-b).map(sub)),
        TickFrequency::DivisionsAndSubdivisions => {
            out.extend((-b..n).flat_map(parts).map(sub));
        }
        _ => {}
    }
    out.sort_by(|p, q| p.at.total_cmp(&q.at).then(p.rank.cmp(&q.rank)));
    out.dedup_by(|q, p| (q.at - p.at).abs() < 1e-9);
    out
}

/// The parts of the bar that alternate their fill, in divisions from zero.
/// The first division has the subdivisions.
fn segments(bar: &ScaleBarItem, divisions: u32) -> Vec<(f64, f64)> {
    let (b, n) = (i32::from(bar.before_zero), divisions as i32);
    let s = bar.subdivisions;
    let mut out = Vec::new();
    for k in -b..n {
        let k = f64::from(k);
        if out.is_empty() && s >= 2 {
            let step = 1.0 / f64::from(s);
            out.extend((0..s).map(|j| (k + f64::from(j) * step, k + f64::from(j + 1) * step)));
        } else {
            out.push((k, k + 1.0));
        }
    }
    out
}

/// The texts of `values`, with `decimals`, or else each with the fewest
/// decimals that show it exactly, up to 6.
fn number_texts(values: &[f64], decimals: Option<u32>, thousands: bool) -> Vec<String> {
    let exact = |v: f64, d: u32| {
        let s = v * 10f64.powi(d as i32);
        (s - s.round()).abs() < 1e-6 * s.abs().max(1.0)
    };
    (values.iter())
        .map(|&v| {
            let d = decimals.unwrap_or_else(|| (0..=6).find(|d| exact(v, *d)).unwrap_or(6));
            let d = d as usize;
            let text = format!("{v:.d$}");
            if !thousands {
                return text;
            }
            let (whole, rest) = text.split_at(text.find('.').unwrap_or(text.len()));
            let mut out = String::new();
            for (i, digit) in whole.chars().enumerate() {
                if i > 0 && (whole.len() - i).is_multiple_of(3) {
                    out.push(',');
                }
                out.push(digit);
            }
            out + rest
        })
        .collect()
}

/// The division value and the divisions right of zero of `unit` in `room`
/// target points. `None` if no bar fits.
fn fit(
    bar: &ScaleBarItem,
    unit: ScaleUnit,
    given: Option<f64>,
    room: f64,
    km_per_point: f64,
) -> Option<(f64, u32)> {
    let per_unit = unit.km() / km_per_point;
    let b = u32::from(bar.before_zero);
    let auto = || nice_division(room / f64::from(bar.divisions + b) / per_unit);
    let given = given.filter(|v| v.is_finite() && *v > 0.0);
    match bar.fit {
        ScaleFit::AdjustDivisionValue => Some((auto()?, bar.divisions)),
        ScaleFit::AdjustWidth => Some((given.or_else(auto)?, bar.divisions)),
        ScaleFit::AdjustDivisions => {
            let value = given.or_else(auto)?;
            let n = (room / (value * per_unit) + 1e-9).floor() - f64::from(b);
            let (lo, hi) = ScaleBarItem::DIVISIONS.into_inner();
            Some((value, n.clamp(f64::from(lo), f64::from(hi)) as u32))
        }
    }
}

/// The side of the bar of a place: away from the other unit, toward it, or
/// on the bar.
#[derive(Copy, Clone, PartialEq)]
enum Side {
    Out,
    In,
    On,
}

fn side(p: ScalePosition) -> Side {
    match p {
        ScalePosition::Above => Side::Out,
        ScalePosition::Below => Side::In,
        ScalePosition::Center => Side::On,
    }
}

/// A line of text to draw: the ink, the text, and the start of its baseline.
struct Text {
    unit_label: bool,
    text: String,
    at: Point,
}

/// A scale bar with zero at x = 0 and the top of the bar of the first unit
/// at y = 0.
struct Layout {
    fills: Vec<Item>,
    lines: Vec<Item>,
    texts: Vec<Text>,
    rows: Vec<BarRow>,
    /// The left and the right end of the bars.
    bars: (f64, f64),
    bounds: Rect,
}

/// The inks of the numbers and the unit labels, and their cap heights.
struct Inks {
    number: TextInk,
    unit: TextInk,
    number_cap: f64,
    unit_cap: f64,
    unit_descent: f64,
}

impl Inks {
    fn new(bar: &ScaleBarItem, unit: f64, fonts: &mut Fonts) -> Inks {
        let number = TextInk::new(&bar.number_style, unit, fonts);
        let ink = TextInk::new(&bar.unit_style, unit, fonts);
        let cap = |ink: &TextInk, fonts: &mut Fonts| {
            let m = fonts.measure("0H", &ink.style);
            (
                if m.cap_height > 0.0 {
                    m.cap_height
                } else {
                    0.7 * m.ascent
                },
                m.descent.abs(),
            )
        };
        let (number_cap, _) = cap(&number, fonts);
        let (unit_cap, unit_descent) = cap(&ink, fonts);
        Inks {
            number,
            unit: ink,
            number_cap,
            unit_cap,
            unit_descent,
        }
    }
}

/// The units of a bar: the unit, its division value, and whether it is the
/// second unit.
fn units(bar: &ScaleBarItem) -> Vec<(ScaleUnit, Option<f64>, bool)> {
    let mut out = vec![(bar.unit, bar.division_value, false)];
    if let Some(second) = bar.second_unit {
        out.push((second, bar.second_division_value, true));
    }
    out
}

/// Lays out a bar with the division values and the divisions `fitted`.
fn layout(
    bar: &ScaleBarItem,
    fitted: &[(f64, u32)],
    unit: f64,
    km_per_point: f64,
    inks: &Inks,
    fonts: &mut Fonts,
) -> Layout {
    let mm = PT_PER_MM * unit;
    let line_kind = bar.kind == ScaleBarKind::Line;
    let h = if line_kind { 0.0 } else { bar.bar_height * mm };
    let width = bar.line_width * unit;
    let ink = rgb(bar.outline);
    let b = f64::from(u8::from(bar.before_zero));
    let mut out = Layout {
        fills: Vec::new(),
        lines: Vec::new(),
        texts: Vec::new(),
        rows: Vec::new(),
        bars: (0.0, 0.0),
        bounds: Rect::ZERO,
    };
    let mut boxes: Vec<Rect> = Vec::new();
    let mut outline = BezPath::new();
    for (r, ((u, _, second), &(value, divisions))) in units(bar).into_iter().zip(fitted).enumerate()
    {
        let division = value * u.km() / km_per_point;
        let x = |at: f64| at * division;
        let (start, end) = (x(-b), x(f64::from(divisions)));
        out.bars = (out.bars.0.min(start), out.bars.1.max(end));
        // The direction away from the other unit, and the bar of this unit.
        let dir = if r == 0 { -1.0 } else { 1.0 };
        let (top, bottom) = if r == 0 { (0.0, h) } else { (h, 2.0 * h) };
        let (outer, inner, mid) = if r == 0 {
            (top, bottom, (top + bottom) / 2.0)
        } else {
            (bottom, top, (top + bottom) / 2.0)
        };
        boxes.push(Rect::new(start, top, end, bottom));

        // The bar.
        let segs = segments(bar, divisions);
        let seg = |(a, c): (f64, f64), y0: f64, y1: f64| Rect::new(x(a), y0, x(c), y1);
        let fill = |rect: Rect, color: [u8; 3]| Item::Fill {
            path: rect.to_path(0.1),
            color: rgb(color),
            rule: FillRule::NonZero,
        };
        let whole = Rect::new(start, top, end, bottom);
        let fills = |i: usize| {
            if i.is_multiple_of(2) {
                bar.fill1
            } else {
                bar.fill2
            }
        };
        match bar.kind {
            ScaleBarKind::Alternating => {
                for (i, s) in segs.iter().enumerate() {
                    out.fills.push(fill(seg(*s, top, bottom), fills(i)));
                    outline.extend(seg(*s, top, bottom).path_elements(0.1));
                }
            }
            ScaleBarKind::DoubleAlternating => {
                for (i, s) in segs.iter().enumerate() {
                    out.fills.push(fill(seg(*s, top, mid), fills(i)));
                    out.fills.push(fill(seg(*s, mid, bottom), fills(i + 1)));
                    outline.extend(seg(*s, top, bottom).path_elements(0.1));
                }
                outline.move_to((start, mid));
                outline.line_to((end, mid));
            }
            ScaleBarKind::Hollow => {
                out.fills.push(fill(whole, bar.fill2));
                for s in &segs {
                    outline.extend(seg(*s, top, bottom).path_elements(0.1));
                }
            }
            ScaleBarKind::SingleDivision => {
                out.fills.push(fill(whole, bar.fill1));
                outline.extend(whole.path_elements(0.1));
            }
            ScaleBarKind::SteppedLine => {
                let y = |i: usize| if i.is_multiple_of(2) { top } else { bottom };
                let other = |i: usize| if i.is_multiple_of(2) { bottom } else { top };
                outline.move_to((start, other(0)));
                for (i, (a, c)) in segs.iter().enumerate() {
                    outline.line_to((x(*a), y(i)));
                    outline.line_to((x(*c), y(i)));
                }
                outline.line_to((end, other(segs.len() - 1)));
            }
            ScaleBarKind::Line => {
                outline.move_to((start, 0.0));
                outline.line_to((end, 0.0));
            }
        }

        // The marks, and how far they reach out of the bar on each side.
        let mark_side = side(bar.mark_position);
        let (mut reach_out, mut reach_in) = (0.0f64, 0.0f64);
        let mut marks = BezPath::new();
        for t in ticks(bar, divisions, bar.mark_frequency) {
            let length = match t.rank {
                Rank::Sub => bar.subdivision_mark_height,
                _ => bar.division_mark_height,
            } * mm;
            if length <= 0.0 && mark_side != Side::On {
                continue;
            }
            let (y0, y1) = match mark_side {
                Side::Out => {
                    reach_out = reach_out.max(length);
                    (outer, outer + dir * length)
                }
                Side::In => {
                    reach_in = reach_in.max(length);
                    (inner, inner - dir * length)
                }
                Side::On => {
                    reach_out = reach_out.max(length / 2.0);
                    reach_in = reach_in.max(length / 2.0);
                    (mid - (h + length) / 2.0, mid + (h + length) / 2.0)
                }
            };
            marks.move_to((x(t.at), y0));
            marks.line_to((x(t.at), y1));
            boxes.push(Rect::new(x(t.at), y0.min(y1), x(t.at), y0.max(y1)));
        }
        if !marks.is_empty() {
            out.lines.push(Item::Stroke {
                path: marks,
                color: ink,
                width,
                round: false,
            });
        }

        // The baseline of a text of cap height `cap` at the distance `gap`
        // from `edge`, on the side `d` (-1 is up).
        let place = |edge: f64, gap: f64, d: f64, cap: f64| {
            if d < 0.0 {
                edge - gap
            } else {
                edge + gap + cap
            }
        };

        // The numbers, with a number of a lower rank left out where it
        // meets another one.
        let number_side = side(bar.number_position);
        let gap = bar.number_gap * mm;
        let cap = inks.number_cap;
        let baseline = match number_side {
            Side::Out => place(outer, reach_out + gap, dir, cap),
            Side::In => place(inner, reach_in + gap, -dir, cap),
            Side::On => mid + cap / 2.0,
        };
        let shown: Vec<Tick> = (ticks(bar, divisions, bar.number_frequency).into_iter())
            .filter(|t| bar.zero_label || t.at.abs() > 1e-9)
            .collect();
        let values: Vec<f64> = shown.iter().map(|t| t.at.abs() * value).collect();
        let texts = number_texts(&values, bar.decimals, bar.thousands);
        let space = 0.3 * inks.number.style.size;
        let mut order: Vec<usize> = (0..shown.len()).collect();
        order.sort_by_key(|i| shown[*i].rank);
        let mut kept: Vec<(usize, Rect)> = Vec::new();
        for i in order {
            let w = inks.number.advance(&texts[i], fonts);
            let cx = x(shown[i].at);
            let rect = Rect::new(cx - w / 2.0, baseline - cap, cx + w / 2.0, baseline);
            if kept
                .iter()
                .any(|(_, k)| k.inflate(space, 0.0).overlaps(rect))
            {
                continue;
            }
            kept.push((i, rect));
        }
        kept.sort_by(|p, q| p.1.x0.total_cmp(&q.1.x0));
        let mut numbers = Vec::new();
        for (i, rect) in &kept {
            out.texts.push(Text {
                unit_label: false,
                text: texts[*i].clone(),
                at: Point::new(rect.x0, baseline),
            });
            boxes.push(*rect);
            numbers.push((texts[*i].clone(), x(shown[*i].at)));
        }
        let first = kept.first().map(|k| k.1.x0);
        let last = kept.last().map(|k| k.1.x1);

        // The unit label.
        let label = bar.unit_text(u, second);
        let w = inks.unit.advance(&label, fonts);
        let ucap = inks.unit_cap;
        let ugap = bar.unit_gap * mm;
        // The baseline beside the bar: on the bar, or on the outer side of
        // a line, where a second unit is.
        let beside = if line_kind && fitted.len() > 1 {
            place(0.0, width, dir, ucap)
        } else {
            mid + ucap / 2.0
        };
        let by_labels = !kept.is_empty() && number_side != Side::On;
        let row_box = boxes
            .iter()
            .fold(Rect::new(start, top, end, bottom), |a, r| a.union(*r));
        let (y_out, y_in) = if dir < 0.0 {
            (row_box.y0, row_box.y1)
        } else {
            (row_box.y1, row_box.y0)
        };
        let at = match bar.unit_position {
            UnitPosition::AfterBar => Point::new(end + ugap, beside),
            UnitPosition::BeforeBar => Point::new(start - ugap - w, beside),
            UnitPosition::AfterLabels if by_labels => {
                Point::new(last.unwrap_or(end).max(end) + ugap, baseline)
            }
            UnitPosition::AfterLabels => Point::new(end + ugap, beside),
            UnitPosition::BeforeLabels if by_labels => {
                Point::new(first.unwrap_or(start).min(start) - ugap - w, baseline)
            }
            UnitPosition::BeforeLabels => Point::new(start - ugap - w, beside),
            UnitPosition::AboveLeft => Point::new(start, place(y_out, ugap, dir, ucap)),
            UnitPosition::AboveCenter => {
                Point::new((start + end - w) / 2.0, place(y_out, ugap, dir, ucap))
            }
            UnitPosition::BelowLeft => Point::new(start, place(y_in, ugap, -dir, ucap)),
            UnitPosition::BelowCenter => {
                Point::new((start + end - w) / 2.0, place(y_in, ugap, -dir, ucap))
            }
        };
        if w > 0.0 {
            boxes.push(Rect::new(
                at.x,
                at.y - ucap,
                at.x + w,
                at.y + inks.unit_descent,
            ));
            out.texts.push(Text {
                unit_label: true,
                text: label,
                at,
            });
        }
        out.rows.push(BarRow {
            unit: u,
            value,
            divisions,
            start,
            zero: 0.0,
            end,
            numbers,
        });
    }
    if !outline.is_empty() {
        out.lines.insert(
            0,
            Item::Stroke {
                path: outline,
                color: ink,
                width,
                round: false,
            },
        );
    }
    let half = width / 2.0;
    out.bounds = (boxes.iter().copied())
        .reduce(|a, r| a.union(r))
        .unwrap_or(Rect::ZERO)
        .inflate(half, half);
    out
}

/// A scale bar in `rect`, in target points with `unit` target points to a
/// page point, at `km_per_point` ground kilometers to a target point. The
/// drawing starts at the left of `rect` and is in the middle of its height.
/// With [`ScaleFit::AdjustWidth`] it can be wider than `rect`.
pub fn draw(
    bar: &ScaleBarItem,
    rect: Rect,
    unit: f64,
    km_per_point: f64,
    fonts: &mut Fonts,
) -> Drawing {
    if !(km_per_point.is_finite() && km_per_point > 0.0) {
        return Drawing::default();
    }
    let inks = Inks::new(bar, unit, fonts);
    let pad = PAD * unit;
    let units = units(bar);
    // The text that is out of the bars on the left and on the right, and
    // the space left of the zero of each unit that the other unit takes.
    let (mut over, mut extra) = ((0.0, 0.0), vec![0.0; units.len()]);
    let mut fitted = Vec::new();
    let mut laid = None;
    for _ in 0..3 {
        let room = rect.width() - 2.0 * pad - over.0 - over.1;
        let next: Option<Vec<_>> = (units.iter().zip(&extra))
            .map(|((u, given, _), extra)| fit(bar, *u, *given, room - extra, km_per_point))
            .collect();
        let Some(next) = next else {
            return Drawing::default();
        };
        let l = layout(bar, &next, unit, km_per_point, &inks, fonts);
        let before: Vec<f64> = l.rows.iter().map(|r| -r.start).collect();
        let most = before.iter().copied().fold(0.0, f64::max);
        extra = before.iter().map(|b| most - b).collect();
        over = (
            (l.bars.0 - l.bounds.x0).max(0.0),
            (l.bounds.x1 - l.bars.1).max(0.0),
        );
        let same = next == fitted;
        fitted = next;
        laid = Some(l);
        if same {
            break;
        }
    }
    let Some(l) = laid else {
        return Drawing::default();
    };
    let shift = Affine::translate((
        rect.x0 + pad - l.bounds.x0,
        rect.center().y - l.bounds.center().y,
    ));
    let mut items = l.fills;
    items.extend(l.lines);
    for item in &mut items {
        match item {
            Item::Fill { path, .. } | Item::Stroke { path, .. } => *path = shift * &*path,
            _ => {}
        }
    }
    let mut painted = Painted::default();
    for t in &l.texts {
        let ink = if t.unit_label {
            &inks.unit
        } else {
            &inks.number
        };
        ink.line(
            &t.text,
            shift * Affine::translate(t.at.to_vec2()),
            fonts,
            &mut painted,
        );
    }
    items.extend(painted.into_items());
    let dx = shift.translation().x;
    let rows = (l.rows.into_iter())
        .map(|r| BarRow {
            start: r.start + dx,
            zero: r.zero + dx,
            end: r.end + dx,
            numbers: (r.numbers.into_iter()).map(|(t, x)| (t, x + dx)).collect(),
            ..r
        })
        .collect();
    Drawing {
        items,
        rows,
        bounds: shift.transform_rect_bbox(l.bounds),
    }
}

/// The ground kilometers to a page point of a preview.
const PREVIEW_KM_PER_POINT: f64 = 10.0 / PT_PER_MM;

/// A sample of `bar` in the style `kind`, for a thumbnail, in a box of
/// `width_mm` by `height_mm` with its top left at the origin, in page
/// points. The sample fits its division value to the box.
pub fn preview(
    bar: &ScaleBarItem,
    kind: ScaleBarKind,
    width_mm: f64,
    height_mm: f64,
    fonts: &mut Fonts,
) -> Vec<Item> {
    let sample = ScaleBarItem {
        kind,
        fit: ScaleFit::AdjustDivisionValue,
        ..bar.clone()
    };
    let rect = Rect::new(0.0, 0.0, width_mm * PT_PER_MM, height_mm * PT_PER_MM);
    draw(&sample, rect, 1.0, PREVIEW_KM_PER_POINT, fonts).items
}

/// [`preview`] as an image of `scale` pixels per page point.
pub fn preview_rgba(
    bar: &ScaleBarItem,
    kind: ScaleBarKind,
    width_mm: f64,
    height_mm: f64,
    scale: f64,
    fonts: &mut Fonts,
) -> anyhow::Result<Rgba> {
    let mut list = DisplayList::new(width_mm * PT_PER_MM, height_mm * PT_PER_MM);
    list.items = preview(bar, kind, width_mm, height_mm, fonts);
    Ok(vale_render::render_rgba(&list, scale)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vale_store::{FurnitureText, TextCase};

    fn fonts() -> Fonts {
        crate::pipeline::fonts().unwrap()
    }

    fn texts(items: &[Item]) -> Vec<String> {
        (items.iter())
            .filter_map(|i| match i {
                Item::Glyphs(run) => Some(run.text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The marks and the other lines: the number of subpaths of the strokes.
    fn strokes(items: &[Item]) -> usize {
        (items.iter())
            .map(|i| match i {
                Item::Stroke { path, .. } => (path.elements().iter())
                    .filter(|e| matches!(e, kurbo::PathEl::MoveTo(_)))
                    .count(),
                _ => 0,
            })
            .sum()
    }

    fn is_nice(v: f64) -> bool {
        let p = 10f64.powf(v.log10().floor());
        [1.0, 2.0, 2.5, 5.0]
            .iter()
            .any(|m| (m * p - v).abs() < 1e-9 * v)
    }

    const RECT: Rect = Rect::new(10.0, 20.0, 210.0, 60.0);

    #[test]
    fn nice_division_values_are_1_2_2_5_and_5() {
        assert_eq!(nice_division(7.3), Some(5.0));
        assert_eq!(nice_division(2.6), Some(2.5));
        assert_eq!(nice_division(2.4), Some(2.0));
        assert_eq!(nice_division(1999.0), Some(1000.0));
        assert_eq!(nice_division(0.03), Some(0.025));
        assert_eq!(nice_division(0.0), None);
    }

    #[test]
    fn each_kind_draws_a_bar_that_fits_its_box() {
        let mut f = fonts();
        for kind in ScaleBarKind::ALL {
            let bar = ScaleBarItem {
                kind,
                subdivisions: 2,
                ..ScaleBarItem::default()
            };
            // 1.3 km to a point: the bar can be up to about 250 km long.
            let d = draw(&bar, RECT, 1.0, 1.3, &mut f);
            let fills = (d.items.iter()).filter(|i| matches!(i, Item::Fill { .. }));
            let has_fills = !matches!(kind, ScaleBarKind::Line | ScaleBarKind::SteppedLine);
            assert_eq!(fills.count() > 0, has_fills, "{kind:?}");
            assert!(strokes(&d.items) > 0, "{kind:?}");
            let row = &d.rows[0];
            assert!(is_nice(row.value), "{kind:?} {}", row.value);
            assert_eq!(row.divisions, 4);
            let want: Vec<String> = (0..=4)
                .map(|k| format!("{}", k as f64 * row.value))
                .collect();
            let numbers: Vec<String> = row.numbers.iter().map(|n| n.0.clone()).collect();
            assert_eq!(numbers, want, "{kind:?}");
            let mut shown = want.clone();
            shown.push("Kilometers".to_string());
            assert_eq!(texts(&d.items), shown, "{kind:?}");
            // The length is true, and the bar and its text are in the box,
            // and the next nice value would not fit.
            assert!(((row.end - row.zero) * 1.3 - 4.0 * row.value).abs() < 1e-9);
            assert!(
                d.bounds.x0 >= RECT.x0 && d.bounds.x1 <= RECT.x1,
                "{:?}",
                d.bounds
            );
            let next = nice_division(row.value * 1.01 * 5.0 / 2.0).unwrap();
            assert!(4.0 * next / 1.3 > RECT.width() - (row.end - row.start));
        }
    }

    #[test]
    fn subdivisions_and_the_division_before_zero_get_numbers() {
        let mut f = fonts();
        let bar = ScaleBarItem {
            fit: ScaleFit::AdjustWidth,
            division_value: Some(10.0),
            divisions: 3,
            subdivisions: 2,
            number_frequency: TickFrequency::DivisionsAndFirstSubdivisions,
            ..ScaleBarItem::default()
        };
        let numbers = |bar: &ScaleBarItem, f: &mut Fonts| {
            let d = draw(bar, RECT, 1.0, 0.05, f);
            (d.rows[0].numbers.iter())
                .map(|n| n.0.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(numbers(&bar, &mut f), ["0", "5", "10", "20", "30"]);
        let before = ScaleBarItem {
            before_zero: true,
            ..bar.clone()
        };
        assert_eq!(numbers(&before, &mut f), ["10", "5", "0", "10", "20", "30"]);
        let no_zero = ScaleBarItem {
            zero_label: false,
            ..before.clone()
        };
        assert_eq!(numbers(&no_zero, &mut f), ["10", "5", "10", "20", "30"]);
        let decimals = ScaleBarItem {
            division_value: Some(2.5),
            subdivisions: 0,
            decimals: Some(2),
            ..bar.clone()
        };
        assert_eq!(numbers(&decimals, &mut f), ["0.00", "2.50", "5.00", "7.50"]);
        let auto = ScaleBarItem {
            decimals: None,
            ..decimals
        };
        assert_eq!(numbers(&auto, &mut f), ["0", "2.5", "5", "7.5"]);
        let big = ScaleBarItem {
            division_value: Some(5000.0),
            subdivisions: 0,
            ..bar.clone()
        };
        let d = draw(&big, Rect::new(0.0, 0.0, 300.0, 40.0), 1.0, 100.0, &mut f);
        let shown: Vec<_> = d.rows[0].numbers.iter().map(|n| n.0.as_str()).collect();
        assert_eq!(shown, ["0", "5,000", "10,000", "15,000"]);
    }

    #[test]
    fn crowded_numbers_give_way_to_the_ends() {
        let mut f = fonts();
        let bar = ScaleBarItem {
            fit: ScaleFit::AdjustWidth,
            division_value: Some(1000.0),
            divisions: 10,
            ..ScaleBarItem::default()
        };
        // 10 divisions of 10 points cannot show 11 numbers of 4 digits.
        let d = draw(&bar, RECT, 1.0, 100.0, &mut f);
        let shown: Vec<_> = d.rows[0].numbers.iter().map(|n| n.0.as_str()).collect();
        assert_eq!(shown.first(), Some(&"0"));
        assert_eq!(shown.last(), Some(&"10,000"));
        assert!(shown.len() < 11, "{shown:?}");
    }

    #[test]
    fn each_mark_frequency_has_its_marks() {
        let mut f = fonts();
        for (freq, before, want) in [
            (TickFrequency::None, false, 0),
            (TickFrequency::Divisions, false, 5),
            (TickFrequency::DivisionsAndFirstMidpoint, false, 6),
            (TickFrequency::DivisionsAndFirstSubdivisions, false, 8),
            (TickFrequency::DivisionsAndSubdivisions, false, 17),
            (TickFrequency::Ends, false, 2),
            (TickFrequency::EndsAndZero, false, 2),
            (TickFrequency::EndsAndZero, true, 3),
            (TickFrequency::Divisions, true, 6),
        ] {
            let bar = ScaleBarItem {
                kind: ScaleBarKind::Line,
                subdivisions: 4,
                before_zero: before,
                mark_frequency: freq,
                number_frequency: TickFrequency::None,
                ..ScaleBarItem::default()
            };
            let d = draw(&bar, RECT, 1.0, 1.3, &mut f);
            // The baseline and the marks.
            assert_eq!(strokes(&d.items), 1 + want, "{freq:?} {before}");
        }
    }

    #[test]
    fn a_double_scale_shares_its_zero() {
        let mut f = fonts();
        let bar = ScaleBarItem {
            kind: ScaleBarKind::Line,
            unit: ScaleUnit::Miles,
            second_unit: Some(ScaleUnit::Kilometers),
            ..ScaleBarItem::default()
        };
        let km_per_point = 1.3;
        let d = draw(&bar, RECT, 1.0, km_per_point, &mut f);
        let [miles, km] = &d.rows[..] else {
            panic!("{:?}", d.rows)
        };
        assert_eq!(miles.zero, km.zero);
        for row in [miles, km] {
            let ground = (row.end - row.zero) * km_per_point;
            let want = f64::from(row.divisions) * row.value * row.unit.km();
            assert!((ground - want).abs() < 1e-9 * want, "{row:?}");
        }
        assert_eq!(miles.unit, ScaleUnit::Miles);
        // The miles are above the line and the kilometers below it.
        let y = |text: &str| {
            (d.items.iter())
                .find_map(|i| match i {
                    Item::Glyphs(run) if run.text == text => Some(run.glyphs[0].1.translation().y),
                    _ => None,
                })
                .unwrap()
        };
        assert!(y("Miles") < y("Kilometers"));
    }

    #[test]
    fn the_unit_label_has_its_text_style() {
        let mut f = fonts();
        let bar = ScaleBarItem {
            unit_style: FurnitureText {
                case: TextCase::Upper,
                halo: true,
                halo_color: [1, 2, 3],
                ..FurnitureText::sized(8.0)
            },
            unit_label_style: vale_store::UnitLabelStyle::Abbreviation,
            ..ScaleBarItem::default()
        };
        let d = draw(&bar, RECT, 1.0, 1.3, &mut f);
        assert_eq!(texts(&d.items).last().map(String::as_str), Some("KM"));
        let halo = (d.items.iter()).any(|i| {
            matches!(i, Item::Stroke { color, round: true, .. } if *color == vale_render::Color::rgb(1, 2, 3))
        });
        assert!(halo);
        // The halo is under the text.
        let halo_at = (d.items.iter()).position(|i| matches!(i, Item::Stroke { round: true, .. }));
        let text_at = (d.items.iter()).position(|i| matches!(i, Item::Glyphs(_)));
        assert!(halo_at < text_at);
    }

    #[test]
    fn a_preview_draws_each_kind() {
        let mut f = fonts();
        for kind in ScaleBarKind::ALL {
            let items = preview(&ScaleBarItem::default(), kind, 40.0, 10.0, &mut f);
            assert!(!texts(&items).is_empty(), "{kind:?}");
        }
        let image = preview_rgba(
            &ScaleBarItem::default(),
            ScaleBarKind::Hollow,
            40.0,
            10.0,
            2.0,
            &mut f,
        )
        .unwrap();
        assert!(image.data.chunks(4).any(|px| px[3] > 0));
    }
}
