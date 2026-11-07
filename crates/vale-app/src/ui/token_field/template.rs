//! A label text template as the nodes of a field: text as chars, fields
//! as chips, a line break as a chip, and a group as a box.

use vale_style::expr::editor::{Case, Empty, FieldFormat, Piece, Template};

use super::{BoxKind, Boxed, Caret, ChipKind, Field, Glyph, Look, Node, Selection};
use crate::ui::expr_help::ExprHelp;

/// The most chars of the label of an expression chip.
const MAX_LABEL: usize = 24;

/// The badge of a field chip: the format, in short.
pub fn badge(format: &FieldFormat) -> Option<String> {
    let mut parts = Vec::new();
    if format.thousands {
        let places = format.places.unwrap_or(0).max(0) as usize;
        let decimals = if places > 0 {
            format!(".{}", "0".repeat(places.min(3)))
        } else {
            String::new()
        };
        parts.push(format!("1,234{decimals}"));
    } else if let Some(p) = format.places {
        parts.push(format!("{p} dp"));
    }
    match format.case {
        Case::AsIs => {}
        Case::Upper => parts.push("ABC".into()),
        Case::Lower => parts.push("abc".into()),
        Case::Title => parts.push("Abc".into()),
    }
    if format.trim {
        parts.push("Trim".into());
    }
    match &format.empty {
        Empty::None => {}
        Empty::Text(t) => parts.push(format!("or “{t}”")),
        Empty::Field(f) => parts.push(format!("or {f}")),
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// The look of the chip of field `name`.
pub fn field_look(name: &str, format: &FieldFormat, help: &ExprHelp) -> Look {
    Look {
        kind: ChipKind::Field,
        glyph: Some(help.field_type(name).map_or(Glyph::Text, Glyph::of)),
        label: name.to_owned(),
        badge: badge(format),
    }
}

fn short(text: &str) -> String {
    if text.chars().count() <= MAX_LABEL {
        return text.to_owned();
    }
    let mut s: String = text.chars().take(MAX_LABEL - 1).collect();
    s.push('…');
    s
}

/// The node of a field chip.
pub fn field_node(name: &str, format: FieldFormat, help: &ExprHelp) -> Node<Piece> {
    Node::Chip(super::Chip {
        look: field_look(name, &format, help),
        data: Piece::Field {
            name: name.to_owned(),
            format,
        },
    })
}

/// The nodes of a list of pieces.
pub fn nodes_of(pieces: &[Piece], help: &ExprHelp) -> Vec<Node<Piece>> {
    let mut out = Vec::new();
    for p in pieces {
        match p {
            Piece::Text(s) => out.extend(s.chars().map(Node::Char)),
            Piece::LineBreak => out.push(Node::Break),
            Piece::Field { name, format } => out.push(field_node(name, format.clone(), help)),
            Piece::Group(items) => {
                let children = nodes_of(items, help);
                out.push(Node::Box(Boxed {
                    kind: BoxKind::Group,
                    label: group_label(&children),
                    children,
                    data: None,
                }));
            }
            Piece::Expr(seq) => out.push(Node::Chip(super::Chip {
                look: Look {
                    kind: ChipKind::Function,
                    glyph: None,
                    label: short(&seq.to_text()),
                    badge: None,
                },
                data: p.clone(),
            })),
            Piece::Code(s) => out.push(Node::Chip(super::Chip {
                look: Look {
                    kind: ChipKind::Code,
                    glyph: None,
                    label: short(s),
                    badge: None,
                },
                data: p.clone(),
            })),
        }
    }
    out
}

/// The field of a template, with the caret at the end.
pub fn field_of(template: &Template, help: &ExprHelp) -> Field<Piece> {
    Field::new(nodes_of(&template.pieces, help))
}

/// The pieces of a list of nodes. A group with no nodes goes.
pub fn pieces_of(nodes: &[Node<Piece>]) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut text = String::new();
    for n in nodes {
        if let Node::Char(c) = n {
            text.push(*c);
            continue;
        }
        if !text.is_empty() {
            out.push(Piece::Text(std::mem::take(&mut text)));
        }
        match n {
            Node::Char(_) => unreachable!(),
            Node::Break => out.push(Piece::LineBreak),
            Node::Chip(c) => out.push(c.data.clone()),
            Node::Box(b) => {
                let items = pieces_of(&b.children);
                if !items.is_empty() {
                    out.push(Piece::Group(items));
                }
            }
        }
    }
    if !text.is_empty() {
        out.push(Piece::Text(text));
    }
    out
}

/// The template of a field.
pub fn template_of(field: &Field<Piece>) -> Template {
    Template::new(pieces_of(&field.nodes))
}

/// The tag of a group: the fields that it needs.
pub fn group_label(children: &[Node<Piece>]) -> String {
    let names: Vec<&str> = (children.iter())
        .filter_map(|n| match n {
            Node::Chip(c) => match &c.data {
                Piece::Field { name, .. } => Some(name.as_str()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    if names.is_empty() {
        String::new()
    } else {
        format!("If {}", names.join(", "))
    }
}

/// Gives each group the tag of its fields.
pub fn relabel(field: &mut Field<Piece>) {
    field.each_box(&mut |b| {
        if b.kind == BoxKind::Group {
            b.label = group_label(&b.children);
        }
    });
}

/// Puts field `name` at the caret, in place of the selection.
pub fn insert_field(field: &mut Field<Piece>, name: &str, help: &ExprHelp) {
    field.replace(vec![field_node(name, FieldFormat::default(), help)]);
}

/// Puts the selection in a group. With no selection, the group takes the
/// field next to the caret, the text before it back to the start of its
/// line, and the text after it to the end of its line, but no text that
/// touches another chip. Returns false if there is nothing to group.
pub fn group(field: &mut Field<Piece>) -> bool {
    if field.sel.is_empty() {
        let Caret { path, index } = field.sel.head.clone();
        let Some(list) = field.list(&path) else {
            return false;
        };
        let text = |n: &Node<Piece>| matches!(n, Node::Char(_));
        let chip = |n: &Node<Piece>| matches!(n, Node::Chip(_));
        // The nearest chip before the caret, or else after it, with only
        // text between.
        let mut i = index;
        while i > 0 && text(&list[i - 1]) {
            i -= 1;
        }
        let mut j = index;
        while j < list.len() && text(&list[j]) {
            j += 1;
        }
        let k = if i > 0 && chip(&list[i - 1]) {
            i - 1
        } else if j < list.len() && chip(&list[j]) {
            j
        } else {
            return false;
        };
        let mut a = k;
        while a > 0 && text(&list[a - 1]) {
            a -= 1;
        }
        if a > 0 && !matches!(list[a - 1], Node::Break) {
            a = k;
        }
        let mut b = k + 1;
        while b < list.len() && text(&list[b]) {
            b += 1;
        }
        if b < list.len() && !matches!(list[b], Node::Break) {
            b = k + 1;
        }
        field.sel = Selection::range(path, a, b);
    }
    let (_, a, b) = field.sel.span();
    if a == b {
        return false;
    }
    field.wrap(|children| Boxed {
        kind: BoxKind::Group,
        label: group_label(&children),
        children,
        data: None,
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(text: &str) -> Field<Piece> {
        field_of(&Template::from_text(text), &ExprHelp::empty())
    }

    fn text(f: &Field<Piece>) -> String {
        template_of(f).to_text()
    }

    #[test]
    fn token_field_types_literal_text() {
        let mut f = field("name");
        f.type_text(" km");
        assert_eq!(text(&f), "concat(name, ' km')");
        // A word that is a field name stays text.
        let mut f = field("");
        f.type_text("name");
        assert_eq!(text(&f), "'name'");
        // Text in the middle splits the text around it.
        let mut f = field("concat('Pop ', pop)");
        f.sel = Selection::at(Caret::new(Vec::new(), 3));
        f.type_text("'s");
        assert_eq!(text(&f), "concat('Pop''s ', pop)");
    }

    #[test]
    fn backspace_selects_chip_then_deletes() {
        let mut f = field("concat('Pop ', pop)");
        assert!(!f.backspace());
        assert!(matches!(f.selected_node(), Some((_, Node::Chip(_)))));
        assert_eq!(text(&f), "concat('Pop ', pop)");
        assert!(f.backspace());
        assert_eq!(text(&f), "'Pop '");
        assert!(f.backspace());
        assert_eq!(text(&f), "'Pop'");
    }

    #[test]
    fn group_takes_the_field_and_the_text_of_its_line() {
        let mut f = field("concat(name, '\n', 'Pop ', pop, ' · ', elev, ' m')");
        let pop = f
            .nodes
            .iter()
            .position(|n| matches!(n, Node::Chip(c) if c.look.label == "pop"));
        f.sel = Selection::at(Caret::new(Vec::new(), pop.unwrap() + 1));
        assert!(group(&mut f));
        let end = f.nodes.len();
        f.sel = Selection::at(Caret::new(Vec::new(), end));
        assert!(group(&mut f));
        let t = text(&f);
        assert!(t.contains(", 'Pop ' || pop, ' · ', elev || ' m')"), "{t}");
        let Some(Node::Box(b)) = f.nodes.iter().find(|n| n.as_box().is_some()) else {
            panic!("no group");
        };
        assert_eq!(b.label, "If pop");
    }

    #[test]
    fn the_badge_shows_the_format() {
        let format = |text: &str| match &Template::from_text(text).pieces[0] {
            Piece::Field { format, .. } => format.clone(),
            p => panic!("{p:?}"),
        };
        assert_eq!(badge(&format("pop")), None);
        assert_eq!(badge(&format("round(pop)")).as_deref(), Some("0 dp"));
        let thousands = badge(&format("format_number(pop, 0)"));
        assert_eq!(thousands.as_deref(), Some("1,234"));
        let text = badge(&format("coalesce(upper(name), alt)"));
        assert_eq!(text.as_deref(), Some("ABC · or alt"));
    }

    #[test]
    fn the_caret_walks_into_groups() {
        let mut f = field("concat('a', 'b' || x)");
        f.sel = Selection::at(Caret::new(Vec::new(), 0));
        let mut seen = Vec::new();
        for _ in 0..8 {
            f.step(super::super::Dir::Forward, false);
            seen.push(f.sel.head.clone());
        }
        assert!(seen.contains(&Caret::new(vec![1], 1)));
        assert_eq!(seen.last(), Some(&Caret::new(Vec::new(), 2)));
    }
}
