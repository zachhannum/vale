//! Filtering, scoring, greedy placement, and output.

use crate::collide::{GridIndex, OrientedBox};
use crate::graticule::Parallels;
use crate::model::*;
use crate::obstacle::Obstacles;
use crate::text::{Fonts, ShapedText};
use crate::{fallback, improve, interleave, line, point, polygon};
use kurbo::{Affine, Point, Rect};
use std::collections::HashMap;

pub(crate) struct Candidate {
    pub kind: CandidateKind,
    /// Static cost, lower is better.
    pub cost: f64,
    /// One per glyph, in run order then glyph order.
    pub transforms: Vec<Affine>,
    /// Already grown by the class margin.
    pub boxes: Vec<OrientedBox>,
    /// Union of the box AABBs.
    pub bounds: Rect,
    pub leader: Option<Leader>,
    /// How far the glyphs can slide back and forward along their path, in page units.
    pub slide: [f64; 2],
}

/// One label to place: a feature, or one repeat of a line or polygon feature.
pub(crate) struct Instance {
    pub feature_index: usize,
    pub repeat: u32,
    /// Sorted by cost, ties keep generation order.
    pub candidates: Vec<Candidate>,
    /// Set when `candidates` is empty.
    pub empty_reason: Option<UnplacedReason>,
    /// The fallback tries, in the order of their candidates.
    pub variants: Vec<Variant>,
    /// The candidates that a forbidden obstacle blocks, kept for a class that never removes.
    pub blocked: Vec<Candidate>,
    /// The class is a background class.
    pub background: bool,
    /// The margin of a class whose margin is not a hard rule, else 0.
    pub soft_margin: f64,
}

/// The candidates of one fallback try, from `first` to the `first` of the next variant.
pub(crate) struct Variant {
    pub fallback: Fallback,
    pub shaped: ShapedText,
    pub first: usize,
}

impl Instance {
    fn new(
        feature_index: usize,
        repeat: u32,
        slot: Result<Vec<Candidate>, UnplacedReason>,
    ) -> Self {
        let (candidates, empty_reason) = match slot {
            Ok(c) => (c, None),
            Err(r) => (Vec::new(), Some(r)),
        };
        Instance {
            feature_index,
            repeat,
            candidates,
            empty_reason,
            variants: Vec::new(),
            blocked: Vec::new(),
            background: false,
            soft_margin: 0.0,
        }
    }

    /// The variant of candidate `ci`, or `None` for the label of the class.
    fn variant(&self, ci: usize) -> Option<&Variant> {
        self.variants.iter().rev().find(|v| ci >= v.first)
    }
}

/// The placed set: which candidate each instance uses, and a grid of their boxes.
pub(crate) struct State {
    grid: GridIndex,
    pub chosen: Vec<Option<usize>>,
    scratch: Vec<u32>,
    /// Soft margins count as hard. If it is `false`, only the text inside a soft margin
    /// collides.
    pub strict: bool,
}

impl State {
    pub fn new(bounds: Rect, instance_count: usize) -> Self {
        State {
            grid: GridIndex::new(bounds, 32.0),
            chosen: vec![None; instance_count],
            scratch: Vec::new(),
            strict: true,
        }
    }

    /// Pushes every placed instance other than `skip` whose boxes intersect `cand`, which
    /// belongs to `skip`. A background label and another label do not intersect.
    pub fn hits(
        &mut self,
        instances: &[Instance],
        cand: &Candidate,
        skip: usize,
        out: &mut Vec<usize>,
    ) {
        let own = &instances[skip];
        let soft = |m: f64| if self.strict { 0.0 } else { m };
        let shrink = soft(own.soft_margin);
        for b in &cand.boxes {
            self.grid.query(b.aabb(), &mut self.scratch);
            let b = b.inflate(-shrink);
            for &id in &self.scratch {
                let id = id as usize;
                if id == skip || instances[id].background != own.background {
                    continue;
                }
                let Some(ci) = self.chosen[id] else { continue };
                let other = soft(instances[id].soft_margin);
                if instances[id].candidates[ci]
                    .boxes
                    .iter()
                    .any(|o| o.inflate(-other).intersects(&b))
                {
                    out.push(id);
                }
            }
        }
    }

    /// Pushes every placed instance other than `skip` with a glyph box that touches `boxes`,
    /// background or not, once each.
    pub fn letters(
        &mut self,
        instances: &[Instance],
        boxes: &[OrientedBox],
        skip: usize,
        out: &mut Vec<usize>,
    ) {
        for b in boxes {
            self.grid.query(b.aabb(), &mut self.scratch);
            for &id in &self.scratch {
                let id = id as usize;
                if id == skip || out.contains(&id) || self.chosen[id].is_none() {
                    continue;
                }
                if self.boxes(instances, id).iter().any(|o| o.intersects(b)) {
                    out.push(id);
                }
            }
        }
    }

    /// The candidate of a placed instance.
    pub fn chosen_candidate<'a>(&self, instances: &'a [Instance], inst: usize) -> &'a Candidate {
        &instances[inst].candidates[self.chosen[inst].expect("placed instance")]
    }

    /// The glyph boxes of a placed instance.
    pub fn boxes<'a>(&self, instances: &'a [Instance], inst: usize) -> &'a [OrientedBox] {
        &self.chosen_candidate(instances, inst).boxes
    }

    pub fn insert(&mut self, instances: &[Instance], inst: usize, cand: usize) {
        for b in &instances[inst].candidates[cand].boxes {
            self.grid.insert(inst as u32, b.aabb());
        }
        self.chosen[inst] = Some(cand);
    }

    pub fn remove(&mut self, instances: &[Instance], inst: usize) {
        if let Some(cand) = self.chosen[inst].take() {
            for b in &instances[inst].candidates[cand].boxes {
                self.grid.remove(inst as u32, b.aabb());
            }
        }
    }
}

fn inside(bounds: Rect, p: Point) -> bool {
    p.x >= bounds.x0 && p.x <= bounds.x1 && p.y >= bounds.y0 && p.y <= bounds.y1
}

pub(crate) fn run(fonts: &mut Fonts, input: &LabelInput) -> Labeling {
    let bounds = input.bounds;
    let mut unplaced: Vec<UnplacedLabel> = Vec::new();
    let fail = |f: &Feature, repeat: u32, reason| UnplacedLabel {
        feature: f.id,
        class: f.class,
        repeat,
        text: f.text.clone(),
        reason,
    };

    // Step 1 of the pipeline: validate and shape.
    let mut valid: Vec<(usize, ShapedText)> = Vec::new();
    for (fi, f) in input.features.iter().enumerate() {
        if f.text.trim().is_empty() {
            unplaced.push(fail(f, 0, UnplacedReason::EmptyText));
            continue;
        }
        let Some(class) = input.classes.get(f.class) else {
            unplaced.push(fail(f, 0, UnplacedReason::InvalidClass));
            continue;
        };
        match (&f.geometry, &class.placement) {
            (Geometry::Point { .. }, Placement::Point(_))
            | (Geometry::Line { .. }, Placement::Line(_))
            | (Geometry::Polygon { .. }, Placement::Polygon(_)) => {}
            _ => {
                unplaced.push(fail(f, 0, UnplacedReason::GeometryMismatch));
                continue;
            }
        }
        let shaped = fonts.shape(&f.text, &class.style);
        if shaped.glyph_count() == 0 {
            unplaced.push(fail(f, 0, UnplacedReason::FontNotFound));
            continue;
        }
        if shaped.missing_glyphs > 0 {
            unplaced.push(fail(f, 0, UnplacedReason::MissingGlyphs));
            continue;
        }
        if let Geometry::Point { position, .. } = &f.geometry
            && !inside(bounds, *position)
        {
            unplaced.push(fail(f, 0, UnplacedReason::OutOfBounds));
            continue;
        }
        valid.push((fi, shaped));
    }
    let (merged, absorbed) = merge_lines(input, &valid);
    valid.retain(|(fi, _)| !absorbed.contains_key(fi));

    // The input obstacles and the features as obstacles, with the weights of their classes.
    let mut obstacles = Obstacles::new(bounds);
    for o in &input.obstacles {
        obstacles.add(o);
    }
    let mut line_keys: HashMap<usize, u32> = HashMap::new();
    for (fi, f) in input.features.iter().enumerate() {
        let placement = input.classes.get(f.class).map(|c| &c.placement);
        match &f.geometry {
            Geometry::Point {
                position,
                symbol_radius,
                ..
            } if *symbol_radius > 0.0 => {
                let weight = match placement {
                    Some(Placement::Point(pp)) => pp.weight,
                    _ => PointPlacement::default().weight,
                };
                let r = *symbol_radius;
                let rect = Rect::new(
                    position.x - r,
                    position.y - r,
                    position.x + r,
                    position.y + r,
                );
                obstacles.add_box(OrientedBox::from_rect(rect), weight, Some(fi));
            }
            Geometry::Line { path, .. } => {
                let weight = match placement {
                    Some(Placement::Line(lp)) => lp.weight,
                    _ => LinePlacement::default().weight,
                };
                let owner = absorbed.get(&fi).copied().unwrap_or(fi);
                let key = *line_keys
                    .entry(owner)
                    .or_insert_with(|| obstacles.line_key());
                obstacles.add_line(path, 0.0, weight, key, Some(owner));
            }
            Geometry::Polygon { rings } => {
                if let Some(Placement::Polygon(pp)) = placement {
                    obstacles.add_polygon(rings, pp.inside_weight, pp.outline_weight, Some(fi));
                }
            }
            _ => {}
        }
    }

    // The rings and bounds of every polygon feature, for the labels that avoid other polygons.
    let polygons: Vec<(usize, &[Vec<Point>], Rect)> = input
        .features
        .iter()
        .enumerate()
        .filter_map(|(fi, f)| match &f.geometry {
            Geometry::Polygon { rings } => {
                let mut pts = rings.iter().flatten();
                let first = pts.next()?;
                let bbox = pts.fold(Rect::from_points(*first, *first), |b, p| b.union_pt(*p));
                Some((fi, rings.as_slice(), bbox))
            }
            _ => None,
        })
        .collect();

    // Every point feature inside the bounds, for the labels that are not nearer to another point.
    let points = input
        .classes
        .iter()
        .any(|c| matches!(&c.placement, Placement::Point(pp) if pp.ambiguous != AmbiguousLabels::Keep))
        .then(|| Points::new(input));

    // Step 1: candidates.
    let parallels = Parallels::new(&input.parallels, bounds);
    let generator = Generator {
        input,
        parallels: &parallels,
        polygons: &polygons,
        merged: &merged,
        points: points.as_ref(),
    };
    let mut instances: Vec<Instance> = Vec::new();
    for (fi, shaped) in &valid {
        let f = &input.features[*fi];
        let class = &input.classes[f.class];
        for (repeat, slot) in generator.slots(*fi, class, shaped).into_iter().enumerate() {
            let mut inst = Instance::new(*fi, repeat as u32, slot);
            inst.background = class.background;
            inst.soft_margin = if class.margin_hard { 0.0 } else { class.margin };
            instances.push(inst);
        }
    }

    // Step 2: filter and score.
    let scorer = Scorer {
        bounds,
        obstacles: &obstacles,
    };
    for inst in &mut instances {
        if inst.candidates.is_empty() {
            continue;
        }
        let cands = std::mem::take(&mut inst.candidates);
        let (kept, blocked) = scorer.score(inst.feature_index, inst.soft_margin, cands);
        inst.blocked = blocked;
        match kept {
            Ok(kept) => inst.candidates = kept,
            Err(r) => inst.empty_reason = Some(r),
        }
    }

    // Step 3: place by priority.
    let mut order: Vec<usize> = (0..instances.len()).collect();
    order.sort_by(|&a, &b| {
        let (ia, ib) = (&instances[a], &instances[b]);
        let (fa, fb) = (
            &input.features[ia.feature_index],
            &input.features[ib.feature_index],
        );
        let (ca, cb) = (&input.classes[fa.class], &input.classes[fb.class]);
        cb.background
            .cmp(&ca.background)
            .then_with(|| cb.priority.cmp(&ca.priority))
            .then_with(|| fb.priority.total_cmp(&fa.priority))
            .then_with(|| fa.id.cmp(&fb.id))
            .then_with(|| ia.repeat.cmp(&ib.repeat))
    });
    let dup = improve::Dup::new(
        instances
            .iter()
            .map(|inst| {
                let f = &input.features[inst.feature_index];
                (f.class, f.text.clone())
            })
            .collect(),
        &input.classes,
    );
    let mut conflict_buf: Vec<usize> = Vec::new();
    let mut state = place(&instances, &order, &dup, input);

    // Step 4b: interleave. Labels slide so that their gaps hold the letters of the labels that
    // cross them. A label pays for each of its letters on a letter of a label that it does not
    // conflict with, because one of the two is a background label.
    let shaped_of = |fi: usize| &valid.iter().find(|(v, _)| *v == fi).unwrap().1;
    let texts: Vec<interleave::Text> = instances
        .iter()
        .map(|inst| {
            let f = &input.features[inst.feature_index];
            interleave::Text {
                shaped: shaped_of(inst.feature_index),
                margin: input.classes[f.class].margin,
            }
        })
        .collect();
    let mut aimed = interleave::aim(&instances, &mut state, &texts, |i, boxes| {
        let inst = &instances[i];
        scorer.obstacle_cost(inst.feature_index, inst.soft_margin, boxes)
    });
    let backgrounds = instances
        .iter()
        .enumerate()
        .any(|(i, inst)| inst.background && state.chosen[i].is_some());
    let mut changed = aimed.iter().any(|a| !a.is_empty());
    for (i, new) in aimed.iter_mut().enumerate() {
        instances[i].candidates.append(new);
    }
    if backgrounds {
        for i in 0..instances.len() {
            for ci in 0..instances[i].candidates.len() {
                let boxes = &instances[i].candidates[ci].boxes;
                let n = interleave::on_letters(&instances, &mut state, boxes, i);
                if n > 0 {
                    instances[i].candidates[ci].cost += interleave::ON_LETTER * n as f64;
                    changed = true;
                }
            }
        }
    }
    if changed {
        for (i, inst) in instances.iter_mut().enumerate() {
            let chosen = state.chosen[i];
            let mut ranked: Vec<(usize, Candidate)> = std::mem::take(&mut inst.candidates)
                .into_iter()
                .enumerate()
                .collect();
            ranked.sort_by(|a, b| a.1.cost.total_cmp(&b.1.cost));
            state.chosen[i] = chosen.and_then(|c| ranked.iter().position(|(k, _)| *k == c));
            inst.candidates = ranked.into_iter().map(|(_, c)| c).collect();
        }
        interleave::settle(&instances, &mut state, &order, &dup);
    }
    let pen = improve::penalties(&instances, &order);

    // Step 5: fall back.
    for &i in &order {
        if state.chosen[i].is_some() {
            continue;
        }
        let fi = instances[i].feature_index;
        let f = &input.features[fi];
        let class = &input.classes[f.class];
        'fallbacks: for fb in fallback::order(&class.fallbacks) {
            for attempt in fallback::attempts(fonts, &f.text, class, fb) {
                let slot = generator
                    .slots(fi, &attempt.class, &attempt.shaped)
                    .into_iter()
                    .nth(instances[i].repeat as usize);
                let Some(Ok(mut cands)) = slot else { continue };
                if fb == Fallback::Leader {
                    if let Geometry::Point {
                        position,
                        symbol_radius,
                        ..
                    } = &f.geometry
                    {
                        for c in &mut cands {
                            c.leader = Some(point::leader(
                                *position,
                                *symbol_radius,
                                c.bounds.inflate(-class.margin, -class.margin),
                            ));
                        }
                    } else {
                        cands.retain(|c| c.leader.is_some());
                    }
                }
                let Ok(cands) = scorer.score(fi, instances[i].soft_margin, cands).0 else {
                    continue;
                };
                let first = instances[i].candidates.len();
                instances[i].candidates.extend(cands);
                instances[i].variants.push(Variant {
                    fallback: attempt.fallback,
                    shaped: attempt.shaped,
                    first,
                });
                let n = instances[i].candidates.len();
                if let Some(ci) =
                    first_free(&instances, &mut state, &dup, i, first..n, &mut conflict_buf)
                {
                    state.insert(&instances, i, ci);
                    break 'fallbacks;
                }
            }
        }
    }

    // A label that is still out can use the soft margins of other labels, and its own.
    if instances.iter().any(|inst| inst.soft_margin > 0.0) {
        state.strict = false;
        for &i in &order {
            let n = instances[i].candidates.len();
            if state.chosen[i].is_none()
                && let Some(ci) =
                    first_free(&instances, &mut state, &dup, i, 0..n, &mut conflict_buf)
            {
                state.insert(&instances, i, ci);
            }
        }
        state.strict = true;
    }

    // A class that never removes places each label that is still out at its best candidate,
    // over other labels and obstacles.
    for &i in &order {
        let inst = &mut instances[i];
        if state.chosen[i].is_some()
            || !input.classes[input.features[inst.feature_index].class].never_remove
        {
            continue;
        }
        if inst.candidates.is_empty() && !inst.blocked.is_empty() {
            inst.candidates.push(inst.blocked.swap_remove(0));
        }
        if !inst.candidates.is_empty() {
            state.insert(&instances, i, 0);
        }
    }
    let total_cost = improve::energy(&instances, &state, &pen);

    // Step 6: output.
    let mut placed: Vec<PlacedLabel> = Vec::new();
    let mut rep_unplaced: Vec<UnplacedLabel> = Vec::new();
    for (i, inst) in instances.iter().enumerate() {
        let f = &input.features[inst.feature_index];
        match state.chosen[i] {
            Some(ci) => {
                let c = &inst.candidates[ci];
                let variant = inst.variant(ci);
                let shaped = variant.map_or_else(|| shaped_of(inst.feature_index), |v| &v.shaped);
                let mut t = c.transforms.iter();
                let narrow = Affine::scale_non_uniform(shaped.scale_x, 1.0);
                let glyph_runs = shaped
                    .runs
                    .iter()
                    .map(|run| PlacedGlyphRun {
                        font: run.font.clone(),
                        font_size: run.font_size,
                        normalized_coords: run.normalized_coords.clone(),
                        glyphs: run
                            .glyphs
                            .iter()
                            .map(|g| PlacedGlyph {
                                id: g.id,
                                transform: t.next().copied().unwrap_or(Affine::IDENTITY) * narrow,
                            })
                            .collect(),
                    })
                    .collect();
                placed.push(PlacedLabel {
                    feature: f.id,
                    class: f.class,
                    repeat: inst.repeat,
                    text: f.text.clone(),
                    kind: c.kind,
                    glyph_runs,
                    boxes: c.boxes.clone(),
                    bounds: c.bounds,
                    cost: c.cost,
                    leader: c.leader,
                    fallback: variant.map(|v| v.fallback),
                });
            }
            None => {
                let reason = if inst.candidates.is_empty()
                    || inst.variants.first().is_some_and(|v| v.first == 0)
                {
                    inst.empty_reason.unwrap_or(UnplacedReason::Obstructed)
                } else {
                    UnplacedReason::Collision
                };
                rep_unplaced.push(fail(f, inst.repeat, reason));
            }
        }
    }
    unplaced.extend(rep_unplaced);
    placed.sort_by_key(|p| (p.feature, p.repeat));
    unplaced.sort_by_key(|u| (u.feature, u.repeat));
    Labeling {
        placed,
        unplaced,
        total_cost,
    }
}

/// Ends of lines closer than this, in page units, meet.
const MERGE_TOLERANCE: f64 = 0.01;

/// The lines of classes with [`LinePlacement::merge_connected`] that meet end to end and have
/// the same text, joined into one path for the first feature of each chain. The second map
/// gives the first feature of the chain of each other feature.
fn merge_lines(
    input: &LabelInput,
    valid: &[(usize, ShapedText)],
) -> (HashMap<usize, Vec<Point>>, HashMap<usize, usize>) {
    let mut groups: Vec<((usize, &str), Vec<usize>)> = Vec::new();
    for (fi, _) in valid {
        let f = &input.features[*fi];
        let merges = matches!(
            &input.classes[f.class].placement,
            Placement::Line(lp) if lp.merge_connected
        );
        if !merges || !matches!(&f.geometry, Geometry::Line { path, .. } if path.len() >= 2) {
            continue;
        }
        let key = (f.class, f.text.as_str());
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, g)) => g.push(*fi),
            None => groups.push((key, vec![*fi])),
        }
    }
    let path_of = |fi: usize| match &input.features[fi].geometry {
        Geometry::Line { path, .. } => path.as_slice(),
        _ => &[],
    };
    let meet = |a: Point, b: Point| a.distance(b) <= MERGE_TOLERANCE;
    let mut merged = HashMap::new();
    let mut absorbed = HashMap::new();
    for (_, group) in groups {
        let mut used = vec![false; group.len()];
        for i in 0..group.len() {
            if used[i] {
                continue;
            }
            used[i] = true;
            let mut path = path_of(group[i]).to_vec();
            let mut joined = false;
            // Grow the chain at its end, then at its start.
            for at_end in [true, false] {
                loop {
                    let tip = if at_end {
                        path[path.len() - 1]
                    } else {
                        path[0]
                    };
                    let next = (0..group.len()).find_map(|j| {
                        let p = path_of(group[j]);
                        let (first, last) = (p[0], p[p.len() - 1]);
                        (!used[j] && (meet(tip, first) || meet(tip, last)))
                            .then_some((j, meet(tip, first)))
                    });
                    let Some((j, from_first)) = next else { break };
                    used[j] = true;
                    absorbed.insert(group[j], group[i]);
                    joined = true;
                    let mut piece = path_of(group[j]).to_vec();
                    if from_first != at_end {
                        piece.reverse();
                    }
                    if at_end {
                        path.extend_from_slice(&piece[1..]);
                    } else {
                        piece.pop();
                        piece.extend_from_slice(&path);
                        path = piece;
                    }
                }
            }
            if joined {
                merged.insert(group[i], path);
            }
        }
    }
    (merged, absorbed)
}

/// Makes the candidates of a feature.
struct Generator<'a> {
    input: &'a LabelInput,
    parallels: &'a Parallels,
    /// The index, rings, and bounds of each polygon feature.
    polygons: &'a [(usize, &'a [Vec<Point>], Rect)],
    /// The joined path of each line feature that absorbed connected lines.
    merged: &'a HashMap<usize, Vec<Point>>,
    /// Set when a class removes ambiguous point labels.
    points: Option<&'a Points>,
}

/// The point features inside the bounds.
struct Points {
    /// The index, class, position, and symbol radius of each point feature.
    list: Vec<(usize, usize, Point, f64)>,
    /// The symbol bounds of each point, by index in `list`.
    grid: GridIndex,
}

impl Points {
    fn new(input: &LabelInput) -> Self {
        let mut list = Vec::new();
        let mut grid = GridIndex::new(input.bounds, 32.0);
        for (fi, f) in input.features.iter().enumerate() {
            if let Geometry::Point {
                position,
                symbol_radius,
                ..
            } = &f.geometry
                && inside(input.bounds, *position)
            {
                let r = symbol_radius.max(0.0);
                grid.insert(
                    list.len() as u32,
                    Rect::new(
                        position.x - r,
                        position.y - r,
                        position.x + r,
                        position.y + r,
                    ),
                );
                list.push((fi, f.class, *position, r));
            }
        }
        Points { list, grid }
    }

    /// Removes the candidates of point feature `fi` whose text is nearer to another point than
    /// to its own point.
    fn unambiguous(
        &self,
        input: &LabelInput,
        fi: usize,
        class: &LabelClass,
        mode: AmbiguousLabels,
        cands: Vec<Candidate>,
    ) -> Result<Vec<Candidate>, UnplacedReason> {
        let f = &input.features[fi];
        let Geometry::Point {
            position,
            symbol_radius,
            outline,
            ..
        } = &f.geometry
        else {
            return Ok(cands);
        };
        if cands.is_empty() {
            return Ok(cands);
        }
        let mut near: Vec<u32> = Vec::new();
        let kept: Vec<Candidate> = cands
            .into_iter()
            .filter(|c| {
                let ink: Vec<OrientedBox> =
                    c.boxes.iter().map(|b| b.inflate(-class.margin)).collect();
                let gap = |p: Point, r: f64| {
                    ink.iter()
                        .map(|b| b.distance_to_point(p) - r)
                        .fold(f64::INFINITY, f64::min)
                        .max(0.0)
                };
                let own = match outline {
                    Some(ring) => ink
                        .iter()
                        .map(|b| point::outline_gap(b, ring))
                        .fold(f64::INFINITY, f64::min),
                    None => gap(*position, symbol_radius.max(0.0)),
                };
                self.grid.query(c.bounds.inflate(own, own), &mut near);
                !near.iter().any(|&i| {
                    let (ofi, oclass, p, r) = self.list[i as usize];
                    ofi != fi
                        && (mode == AmbiguousLabels::AllClasses || oclass == f.class)
                        && gap(p, r) < own - 1e-9
                })
            })
            .collect();
        if kept.is_empty() {
            Err(UnplacedReason::Ambiguous)
        } else {
            Ok(kept)
        }
    }
}

impl Generator<'_> {
    /// The label slots of feature `fi` in `class`, one for each instance, each with its
    /// candidates or the reason that it has none.
    fn slots(
        &self,
        fi: usize,
        class: &LabelClass,
        shaped: &ShapedText,
    ) -> Vec<Result<Vec<Candidate>, UnplacedReason>> {
        let f = &self.input.features[fi];
        let bounds = self.input.bounds;
        match (&f.geometry, &class.placement) {
            (
                Geometry::Point {
                    position,
                    symbol_radius,
                    angle,
                    outline,
                },
                Placement::Point(pp),
            ) => {
                let symbol = point::PointSymbol {
                    position: *position,
                    radius: *symbol_radius,
                    angle: *angle,
                    outline: outline.as_deref(),
                };
                let cands = point::candidates(&symbol, class, pp, shaped, self.parallels);
                vec![match self.points {
                    Some(points) if pp.ambiguous != AmbiguousLabels::Keep => {
                        points.unambiguous(self.input, fi, class, pp.ambiguous, cands)
                    }
                    _ => Ok(cands),
                }]
            }
            (Geometry::Line { path, stroke_width }, Placement::Line(lp)) => {
                let path = self.merged.get(&fi).unwrap_or(path);
                line::slots(path, *stroke_width, class, lp, shaped, bounds)
                    .into_iter()
                    .map(|slot| match slot {
                        Ok(c) if c.is_empty() => Err(UnplacedReason::LineTooShort),
                        other => other,
                    })
                    .collect()
            }
            (Geometry::Polygon { rings }, Placement::Polygon(pp)) => {
                let others: Vec<&[Vec<Point>]> = if pp.avoid_other_polygons {
                    let own = self.polygons.iter().find(|p| p.0 == fi).map(|p| p.2);
                    self.polygons
                        .iter()
                        .filter(|(o, _, b)| {
                            *o != fi && own.is_some_and(|r| r.intersect(*b).area() > 0.0)
                        })
                        .map(|p| p.1)
                        .collect()
                } else {
                    Vec::new()
                };
                polygon::slots(rings, &others, class, pp, shaped, bounds, self.parallels)
            }
            _ => Vec::new(),
        }
    }
}

/// The first candidate of `range` that no placed label blocks.
/// Places the instances greedily in `order`, then improves the placement.
fn place(instances: &[Instance], order: &[usize], dup: &improve::Dup, input: &LabelInput) -> State {
    let mut state = State::new(input.bounds, instances.len());
    let mut buf: Vec<usize> = Vec::new();
    for &i in order {
        let n = instances[i].candidates.len();
        if let Some(ci) = first_free(instances, &mut state, dup, i, 0..n, &mut buf) {
            state.insert(instances, i, ci);
        }
    }
    if input.options.improve {
        let pen = improve::penalties(instances, order);
        improve::improve(instances, &mut state, order, dup, &pen, input.options.seed);
    }
    state
}

fn first_free(
    instances: &[Instance],
    state: &mut State,
    dup: &improve::Dup,
    i: usize,
    range: std::ops::Range<usize>,
    buf: &mut Vec<usize>,
) -> Option<usize> {
    range.into_iter().find(|&ci| {
        improve::conflicts(instances, state, dup, i, ci, buf);
        buf.is_empty()
    })
}

/// Removes the candidates that leave the bounds or hit a forbidden obstacle, and adds the costs
/// of the other obstacles.
struct Scorer<'a> {
    bounds: Rect,
    obstacles: &'a Obstacles,
}

impl Scorer<'_> {
    /// The cost of the obstacles under `boxes` for feature `own`, or `None` if the boxes leave
    /// the bounds or a forbidden obstacle blocks them.
    fn obstacle_cost(&self, own: usize, soft_margin: f64, boxes: &[OrientedBox]) -> Option<f64> {
        if !boxes.iter().all(|b| b.is_inside(self.bounds)) {
            return None;
        }
        let text: Vec<OrientedBox> = boxes.iter().map(|b| b.inflate(-soft_margin)).collect();
        self.obstacles
            .cost(own, &text, &mut Vec::new(), &mut Vec::new())
    }

    /// The kept candidates of feature `own`, sorted by cost, or the reason that none is kept,
    /// and the candidates that a forbidden obstacle blocks, sorted by cost. Only the text inside
    /// a soft margin of `soft_margin` avoids obstacles.
    fn score(
        &self,
        own: usize,
        soft_margin: f64,
        cands: Vec<Candidate>,
    ) -> (Result<Vec<Candidate>, UnplacedReason>, Vec<Candidate>) {
        let mut hits: Vec<u32> = Vec::new();
        let mut keys: Vec<u32> = Vec::new();
        let mut kept: Vec<Candidate> = Vec::new();
        let mut blocked: Vec<Candidate> = Vec::new();
        for mut c in cands {
            if !c.boxes.iter().all(|b| b.is_inside(self.bounds)) {
                continue;
            }
            let text: Vec<OrientedBox>;
            let boxes = if soft_margin > 0.0 {
                text = c.boxes.iter().map(|b| b.inflate(-soft_margin)).collect();
                &text
            } else {
                &c.boxes
            };
            let cost = self.obstacles.cost(own, boxes, &mut hits, &mut keys);
            // A leader line does not cross a forbidden obstacle other than its own feature.
            let leader_blocked = c.leader.is_some_and(|l| {
                self.obstacles
                    .blocks_segment(own, l.anchor, l.end, &mut hits)
            });
            match cost {
                Some(cost) if !leader_blocked => {
                    c.cost += cost;
                    kept.push(c);
                }
                _ => blocked.push(c),
            }
        }
        blocked.sort_by(|a, b| a.cost.total_cmp(&b.cost));
        if kept.is_empty() {
            let reason = if blocked.is_empty() {
                UnplacedReason::OutOfBounds
            } else {
                UnplacedReason::Obstructed
            };
            return (Err(reason), blocked);
        }
        kept.sort_by(|a, b| a.cost.total_cmp(&b.cost));
        (Ok(kept), blocked)
    }
}
