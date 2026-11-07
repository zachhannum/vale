//! A display list made ready to draw parts of it many times.
//!
//! The clips are nested, the glyph outlines and the images are made one time, and each item
//! has a page-space bounding box. A renderer skips the items that a region does not touch, so
//! the cost of a region follows the items in it, not the size of the list.

use std::io::Cursor;
use std::sync::Arc;

use kurbo::{Affine, BezPath, Rect, Shape};
use vello_cpu::Pixmap;

use crate::list::{Color, DisplayList, FillRule, Item, RenderError};
use crate::raster::glyph_run_outline;

/// The distance in pixels around a box that still counts as inside a region. It holds the
/// antialiased edge pixels.
const MARGIN: f64 = 1.0;

/// One node of a prepared list.
#[derive(Clone, Debug)]
pub(crate) enum Node {
    Fill {
        path: BezPath,
        color: Color,
        rule: FillRule,
        bbox: Rect,
    },
    Stroke {
        path: BezPath,
        color: Color,
        width: f64,
        round: bool,
        bbox: Rect,
        /// The boxes of runs of segments of a long path. A region that misses each of them
        /// draws nothing of the stroke.
        pieces: Vec<Rect>,
    },
    /// The outlines of a glyph run, filled with the nonzero rule.
    Glyphs {
        path: BezPath,
        color: Color,
        bbox: Rect,
    },
    /// A decoded image with premultiplied pixels. The box is `rect`.
    Image {
        pixmap: Arc<Pixmap>,
        rect: Rect,
        interpolate: bool,
    },
    Clip {
        path: BezPath,
        rule: FillRule,
        bbox: Rect,
        children: Vec<Node>,
    },
    Layer {
        alpha: f32,
        bbox: Rect,
        children: Vec<Node>,
    },
    /// The items of a `Rasterize` item. A raster backend draws them as usual.
    Rasterize { bbox: Rect, children: Vec<Node> },
}

impl Node {
    /// Whether the node touches a region of `w` by `h` pixels under `transform`.
    pub(crate) fn visible(&self, transform: Affine, w: f64, h: f64) -> bool {
        if !visible(transform, self.bbox(), w, h) {
            return false;
        }
        match self {
            Node::Stroke { pieces, .. } if !pieces.is_empty() => {
                pieces.iter().any(|p| visible(transform, *p, w, h))
            }
            _ => true,
        }
    }

    pub(crate) fn bbox(&self) -> Rect {
        match self {
            Node::Fill { bbox, .. }
            | Node::Stroke { bbox, .. }
            | Node::Glyphs { bbox, .. }
            | Node::Clip { bbox, .. }
            | Node::Layer { bbox, .. }
            | Node::Rasterize { bbox, .. } => *bbox,
            Node::Image { rect, .. } => *rect,
        }
    }
}

/// A display list that draws regions fast. See the module documentation.
#[derive(Clone, Debug)]
pub struct PreparedList {
    pub(crate) nodes: Vec<Node>,
    bounds: Rect,
    width: f64,
    height: f64,
}

impl PreparedList {
    /// Prepares the list. A `PopClip` with no open clip does nothing, and a clip that is open
    /// at the end of the list or of a `Layer` or `Rasterize` closes there. Fails when a PNG
    /// image does not decode.
    pub fn new(list: &DisplayList) -> Result<Self, RenderError> {
        let nodes = build(&list.items)?;
        let bounds = union(&nodes).unwrap_or(Rect::ZERO);
        Ok(PreparedList {
            nodes,
            bounds,
            width: list.width,
            height: list.height,
        })
    }

    /// The page-space box of all items. It can be larger than what the items draw.
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// The width of the page, in points.
    pub fn width(&self) -> f64 {
        self.width
    }

    /// The height of the page, in points.
    pub fn height(&self) -> f64 {
        self.height
    }

    /// The number of fills, strokes, glyph runs, and images that a region of `width` by
    /// `height` pixels under `transform` draws.
    pub fn visible_count(&self, transform: Affine, width: u32, height: u32) -> usize {
        fn count(nodes: &[Node], transform: Affine, w: f64, h: f64) -> usize {
            nodes
                .iter()
                .filter(|n| n.visible(transform, w, h))
                .map(|n| match n {
                    Node::Clip { children, .. }
                    | Node::Layer { children, .. }
                    | Node::Rasterize { children, .. } => count(children, transform, w, h),
                    _ => 1,
                })
                .sum()
        }
        count(&self.nodes, transform, width.into(), height.into())
    }
}

/// Whether a page-space box touches a region of `w` by `h` pixels under `transform`.
pub(crate) fn visible(transform: Affine, bbox: Rect, w: f64, h: f64) -> bool {
    let r = transform.transform_rect_bbox(bbox);
    r.x1 > -MARGIN && r.y1 > -MARGIN && r.x0 < w + MARGIN && r.y0 < h + MARGIN
}

fn union(nodes: &[Node]) -> Option<Rect> {
    nodes.iter().map(Node::bbox).reduce(|a, b| a.union(b))
}

/// The page-space box of a stroke. A miter join with the limit 4 reaches out by twice the
/// width, and a butt cap by half the width.
fn stroke_bbox(path: &BezPath, width: f64, round: bool) -> Rect {
    let reach = stroke_reach(width, round);
    path.bounding_box().inflate(reach, reach)
}

fn stroke_reach(width: f64, round: bool) -> f64 {
    let reach = if round { width / 2.0 } else { 2.0 * width };
    reach.abs() * 1.01
}

/// The most pieces of a stroke.
const MOST_PIECES: usize = 64;

/// The boxes of runs of segments of a path, at most `MOST_PIECES`, each grown by the reach
/// of the stroke. A path of one segment has no pieces.
fn stroke_pieces(path: &BezPath, width: f64, round: bool) -> Vec<Rect> {
    let reach = stroke_reach(width, round);
    let run_length = path.segments().count().div_ceil(MOST_PIECES).max(1);
    let mut pieces = Vec::new();
    let mut run: Option<Rect> = None;
    let mut count = 0;
    for segment in path.segments() {
        let b = segment.bounding_box();
        run = Some(run.map_or(b, |r| r.union(b)));
        count += 1;
        if count == run_length {
            pieces.extend(run.take().map(|r| r.inflate(reach, reach)));
            count = 0;
        }
    }
    pieces.extend(run.map(|r| r.inflate(reach, reach)));
    if pieces.len() < 2 {
        pieces.clear();
    }
    pieces
}

/// A clip with its path and the nodes in it.
struct Frame {
    path: BezPath,
    rule: FillRule,
    children: Vec<Node>,
}

impl Frame {
    /// The clip node, or `None` when the clip holds nothing.
    fn close(self) -> Option<Node> {
        let bbox = union(&self.children)?.intersect(self.path.bounding_box());
        Some(Node::Clip {
            path: self.path,
            rule: self.rule,
            bbox,
            children: self.children,
        })
    }
}

fn build(items: &[Item]) -> Result<Vec<Node>, RenderError> {
    let mut out = Vec::new();
    let mut open: Vec<Frame> = Vec::new();
    for item in items {
        let node = match item {
            Item::Fill { path, color, rule } => Some(Node::Fill {
                path: path.clone(),
                color: *color,
                rule: *rule,
                bbox: path.bounding_box(),
            }),
            Item::Stroke {
                path,
                color,
                width,
                round,
            } => Some(Node::Stroke {
                path: path.clone(),
                color: *color,
                width: *width,
                round: *round,
                bbox: stroke_bbox(path, *width, *round),
                pieces: stroke_pieces(path, *width, *round),
            }),
            Item::Glyphs(run) => {
                let path = glyph_run_outline(run);
                let bbox = path.bounding_box();
                Some(Node::Glyphs {
                    path,
                    color: run.color,
                    bbox,
                })
            }
            Item::Image {
                png,
                rect,
                interpolate,
            } => {
                let pixmap = Pixmap::from_png(Cursor::new(png))
                    .map_err(|e| RenderError::Png(format!("{e:?}")))?;
                (rect.width() > 0.0 && rect.height() > 0.0).then(|| Node::Image {
                    pixmap: Arc::new(pixmap),
                    rect: *rect,
                    interpolate: *interpolate,
                })
            }
            Item::Rasterize { items, .. } => {
                let children = build(items)?;
                union(&children).map(|bbox| Node::Rasterize { bbox, children })
            }
            Item::Layer { alpha, items } => {
                let children = build(items)?;
                union(&children).map(|bbox| Node::Layer {
                    alpha: *alpha,
                    bbox,
                    children,
                })
            }
            Item::PushClip { path, rule } => {
                open.push(Frame {
                    path: path.clone(),
                    rule: *rule,
                    children: Vec::new(),
                });
                None
            }
            Item::PopClip => open.pop().and_then(Frame::close),
        };
        if let Some(node) = node {
            open.last_mut()
                .map_or(&mut out, |f| &mut f.children)
                .push(node);
        }
    }
    while let Some(frame) = open.pop() {
        if let Some(node) = frame.close() {
            open.last_mut()
                .map_or(&mut out, |f| &mut f.children)
                .push(node);
        }
    }
    Ok(out)
}
