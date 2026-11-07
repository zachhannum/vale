//! The flow layout of a field: atoms in lines that wrap, and boxes that lay
//! out their children inside their frames.

use std::collections::HashMap;

use eframe::egui::{FontId, Pos2, Rect, Vec2, pos2, vec2};

use super::caret::Caret;
use super::{BoxKind, Boxed, Glyph, Look, Node};

/// The sizes of a field.
#[derive(Clone, Debug, PartialEq)]
pub struct Metrics {
    /// The height of a chip and of a line of text.
    pub chip: f32,
    pub font: FontId,
    /// The font of a badge, a group tag, and a slot placeholder.
    pub small: FontId,
    /// The font of a type glyph.
    pub glyph_font: FontId,
    /// The side of the square of a type glyph.
    pub glyph: f32,
    pub radius: f32,
    /// The padding of a chip at its left and its right.
    pub chip_pad: (f32, f32),
    /// The space between the parts of a chip.
    pub chip_gap: f32,
    /// The space between two atoms that are not both text.
    pub gap: f32,
    /// The space between two lines. A group tag is in this space.
    pub line_gap: f32,
    /// The padding of the field around its lines.
    pub pad: Vec2,
    /// The padding of a group or a block: left, right, and top and bottom.
    pub box_pad: (f32, f32, f32),
    /// The size of a line break chip.
    pub break_size: Vec2,
    /// The height of the caret.
    pub caret: f32,
}

impl Metrics {
    /// The sizes of the iPad layout, with touch-size chips.
    pub fn pad() -> Self {
        Metrics {
            chip: 32.0,
            font: FontId::proportional(15.0),
            small: FontId::proportional(11.0),
            glyph_font: FontId::monospace(10.5),
            glyph: 22.0,
            radius: 8.0,
            chip_pad: (5.0, 10.0),
            chip_gap: 6.0,
            gap: 3.0,
            line_gap: 12.0,
            pad: vec2(12.0, 12.0),
            box_pad: (6.0, 8.0, 2.0),
            break_size: vec2(24.0, 24.0),
            caret: 24.0,
        }
    }

    /// The sizes of the desktop layout.
    pub fn desktop() -> Self {
        Metrics {
            chip: 24.0,
            font: FontId::proportional(13.0),
            small: FontId::proportional(10.0),
            glyph_font: FontId::monospace(9.0),
            glyph: 17.0,
            radius: 6.0,
            chip_pad: (4.0, 7.0),
            chip_gap: 5.0,
            gap: 2.0,
            line_gap: 9.0,
            pad: vec2(7.0, 7.0),
            box_pad: (4.0, 6.0, 1.0),
            break_size: vec2(20.0, 18.0),
            caret: 16.0,
        }
    }
}

/// The space between the name of a slot and its frame.
pub const SLOT_NAME_GAP: f32 = 5.0;

/// True if the slot `b` with index `i` in its block shows the name of its
/// argument before its frame: a slot after the first, with nodes.
pub fn shows_name<P>(b: &Boxed<P>, i: usize) -> bool {
    b.kind == BoxKind::Slot && i > 0 && !b.children.is_empty() && !b.label.is_empty()
}

/// What a placed rectangle shows.
#[derive(Clone, Debug, PartialEq)]
pub enum PlacedKind {
    Char(char),
    Break,
    Chip,
    /// The frame of a box. It comes before the children of the box.
    Frame,
}

/// A rectangle of the layout and the path of its node.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub path: Vec<usize>,
    pub rect: Rect,
    pub kind: PlacedKind,
}

/// The layout of a field, from the top left of its lines.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub placed: Vec<Placed>,
    pub size: Vec2,
    /// Each place of the caret and the rectangle of the caret there.
    pub stops: Vec<(Caret, Rect)>,
}

impl Layout {
    pub fn translate(&mut self, by: Vec2) {
        for p in &mut self.placed {
            p.rect = p.rect.translate(by);
        }
        for (_, r) in &mut self.stops {
            *r = r.translate(by);
        }
    }

    /// The rectangle of the caret at `caret`.
    pub fn caret_rect(&self, caret: &Caret) -> Option<Rect> {
        self.stops.iter().find(|(c, _)| c == caret).map(|(_, r)| *r)
    }

    /// The rectangle of the node at `path`.
    pub fn rect_of(&self, path: &[usize]) -> Option<Rect> {
        self.placed.iter().find(|p| p.path == path).map(|p| p.rect)
    }

    /// The chip under `pos`.
    pub fn chip_at(&self, pos: Pos2) -> Option<&Placed> {
        (self.placed.iter().rev()).find(|p| p.kind == PlacedKind::Chip && p.rect.contains(pos))
    }

    /// The place of the caret nearest to `pos`. A place on the line of
    /// `pos` comes before a place on another line.
    pub fn stop_at(&self, pos: Pos2) -> Option<Caret> {
        self.stop_near(pos, |_| true)
    }

    /// The place of the caret nearest to `pos`, of the places that `allow`
    /// gives.
    pub fn stop_near(&self, pos: Pos2, allow: impl Fn(&Caret) -> bool) -> Option<Caret> {
        let cost = |r: &Rect| {
            let dy = if pos.y < r.top() {
                r.top() - pos.y
            } else {
                (pos.y - r.bottom()).max(0.0)
            };
            dy * 8.0 + (pos.x - r.center().x).abs()
        };
        (self.stops.iter())
            .filter(|(c, _)| allow(c))
            .min_by(|a, b| cost(&a.1).total_cmp(&cost(&b.1)))
            .map(|(c, _)| c.clone())
    }
}

/// Measures the width of a text in a font.
pub type Measure<'a> = dyn FnMut(&str, &FontId) -> f32 + 'a;

/// The text of a type glyph.
pub fn glyph_text(glyph: Glyph) -> &'static str {
    match glyph {
        Glyph::Text => "Aa",
        Glyph::Number => "#",
        Glyph::Bool => "",
    }
}

/// The width of a chip, and the left of its label and of its badge.
pub fn chip_parts(look: &Look, m: &Metrics, measure: &mut Measure) -> (f32, f32, Option<f32>) {
    let mut x = m.chip_pad.0;
    if look.glyph.is_some() {
        x += m.glyph + m.chip_gap;
    } else {
        x = m.chip_pad.1;
    }
    let label = x;
    x += measure(&look.label, &m.font);
    let badge = look.badge.as_ref().map(|b| {
        x += m.chip_gap;
        let at = x;
        x += 1.0 + m.chip_gap + measure(b, &m.small);
        at
    });
    (x + m.chip_pad.1, label, badge)
}

/// One unbreakable part of a line.
struct Unit {
    placed: Vec<Placed>,
    ends: Vec<(Vec<usize>, Rect)>,
    size: Vec2,
    text: bool,
    space: bool,
    breaks: bool,
}

impl Unit {
    fn new(text: bool) -> Self {
        Unit {
            placed: Vec::new(),
            ends: Vec::new(),
            size: Vec2::ZERO,
            text,
            space: false,
            breaks: false,
        }
    }
}

/// A list laid out in lines.
struct Flowed {
    placed: Vec<Placed>,
    /// The caret at the end of a list that is empty or ends with a line
    /// break, by the path of the list.
    ends: Vec<(Vec<usize>, Rect)>,
    size: Vec2,
}

struct Flow<'a, 'b> {
    m: &'a Metrics,
    measure: &'a mut Measure<'b>,
}

impl Flow<'_, '_> {
    fn list<P>(&mut self, nodes: &[Node<P>], path: &mut Vec<usize>, width: f32) -> Flowed {
        let m = self.m;
        let mut units: Vec<Unit> = Vec::new();
        let mut word: Option<Unit> = None;
        for (i, node) in nodes.iter().enumerate() {
            path.push(i);
            match node {
                Node::Char(c) if !c.is_whitespace() => {
                    let w = (self.measure)(&c.to_string(), &m.font);
                    let unit = word.get_or_insert_with(|| Unit::new(true));
                    let rect = Rect::from_min_size(pos2(unit.size.x, 0.0), vec2(w, m.chip));
                    unit.placed.push(Placed {
                        path: path.clone(),
                        rect,
                        kind: PlacedKind::Char(*c),
                    });
                    unit.size = vec2(unit.size.x + w, m.chip);
                }
                _ => {
                    if let Some(w) = word.take() {
                        units.push(w);
                    }
                    units.push(self.unit(node, path, width));
                }
            }
            path.pop();
        }
        if let Some(w) = word.take() {
            units.push(w);
        }
        self.lines(units, nodes, path, width)
    }

    fn unit<P>(&mut self, node: &Node<P>, path: &mut Vec<usize>, width: f32) -> Unit {
        let m = self.m;
        let mut unit = Unit::new(false);
        let place = |size: Vec2, kind: PlacedKind| {
            let rect = Rect::from_min_size(pos2(0.0, (m.chip - size.y).max(0.0) / 2.0), size);
            (rect, kind, vec2(size.x, size.y.max(m.chip)))
        };
        let (rect, kind, size) = match node {
            Node::Char(c) => {
                let w = (self.measure)(&c.to_string(), &m.font);
                unit.text = true;
                unit.space = true;
                place(vec2(w, m.chip), PlacedKind::Char(*c))
            }
            Node::Break => {
                unit.breaks = true;
                place(m.break_size, PlacedKind::Break)
            }
            Node::Chip(chip) => {
                let (w, _, _) = chip_parts(&chip.look, m, self.measure);
                place(vec2(w, m.chip), PlacedKind::Chip)
            }
            Node::Box(b) => {
                self.boxed(b, path, width, &mut unit);
                return unit;
            }
        };
        unit.placed.push(Placed {
            path: path.clone(),
            rect,
            kind,
        });
        unit.size = size;
        unit
    }

    fn boxed<P>(&mut self, b: &Boxed<P>, path: &mut Vec<usize>, width: f32, unit: &mut Unit) {
        let m = self.m;
        let (left, right, vert) = match b.kind {
            BoxKind::Slot => (4.0, 4.0, 0.0),
            _ => m.box_pad,
        };
        let title = match b.kind {
            BoxKind::Block => (self.measure)(&b.label, &m.font) + m.chip_gap,
            _ => 0.0,
        };
        let inner_width = (width - left - right - title).max(m.chip);
        let mut inner = self.list(&b.children, path, inner_width);
        if b.children.is_empty() {
            inner.size = match b.kind {
                BoxKind::Slot => vec2((self.measure)(&b.label, &m.small) + 12.0, m.chip),
                _ => vec2(m.chip * 0.75, m.chip),
            };
        }
        // A filled slot after the first shows the name of its argument
        // before its frame.
        let name = match path.last() {
            Some(&i) if shows_name(b, i) => (self.measure)(&b.label, &m.small) + SLOT_NAME_GAP,
            _ => 0.0,
        };
        let offset = vec2(name + left + title, vert);
        let size = vec2(
            left + title + inner.size.x + right,
            inner.size.y.max(m.chip) + 2.0 * vert,
        );
        unit.placed.push(Placed {
            path: path.clone(),
            rect: Rect::from_min_size(pos2(name, 0.0), size),
            kind: PlacedKind::Frame,
        });
        let size = size + vec2(name, 0.0);
        // The children are centered in the frame when the frame is taller.
        let dy = (size.y - 2.0 * vert - inner.size.y).max(0.0) / 2.0;
        for mut p in inner.placed {
            p.rect = p.rect.translate(offset + vec2(0.0, dy));
            unit.placed.push(p);
        }
        for (p, r) in inner.ends {
            unit.ends.push((p, r.translate(offset + vec2(0.0, dy))));
        }
        unit.size = size;
    }

    fn lines<P>(
        &mut self,
        units: Vec<Unit>,
        nodes: &[Node<P>],
        path: &[usize],
        width: f32,
    ) -> Flowed {
        let m = self.m;
        let mut out = Flowed {
            placed: Vec::new(),
            ends: Vec::new(),
            size: Vec2::ZERO,
        };
        let mut line: Vec<(f32, Unit)> = Vec::new();
        let mut x = 0.0f32;
        let mut top = 0.0f32;
        let mut prev_text = true;
        let finish = |line: &mut Vec<(f32, Unit)>, top: &mut f32, out: &mut Flowed| {
            let h = line.iter().map(|(_, u)| u.size.y).fold(m.chip, f32::max);
            for (ux, u) in line.drain(..) {
                let at = vec2(ux, *top + (h - u.size.y) / 2.0);
                out.size.x = out.size.x.max(ux + u.size.x);
                for mut p in u.placed {
                    p.rect = p.rect.translate(at);
                    out.placed.push(p);
                }
                for (p, r) in u.ends {
                    out.ends.push((p, r.translate(at)));
                }
            }
            *top += h + m.line_gap;
        };
        for unit in units {
            let gap = if line.is_empty() || (prev_text && unit.text) {
                0.0
            } else {
                m.gap
            };
            if !line.is_empty() && x + gap + unit.size.x > width && !unit.space {
                finish(&mut line, &mut top, &mut out);
                x = 0.0;
            }
            let gap = if line.is_empty() { 0.0 } else { gap };
            let ux = x + gap;
            x = ux + unit.size.x;
            prev_text = unit.text;
            let breaks = unit.breaks;
            line.push((ux, unit));
            if breaks {
                finish(&mut line, &mut top, &mut out);
                x = 0.0;
                prev_text = true;
            }
        }
        if !line.is_empty() {
            finish(&mut line, &mut top, &mut out);
        }
        let ends_open = nodes.is_empty() || matches!(nodes.last(), Some(Node::Break));
        if ends_open {
            if !nodes.is_empty() || top == 0.0 {
                // The empty last line.
                top += m.chip + m.line_gap;
            }
            let y = top - m.line_gap - m.chip / 2.0;
            let rect = Rect::from_center_size(pos2(1.0, y), vec2(2.0, m.caret));
            out.ends.push((path.to_vec(), rect));
        }
        out.size.y = (top - m.line_gap).max(m.chip);
        out
    }
}

/// Lays out `nodes` in lines of at most `width`.
pub fn layout<P>(nodes: &[Node<P>], m: &Metrics, width: f32, measure: &mut Measure) -> Layout {
    let mut flow = Flow { m, measure };
    let flowed = flow.list(nodes, &mut Vec::new(), width);
    let rects: HashMap<&[usize], &Placed> = flowed
        .placed
        .iter()
        .map(|p| (p.path.as_slice(), p))
        .collect();
    let ends: HashMap<&[usize], Rect> = flowed
        .ends
        .iter()
        .map(|(p, r)| (p.as_slice(), *r))
        .collect();
    let mut stops = Vec::new();
    let caret =
        |x: f32, r: &Rect| Rect::from_center_size(pos2(x, r.center().y), vec2(2.0, m.caret));
    // Each list to visit: its path, its nodes, and true if it takes the caret.
    type Visit<'n, P> = (Vec<usize>, &'n [Node<P>], bool);
    let mut walk_stack: Vec<Visit<P>> = vec![(Vec::new(), nodes, true)];
    while let Some((path, list, takes)) = walk_stack.pop() {
        for (i, node) in list.iter().enumerate() {
            if let Node::Box(b) = node {
                let mut p = path.clone();
                p.push(i);
                walk_stack.push((p, &b.children, b.kind.takes_caret()));
            }
        }
        if !takes {
            continue;
        }
        for i in 0..=list.len() {
            let mut p = path.clone();
            p.push(i);
            let rect = if i < list.len() {
                rects.get(p.as_slice()).map(|n| {
                    let text = matches!(n.kind, PlacedKind::Char(_));
                    let x = if text {
                        n.rect.left()
                    } else {
                        n.rect.left() - m.gap / 2.0
                    };
                    caret(x, &n.rect)
                })
            } else if let Some(end) = ends.get(path.as_slice()) {
                Some(*end)
            } else {
                p[path.len()] = i - 1;
                rects.get(p.as_slice()).map(|n| {
                    let text = matches!(n.kind, PlacedKind::Char(_));
                    let x = if text {
                        n.rect.right()
                    } else {
                        n.rect.right() + m.gap / 2.0
                    };
                    caret(x, &n.rect)
                })
            };
            if let Some(rect) = rect {
                stops.push((Caret::new(path.clone(), i), rect));
            }
        }
    }
    Layout {
        placed: flowed.placed,
        size: flowed.size,
        stops,
    }
}
