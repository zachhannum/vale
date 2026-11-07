//! A field that edits a line of text and chips, for the expression editor.
//!
//! The field holds a tree of nodes. A char of text and a chip are each one
//! atom of the line. A box, such as an optional group or a function block,
//! lays out its children inside its frame. The caret is a place in one list
//! of nodes: the root or the children of a box that takes the caret.
//!
//! The field does not know the model. An adapter, such as `template`, turns
//! a model into nodes, and the nodes back into the model.

mod caret;
mod layout;
mod paint;
pub mod template;
mod widget;

pub use caret::{Caret, Dir, Selection};
pub use layout::{Layout, Metrics, Placed, PlacedKind, layout};
pub use widget::{Options, Output, keep_focus, show};

/// The color family of a chip.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ChipKind {
    Field,
    Function,
    /// A literal value, such as a number or a text in quotes.
    Value,
    Operator,
    /// Text that does not parse.
    Code,
}

/// The type glyph at the left of a chip.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Glyph {
    Text,
    Number,
    Bool,
}

impl Glyph {
    pub fn of(kind: vale_store::FieldType) -> Glyph {
        use vale_store::FieldType;
        match kind {
            FieldType::Text => Glyph::Text,
            FieldType::Integer | FieldType::Real => Glyph::Number,
            FieldType::Bool => Glyph::Bool,
        }
    }
}

/// How a chip looks.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub kind: ChipKind,
    pub glyph: Option<Glyph>,
    pub label: String,
    /// A short note at the right, such as the format of a field.
    pub badge: Option<String>,
}

/// A chip and the model item that it stands for.
#[derive(Clone, Debug, PartialEq)]
pub struct Chip<P> {
    pub look: Look,
    pub data: P,
}

/// The kind of a box.
#[derive(Clone, Debug, PartialEq)]
pub enum BoxKind {
    /// Text that shows only when its fields have a value. It has a dashed
    /// frame and takes the caret.
    Group,
    /// A function block. Its label is the name of the function, and its
    /// children are slots. The block itself does not take the caret.
    Block,
    /// A slot of a block. It takes the caret, and shows its label while it
    /// is empty.
    Slot,
}

impl BoxKind {
    pub fn takes_caret(&self) -> bool {
        !matches!(self, BoxKind::Block)
    }
}

/// A box of nodes.
#[derive(Clone, Debug, PartialEq)]
pub struct Boxed<P> {
    pub kind: BoxKind,
    /// The tag of a group, the name of a block, or the placeholder of a slot.
    pub label: String,
    pub children: Vec<Node<P>>,
    /// The model item of the box, if the box has one, such as a function.
    pub data: Option<P>,
}

/// One node of a field.
#[derive(Clone, Debug, PartialEq)]
pub enum Node<P> {
    Char(char),
    /// A line break. It shows as a small chip and ends the line.
    Break,
    Chip(Chip<P>),
    Box(Boxed<P>),
}

impl<P> Node<P> {
    pub fn as_box(&self) -> Option<&Boxed<P>> {
        match self {
            Node::Box(b) => Some(b),
            _ => None,
        }
    }
}

/// The nodes of a field and its selection.
#[derive(Clone, Debug, PartialEq)]
pub struct Field<P> {
    pub nodes: Vec<Node<P>>,
    pub sel: Selection,
}

impl<P> Default for Field<P> {
    fn default() -> Self {
        Field {
            nodes: Vec::new(),
            sel: Selection::default(),
        }
    }
}

impl<P> Field<P> {
    /// A field with the caret at the end.
    pub fn new(nodes: Vec<Node<P>>) -> Self {
        let end = Caret::new(Vec::new(), nodes.len());
        Field {
            nodes,
            sel: Selection::at(end),
        }
    }

    /// The text of the field for a screen reader: the chars, the label of
    /// each chip, and the children of each box.
    pub fn plain(&self) -> String {
        fn walk<P>(nodes: &[Node<P>], out: &mut String) {
            for n in nodes {
                match n {
                    Node::Char(c) => out.push(*c),
                    Node::Break => out.push('\n'),
                    Node::Chip(c) => out.push_str(&c.look.label),
                    Node::Box(b) => walk(&b.children, out),
                }
            }
        }
        let mut out = String::new();
        walk(&self.nodes, &mut out);
        out
    }
}
