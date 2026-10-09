//! Filtering, scoring, greedy placement, and output.

use crate::collide::{GridIndex, OrientedBox};
use crate::model::*;
use crate::text::{Fonts, ShapedText};
use crate::{improve, line, point};
use kurbo::{Affine, Point, Rect};

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
}

/// One label to place: a feature, or one repeat of a line feature.
pub(crate) struct Instance {
    pub feature_index: usize,
    pub repeat: u32,
    /// Sorted by cost, ties keep generation order.
    pub candidates: Vec<Candidate>,
    /// Set when `candidates` is empty.
    pub empty_reason: Option<UnplacedReason>,
}

/// The placed set: which candidate each instance uses, and a grid of their boxes.
pub(crate) struct State {
    grid: GridIndex,
    pub chosen: Vec<Option<usize>>,
    scratch: Vec<u32>,
}

impl State {
    pub fn new(bounds: Rect, instance_count: usize) -> Self {
        State {
            grid: GridIndex::new(bounds, 32.0),
            chosen: vec![None; instance_count],
            scratch: Vec::new(),
        }
    }

    /// Pushes every placed instance other than `skip` whose boxes intersect `cand`.
    pub fn hits(
        &mut self,
        instances: &[Instance],
        cand: &Candidate,
        skip: usize,
        out: &mut Vec<usize>,
    ) {
        for b in &cand.boxes {
            self.grid.query(b.aabb(), &mut self.scratch);
            for &id in &self.scratch {
                let id = id as usize;
                if id == skip {
                    continue;
                }
                let Some(ci) = self.chosen[id] else { continue };
                if instances[id].candidates[ci]
                    .boxes
                    .iter()
                    .any(|o| o.intersects(b))
                {
                    out.push(id);
                }
            }
        }
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
            (Geometry::Point { .. }, Placement::Line(_))
            | (Geometry::Line { .. }, Placement::Point(_)) => {
                unplaced.push(fail(f, 0, UnplacedReason::GeometryMismatch));
                continue;
            }
            _ => {}
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

    // Fixed hard obstacles. `owner[i]` is the feature that owns box i, if any.
    let mut hard: Vec<OrientedBox> = Vec::new();
    let mut owner: Vec<Option<usize>> = Vec::new();
    let mut soft: Vec<(OrientedBox, f64)> = Vec::new();
    for o in &input.obstacles {
        match o.kind {
            ObstacleKind::Hard => {
                hard.push(OrientedBox::from_rect(o.rect));
                owner.push(None);
            }
            ObstacleKind::Soft { cost } => soft.push((OrientedBox::from_rect(o.rect), cost)),
        }
    }
    for (fi, _) in &valid {
        if let Geometry::Point {
            position,
            symbol_radius,
        } = &input.features[*fi].geometry
            && *symbol_radius > 0.0
        {
            let r = *symbol_radius;
            hard.push(OrientedBox::from_rect(Rect::new(
                position.x - r,
                position.y - r,
                position.x + r,
                position.y + r,
            )));
            owner.push(Some(*fi));
        }
    }
    let mut hard_grid = GridIndex::new(bounds, 32.0);
    for (i, b) in hard.iter().enumerate() {
        hard_grid.insert(i as u32, b.aabb());
    }

    // Segments of every line feature, for the crossing cost.
    let mut seg_grid = GridIndex::new(bounds, 32.0);
    let mut segs: Vec<(Point, Point, usize)> = Vec::new();
    for (fi, f) in input.features.iter().enumerate() {
        if let Geometry::Line { path, .. } = &f.geometry {
            for w in path.windows(2) {
                seg_grid.insert(
                    segs.len() as u32,
                    Rect::new(w[0].x, w[0].y, w[1].x, w[1].y).abs(),
                );
                segs.push((w[0], w[1], fi));
            }
        }
    }

    // Step 1: candidates.
    let mut instances: Vec<Instance> = Vec::new();
    for (fi, shaped) in &valid {
        let f = &input.features[*fi];
        let class = &input.classes[f.class];
        match (&f.geometry, &class.placement) {
            (
                Geometry::Point {
                    position,
                    symbol_radius,
                },
                Placement::Point(pp),
            ) => {
                instances.push(Instance {
                    feature_index: *fi,
                    repeat: 0,
                    candidates: point::candidates(*position, *symbol_radius, class, pp, shaped),
                    empty_reason: None,
                });
            }
            (Geometry::Line { path, stroke_width }, Placement::Line(lp)) => {
                let slots = line::slots(path, *stroke_width, class, lp, shaped, bounds);
                for (repeat, slot) in slots.into_iter().enumerate() {
                    let (candidates, empty_reason) = match slot {
                        Ok(c) if c.is_empty() => (c, Some(UnplacedReason::LineTooShort)),
                        Ok(c) => (c, None),
                        Err(r) => (Vec::new(), Some(r)),
                    };
                    instances.push(Instance {
                        feature_index: *fi,
                        repeat: repeat as u32,
                        candidates,
                        empty_reason,
                    });
                }
            }
            _ => {}
        }
    }

    // Step 2: filter and score.
    let mut hits: Vec<u32> = Vec::new();
    for inst in &mut instances {
        if inst.candidates.is_empty() {
            continue;
        }
        let own = inst.feature_index;
        let cands = std::mem::take(&mut inst.candidates);
        let mut kept: Vec<Candidate> = Vec::new();
        let mut obstructed = false;
        for mut c in cands {
            if !c.boxes.iter().all(|b| b.is_inside(bounds)) {
                continue;
            }
            let mut blocked = false;
            'hard: for b in &c.boxes {
                hard_grid.query(b.aabb(), &mut hits);
                for &h in &hits {
                    if owner[h as usize] == Some(own) {
                        continue;
                    }
                    if hard[h as usize].intersects(b) {
                        blocked = true;
                        break 'hard;
                    }
                }
            }
            if blocked {
                obstructed = true;
                continue;
            }
            for (rect, cost) in &soft {
                if c.boxes.iter().any(|b| rect.intersects(b)) {
                    c.cost += cost;
                }
            }
            let mut crossed: Vec<usize> = Vec::new();
            for b in &c.boxes {
                seg_grid.query(b.aabb(), &mut hits);
                for &s in &hits {
                    let (a, e, f) = segs[s as usize];
                    if f != own && !crossed.contains(&f) && b.intersects_segment(a, e) {
                        crossed.push(f);
                    }
                }
            }
            c.cost += 3.0 * crossed.len() as f64;
            kept.push(c);
        }
        if kept.is_empty() {
            inst.empty_reason = Some(if obstructed {
                UnplacedReason::Obstructed
            } else {
                UnplacedReason::OutOfBounds
            });
        }
        kept.sort_by(|a, b| a.cost.total_cmp(&b.cost));
        inst.candidates = kept;
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
        cb.priority
            .cmp(&ca.priority)
            .then_with(|| fb.priority.total_cmp(&fa.priority))
            .then_with(|| fa.id.cmp(&fb.id))
            .then_with(|| ia.repeat.cmp(&ib.repeat))
    });
    let mut state = State::new(bounds, instances.len());
    let dup = improve::Dup::new(
        instances
            .iter()
            .map(|inst| {
                let f = &input.features[inst.feature_index];
                (f.class, f.text.clone())
            })
            .collect(),
        input.options.duplicate_distance,
    );
    let mut conflict_buf: Vec<usize> = Vec::new();
    for &i in &order {
        let n = instances[i].candidates.len();
        for ci in 0..n {
            improve::conflicts(&instances, &mut state, &dup, i, ci, &mut conflict_buf);
            if conflict_buf.is_empty() {
                state.insert(&instances, i, ci);
                break;
            }
        }
    }

    // Step 4: improve.
    let pen = improve::penalties(&instances, &order);
    if input.options.improve {
        improve::improve(
            &instances,
            &mut state,
            &order,
            &dup,
            &pen,
            input.options.seed,
        );
    }
    let total_cost = improve::energy(&instances, &state, &pen);

    // Step 5: output.
    let shaped_of = |fi: usize| &valid.iter().find(|(v, _)| *v == fi).unwrap().1;
    let mut placed: Vec<PlacedLabel> = Vec::new();
    let mut rep_unplaced: Vec<UnplacedLabel> = Vec::new();
    for (i, inst) in instances.iter().enumerate() {
        let f = &input.features[inst.feature_index];
        match state.chosen[i] {
            Some(ci) => {
                let c = &inst.candidates[ci];
                let shaped = shaped_of(inst.feature_index);
                let mut t = c.transforms.iter();
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
                                transform: t.next().copied().unwrap_or(Affine::IDENTITY),
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
                });
            }
            None => {
                let reason = if inst.candidates.is_empty() {
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
