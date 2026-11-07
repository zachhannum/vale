//! Cut, Copy, and Paste of the nodes of a field. The clipboard gets the
//! expression text of the nodes, and the app keeps the nodes too, so that a
//! paste of the same text gives the same chips with their formats.

use eframe::egui;

use super::{Field, Node};

/// The model item of a chip, which writes a list of nodes as text.
pub trait Token: Clone + Send + Sync + 'static {
    /// The expression text of `nodes`.
    fn text(nodes: &[Node<Self>]) -> String;
}

/// The nodes of the last copy, and their text.
#[derive(Clone)]
struct Clip<P> {
    text: String,
    nodes: Vec<Node<P>>,
}

fn clip_id() -> egui::Id {
    egui::Id::new("token-field-clip")
}

/// Puts the selection on the clipboard. Returns false if nothing is
/// selected.
pub fn copy<P: Token>(ctx: &egui::Context, field: &Field<P>) -> bool {
    let nodes = field.selected_nodes();
    if nodes.is_empty() {
        return false;
    }
    let text = P::text(&nodes);
    crate::clipboard::set(ctx, &text);
    ctx.data_mut(|d| d.insert_temp(clip_id(), Clip { text, nodes }));
    true
}

/// Puts the selection on the clipboard, and deletes it. Returns true if the
/// nodes changed.
pub fn cut<P: Token>(ctx: &egui::Context, field: &mut Field<P>) -> bool {
    if !copy(ctx, field) {
        return false;
    }
    field.replace(Vec::new());
    true
}

/// Puts the nodes of the last copy in place of the selection, if `text` is
/// their text. Returns false for other text.
pub fn paste<P: Token>(ctx: &egui::Context, field: &mut Field<P>, text: &str) -> bool {
    let clip: Option<Clip<P>> = ctx.data(|d| d.get_temp(clip_id()));
    match clip {
        Some(clip) if clip.text == text => {
            field.replace(clip.nodes);
            true
        }
        _ => false,
    }
}
