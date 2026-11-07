//! The caret, the selection, and the edits of a field.

use super::{Boxed, Field, Node};

/// A place between two nodes of a list. `path` is the path of the box that
/// holds the list, and the empty path is the root. `index` is the count of
/// the nodes before the place.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Caret {
    pub path: Vec<usize>,
    pub index: usize,
}

impl Caret {
    pub fn new(path: Vec<usize>, index: usize) -> Self {
        Caret { path, index }
    }

    /// The path of the node after the caret.
    pub fn node_path(&self) -> Vec<usize> {
        let mut p = self.path.clone();
        p.push(self.index);
        p
    }
}

/// A range of nodes in one list. The anchor stays, and the head moves.
/// Both are in the same list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Caret,
    pub head: Caret,
}

impl Selection {
    pub fn at(caret: Caret) -> Self {
        Selection {
            anchor: caret.clone(),
            head: caret,
        }
    }

    /// The nodes from `a` to `b` in the list at `path`.
    pub fn range(path: Vec<usize>, a: usize, b: usize) -> Self {
        Selection {
            anchor: Caret::new(path.clone(), a),
            head: Caret::new(path, b),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// The list and the range of the selection, from the lower index.
    pub fn span(&self) -> (&[usize], usize, usize) {
        let (a, b) = (self.anchor.index, self.head.index);
        (&self.head.path, a.min(b), a.max(b))
    }
}

/// The direction of a move of the caret.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Dir {
    Back,
    Forward,
}

impl<P: Clone> Field<P> {
    /// The list at `path`.
    pub fn list(&self, path: &[usize]) -> Option<&[Node<P>]> {
        let mut cur: &[Node<P>] = &self.nodes;
        for &i in path {
            match cur.get(i)? {
                Node::Box(b) => cur = &b.children,
                _ => return None,
            }
        }
        Some(cur)
    }

    fn list_mut(&mut self, path: &[usize]) -> Option<&mut Vec<Node<P>>> {
        let mut cur = &mut self.nodes;
        for &i in path {
            match cur.get_mut(i)? {
                Node::Box(b) => cur = &mut b.children,
                _ => return None,
            }
        }
        Some(cur)
    }

    /// The node at `path`.
    pub fn node(&self, path: &[usize]) -> Option<&Node<P>> {
        let (last, parent) = path.split_last()?;
        self.list(parent)?.get(*last)
    }

    /// The node at `path`, to change it.
    pub fn node_mut(&mut self, path: &[usize]) -> Option<&mut Node<P>> {
        let (last, parent) = path.split_last()?;
        self.list_mut(parent)?.get_mut(*last)
    }

    /// True if the list at `path` takes the caret.
    pub fn takes_caret(&self, path: &[usize]) -> bool {
        match self.node(path) {
            None => path.is_empty(),
            Some(Node::Box(b)) => b.kind.takes_caret(),
            Some(_) => false,
        }
    }

    /// Each place of the caret, in the order of the text.
    pub fn stops(&self) -> Vec<Caret> {
        fn walk<P>(nodes: &[Node<P>], path: &mut Vec<usize>, takes: bool, out: &mut Vec<Caret>) {
            for i in 0..=nodes.len() {
                if takes {
                    out.push(Caret::new(path.clone(), i));
                }
                if let Some(Node::Box(b)) = nodes.get(i) {
                    path.push(i);
                    walk(&b.children, path, b.kind.takes_caret(), out);
                    path.pop();
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.nodes, &mut Vec::new(), true, &mut out);
        out
    }

    /// Makes the selection valid after a change of the nodes.
    pub fn clamp(&mut self) {
        let stops = self.stops();
        if !stops.contains(&self.sel.head) {
            let head = (stops.iter())
                .filter(|s| s.path == self.sel.head.path)
                .min_by_key(|s| s.index.abs_diff(self.sel.head.index))
                .cloned()
                .unwrap_or_else(|| Caret::new(Vec::new(), self.nodes.len()));
            self.sel = Selection::at(head);
        }
        let len = self.list(&self.sel.head.path).map_or(0, <[_]>::len);
        if self.sel.anchor.path != self.sel.head.path || self.sel.anchor.index > len {
            self.sel.anchor = self.sel.head.clone();
        }
    }

    /// The node of a selection of one node, and its path.
    pub fn selected_node(&self) -> Option<(Vec<usize>, &Node<P>)> {
        let (path, a, b) = self.sel.span();
        if b != a + 1 {
            return None;
        }
        let mut p = path.to_vec();
        p.push(a);
        let node = self.node(&p)?;
        Some((p, node))
    }

    /// Selects the node at `path`.
    pub fn select_node(&mut self, path: &[usize]) {
        if let Some((last, parent)) = path.split_last() {
            self.sel = Selection::range(parent.to_vec(), *last, *last + 1);
        }
    }

    /// Replaces the selection with `new`, and puts the caret after it.
    pub fn replace(&mut self, new: Vec<Node<P>>) {
        let (path, a, b) = self.sel.span();
        let path = path.to_vec();
        let count = new.len();
        let Some(list) = self.list_mut(&path) else {
            return;
        };
        let b = b.min(list.len());
        let a = a.min(b);
        list.splice(a..b, new);
        self.sel = Selection::at(Caret::new(path, a + count));
    }

    /// Types `text` as chars.
    pub fn type_text(&mut self, text: &str) {
        let chars = text.chars().filter(|c| !c.is_control()).map(Node::Char);
        self.replace(chars.collect());
    }

    /// Deletes the selection. With no selection, deletes the char before
    /// the caret, or selects the chip or the box before it, so that a second
    /// press deletes it. Returns true if the nodes changed.
    pub fn backspace(&mut self) -> bool {
        self.delete(Dir::Back)
    }

    /// `backspace` in the other direction.
    pub fn delete_forward(&mut self) -> bool {
        self.delete(Dir::Forward)
    }

    fn delete(&mut self, dir: Dir) -> bool {
        if !self.sel.is_empty() {
            self.replace(Vec::new());
            return true;
        }
        let Caret { path, index } = self.sel.head.clone();
        let len = self.list(&path).map_or(0, <[_]>::len);
        let next = match dir {
            Dir::Back => index.checked_sub(1),
            Dir::Forward => (index < len).then_some(index),
        };
        let Some(k) = next else {
            // At the edge of a box, the box is next.
            if let Some((&last, parent)) = path.split_last() {
                let mut up = parent.to_vec();
                let mut at = last;
                while !self.takes_caret(&up) {
                    match up.pop() {
                        Some(i) => at = i,
                        None => break,
                    }
                }
                self.sel = Selection::range(up, at, at + 1);
            }
            return false;
        };
        let mut p = path.clone();
        p.push(k);
        match self.node(&p) {
            Some(Node::Char(_)) => {
                self.sel = Selection::range(path, k, k + 1);
                self.replace(Vec::new());
                true
            }
            Some(_) => {
                self.sel = Selection::range(path, k, k + 1);
                false
            }
            None => false,
        }
    }

    /// Moves the caret one place. With `extend`, moves the head of the
    /// selection in its list.
    pub fn step(&mut self, dir: Dir, extend: bool) {
        let head = self.sel.head.clone();
        if extend {
            let len = self.list(&head.path).map_or(0, <[_]>::len);
            let index = match dir {
                Dir::Back => head.index.saturating_sub(1),
                Dir::Forward => (head.index + 1).min(len),
            };
            self.sel.head = Caret::new(head.path, index);
            return;
        }
        if !self.sel.is_empty() {
            let (path, a, b) = self.sel.span();
            let index = if dir == Dir::Back { a } else { b };
            self.sel = Selection::at(Caret::new(path.to_vec(), index));
            return;
        }
        let stops = self.stops();
        let Some(i) = stops.iter().position(|s| *s == head) else {
            return;
        };
        let j = match dir {
            Dir::Back => i.checked_sub(1),
            Dir::Forward => (i + 1 < stops.len()).then_some(i + 1),
        };
        if let Some(j) = j {
            self.sel = Selection::at(stops[j].clone());
        }
    }

    /// Selects all nodes of the root.
    pub fn select_all(&mut self) {
        self.sel = Selection::range(Vec::new(), 0, self.nodes.len());
    }

    /// Puts the selected nodes in a new box, and selects the box.
    pub fn wrap(&mut self, make: impl FnOnce(Vec<Node<P>>) -> Boxed<P>) {
        let (path, a, b) = self.sel.span();
        let path = path.to_vec();
        let Some(list) = self.list_mut(&path) else {
            return;
        };
        let b = b.min(list.len());
        let inner: Vec<Node<P>> = list.drain(a.min(b)..b).collect();
        list.insert(a.min(b), Node::Box(make(inner)));
        self.sel = Selection::range(path, a, a + 1);
    }

    /// Calls `f` on each box, from the inside out.
    pub fn each_box(&mut self, f: &mut dyn FnMut(&mut Boxed<P>)) {
        fn walk<P>(nodes: &mut [Node<P>], f: &mut dyn FnMut(&mut Boxed<P>)) {
            for n in nodes {
                if let Node::Box(b) = n {
                    walk(&mut b.children, f);
                    f(b);
                }
            }
        }
        walk(&mut self.nodes, f);
    }
}
