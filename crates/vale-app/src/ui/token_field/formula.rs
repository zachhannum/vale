//! A value as the nodes of a field. A field, a number, a text, and an
//! operator are chips. A function is a block with a slot for each
//! argument, and parentheses are a block with one slot. Typed chars stay
//! chars until a space, an operator, or a pick makes them a token. The
//! text of the field reads the chars as expression text.

use vale_style::expr::editor::{Formula, Op, Seq, Subject, Tok};
use vale_style::expr::{FUNCTIONS, Function, Var, layer_functions};

use super::{BoxKind, Boxed, Caret, Chip, ChipKind, Field, Glyph, Look, Node, Selection, Token};
use crate::classify::number_text;
use crate::ui::expr_help::ExprHelp;

/// The most chars of the label of a code chip.
const MAX_LABEL: usize = 24;

/// The operators of the tray, in order.
pub const OPERATORS: [Op; 15] = [
    Op::Add,
    Op::Sub,
    Op::Mul,
    Op::Div,
    Op::Rem,
    Op::Eq,
    Op::Ne,
    Op::Lt,
    Op::Le,
    Op::Gt,
    Op::Ge,
    Op::And,
    Op::Or,
    Op::Not,
    Op::Concat,
];

/// The label of an operator chip.
pub fn op_label(op: Op) -> &'static str {
    match op {
        Op::Add => "+",
        Op::Sub => "−",
        Op::Mul => "×",
        Op::Div => "÷",
        Op::Rem => "%",
        Op::Eq => "=",
        Op::Ne => "≠",
        Op::Lt => "<",
        Op::Le => "≤",
        Op::Gt => ">",
        Op::Ge => "≥",
        Op::And => "and",
        Op::Or => "or",
        Op::Not => "not",
        Op::Concat => "||",
    }
}

/// The label of a Cases chip.
pub const CASES: &str = "Cases";

fn short(text: &str) -> String {
    if text.chars().count() <= MAX_LABEL {
        return text.to_owned();
    }
    let mut s: String = text.chars().take(MAX_LABEL - 1).collect();
    s.push('…');
    s
}

/// The look of the chip of a token.
pub fn look_of(tok: &Tok, help: &ExprHelp) -> Look {
    let (kind, label, glyph) = match tok {
        Tok::Field(name) => (
            ChipKind::Field,
            name.clone(),
            Some(help.field_type(name).map_or(Glyph::Text, Glyph::of)),
        ),
        Tok::Var(v) => (
            ChipKind::Variable,
            format!("@{}", v.name()),
            Some(Glyph::of(v.field_type())),
        ),
        Tok::Num(n) => (ChipKind::Value, number_text(*n), None),
        Tok::Text(s) => (ChipKind::Value, format!("“{s}”"), None),
        Tok::Bool(b) => (ChipKind::Value, b.to_string(), None),
        Tok::Op(op) => (ChipKind::Operator, op_label(*op).to_owned(), None),
        Tok::Cases { .. } => (ChipKind::Function, CASES.to_owned(), None),
        Tok::Code(s) => (ChipKind::Code, short(s), None),
        Tok::Paren(_) | Tok::Call { .. } => (ChipKind::Function, short(&tok.to_text()), None),
    };
    Look {
        kind,
        glyph,
        label,
        badge: None,
    }
}

/// The chip of a token.
pub fn chip(tok: Tok, help: &ExprHelp) -> Node<Tok> {
    Node::Chip(Chip {
        look: look_of(&tok, help),
        data: tok,
    })
}

/// The count of the slots of a new block of `f`: each named argument, and
/// two for a function with no limit.
pub fn slot_count(f: &Function) -> usize {
    match f.max_args {
        Some(n) => n.max(f.min_args),
        None => f.min_args.max(2),
    }
}

/// A slot with these nodes. `label` is the name of its argument.
fn slot(label: &str, children: Vec<Node<Tok>>) -> Node<Tok> {
    Node::Box(Boxed {
        kind: BoxKind::Slot,
        label: label.to_owned(),
        children,
        data: None,
    })
}

/// The block of `f` with these slots.
pub fn block(f: &'static Function, slots: Vec<Vec<Node<Tok>>>) -> Node<Tok> {
    let children = (slots.into_iter().enumerate())
        .map(|(i, nodes)| slot(f.slot(i), nodes))
        .collect();
    Node::Box(Boxed {
        kind: BoxKind::Block,
        label: f.name.to_owned(),
        children,
        data: Some(Tok::Call {
            f,
            args: Vec::new(),
        }),
    })
}

/// The block of parentheses around these nodes.
pub fn paren(nodes: Vec<Node<Tok>>) -> Node<Tok> {
    Node::Box(Boxed {
        kind: BoxKind::Block,
        label: String::new(),
        children: vec![slot("", nodes)],
        data: Some(Tok::Paren(Seq::default())),
    })
}

/// The nodes of a sequence.
pub fn nodes_of(seq: &Seq, help: &ExprHelp) -> Vec<Node<Tok>> {
    (seq.0.iter())
        .map(|tok| match tok {
            Tok::Call { f, args } => {
                let mut slots: Vec<Vec<Node<Tok>>> =
                    args.iter().map(|a| nodes_of(a, help)).collect();
                while slots.len() < slot_count(f) {
                    slots.push(Vec::new());
                }
                block(f, slots)
            }
            Tok::Paren(inner) => paren(nodes_of(inner, help)),
            tok => chip(tok.clone(), help),
        })
        .collect()
}

/// The field of a value, with the caret at the end.
pub fn field_of(formula: &Formula, help: &ExprHelp) -> Field<Tok> {
    Field::new(nodes_of(&formula.seq, help))
}

/// The expression text of a list of nodes. A run of chars is text as the
/// user typed it. An empty slot after the arguments that a function needs
/// writes nothing.
pub fn text_of(nodes: &[Node<Tok>]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut run = String::new();
    let flush = |run: &mut String, parts: &mut Vec<String>| {
        let t = run.trim();
        if !t.is_empty() {
            parts.push(t.to_owned());
        }
        run.clear();
    };
    for n in nodes {
        match n {
            Node::Char(c) => run.push(*c),
            Node::Break => {}
            Node::Chip(c) => {
                flush(&mut run, &mut parts);
                parts.push(c.data.to_text());
            }
            Node::Box(b) => {
                flush(&mut run, &mut parts);
                parts.push(box_text(b));
            }
        }
    }
    flush(&mut run, &mut parts);
    parts.join(" ")
}

impl Token for Tok {
    fn text(nodes: &[Node<Tok>]) -> String {
        text_of(nodes)
    }
}

/// Puts `text` in place of the selection, read as a value. Text that does
/// not parse becomes a code chip.
pub fn paste(field: &mut Field<Tok>, text: &str, help: &ExprHelp) {
    if field.sel.is_empty() {
        commit(field, help);
    }
    field.replace(nodes_of(&Formula::from_text(text.trim()).seq, help));
}

fn box_text(b: &Boxed<Tok>) -> String {
    let slots: Vec<String> = (b.children.iter())
        .map(|n| match n {
            Node::Box(s) => text_of(&s.children),
            n => text_of(std::slice::from_ref(n)),
        })
        .collect();
    match &b.data {
        Some(Tok::Call { f, .. }) => {
            let mut args = slots;
            while args.len() > f.min_args && args.last().is_some_and(String::is_empty) {
                args.pop();
            }
            format!("{}({})", f.name, args.join(", "))
        }
        _ => format!("({})", slots.join(", ")),
    }
}

/// The run of chars before the caret: the list, and the range of the run.
fn run_range(field: &Field<Tok>) -> (Vec<usize>, usize, usize) {
    let Caret { path, index } = field.sel.head.clone();
    let list = field.list(&path).unwrap_or(&[]);
    let mut a = index.min(list.len());
    while a > 0 && matches!(list[a - 1], Node::Char(_)) {
        a -= 1;
    }
    (path, a, index)
}

/// The text of the run of chars before the caret.
pub fn pending(field: &Field<Tok>) -> String {
    let (path, a, b) = run_range(field);
    let list = field.list(&path).unwrap_or(&[]);
    (list[a..b.min(list.len())].iter())
        .filter_map(|n| match n {
            Node::Char(c) => Some(*c),
            _ => None,
        })
        .collect()
}

/// Removes the run of chars before the caret, and returns its text.
fn take_run(field: &mut Field<Tok>) -> String {
    let text = pending(field);
    let (path, a, b) = run_range(field);
    field.sel = Selection::range(path, a, b);
    field.replace(Vec::new());
    text
}

/// The function named `name`.
pub fn function_named(name: &str) -> Option<&'static Function> {
    let name = name.trim();
    FUNCTIONS.iter().find(|f| f.name.eq_ignore_ascii_case(name))
}

/// The variable that `@name` names, in any case.
fn var_named(word: &str) -> Option<Var> {
    let name = word.trim().strip_prefix('@')?;
    Var::ALL
        .into_iter()
        .find(|v| v.name().eq_ignore_ascii_case(name))
}

/// The nodes of a word: a text in quotes, a number, a yes or no value, a
/// word operator, a variable, a field, or a function. Another word is code.
fn word_nodes(word: &str, help: &ExprHelp) -> Vec<Node<Tok>> {
    let w = word.trim();
    if w.is_empty() {
        return Vec::new();
    }
    if let Some(rest) = w.strip_prefix('\'') {
        let inner = rest.strip_suffix('\'').unwrap_or(rest).replace("''", "'");
        return vec![chip(Tok::Text(inner), help)];
    }
    let numeric = w.starts_with(|c: char| c.is_ascii_digit() || c == '.');
    if let Some(n) = w.parse::<f64>().ok().filter(|n| numeric && n.is_finite()) {
        return vec![chip(Tok::Num(n), help)];
    }
    let lower = w.to_lowercase();
    let tok = match lower.as_str() {
        "true" => Some(Tok::Bool(true)),
        "false" => Some(Tok::Bool(false)),
        "and" => Some(Tok::Op(Op::And)),
        "or" => Some(Tok::Op(Op::Or)),
        "not" => Some(Tok::Op(Op::Not)),
        _ => None,
    };
    if let Some(tok) = tok {
        return vec![chip(tok, help)];
    }
    if let Some(v) = var_named(w) {
        return vec![chip(Tok::Var(v), help)];
    }
    let field = (help.fields.iter().find(|f| *f == w))
        .or_else(|| help.fields.iter().find(|f| f.to_lowercase() == lower));
    if let Some(name) = field {
        return vec![chip(Tok::Field(name.clone()), help)];
    }
    if let Some(f) = function_named(w) {
        return vec![block(f, vec![Vec::new(); slot_count(f)])];
    }
    vec![chip(Tok::Code(w.to_owned()), help)]
}

/// Makes the run of chars before the caret a token. A function becomes a
/// block, and the caret goes in its first slot. Returns true if there was
/// a run.
pub fn commit(field: &mut Field<Tok>, help: &ExprHelp) -> bool {
    let word = pending(field);
    if word.trim().is_empty() {
        if !word.is_empty() {
            take_run(field);
        }
        return false;
    }
    take_run(field);
    let nodes = word_nodes(&word, help);
    let block = matches!(nodes.as_slice(), [Node::Box(_)]);
    field.replace(nodes);
    if block {
        enter_block_before(field);
    }
    true
}

/// Puts the caret in the first slot of the block before the caret.
fn enter_block_before(field: &mut Field<Tok>) {
    let Caret { path, index } = field.sel.head.clone();
    if index == 0 {
        return;
    }
    let mut p = path;
    p.push(index - 1);
    if field.takes_caret(&p) {
        return;
    }
    p.push(0);
    if field.takes_caret(&p) {
        field.sel = Selection::at(Caret::new(p, 0));
    }
}

/// Moves the caret out of its slot, after the block.
pub fn leave_box(field: &mut Field<Tok>) {
    let path = field.sel.head.path.clone();
    if path.len() >= 2 {
        let block = path.len() - 2;
        let at = path[block] + 1;
        field.sel = Selection::at(Caret::new(path[..block].to_vec(), at));
    }
}

/// Moves the caret to the start of the next slot of its block. After the
/// last slot, a function with no limit gets a new slot if `add`, and
/// otherwise the caret leaves the block.
pub fn next_slot_of_block(field: &mut Field<Tok>, add: bool) {
    let path = field.sel.head.path.clone();
    let Some((&j, block)) = path.split_last() else {
        return;
    };
    let mut next = block.to_vec();
    next.push(j + 1);
    if field.takes_caret(&next) {
        field.sel = Selection::at(Caret::new(next, 0));
        return;
    }
    let unlimited = match field.node(block) {
        Some(Node::Box(b)) => matches!(&b.data, Some(Tok::Call { f, .. }) if f.max_args.is_none()),
        _ => false,
    };
    if add
        && unlimited
        && let Some(Node::Box(b)) = field.node_mut(block)
        && let Some(Tok::Call { f, .. }) = &b.data
    {
        let label = f.slot(j + 1);
        b.children.push(slot(label, Vec::new()));
        field.sel = Selection::at(Caret::new(next, 0));
        return;
    }
    leave_box(field);
}

/// Moves the caret to the start of the next slot in the field, in the order
/// of the text, or to the end of the field.
pub fn next_slot(field: &mut Field<Tok>) {
    let stops = field.stops();
    let head = field.sel.head.clone();
    let at = stops.iter().position(|s| *s == head).unwrap_or(0);
    let is_slot = |f: &Field<Tok>, p: &[usize]| matches!(f.node(p), Some(Node::Box(b)) if b.kind == BoxKind::Slot);
    let next = (stops[at + 1..].iter())
        .find(|s| s.index == 0 && s.path != head.path && is_slot(field, &s.path))
        .cloned()
        .unwrap_or_else(|| Caret::new(Vec::new(), field.nodes.len()));
    field.sel = Selection::at(next);
}

fn push_char(field: &mut Field<Tok>, c: char) {
    field.replace(vec![Node::Char(c)]);
}

/// The operator of a char.
fn op_of(c: char) -> Option<Op> {
    Some(match c {
        '+' => Op::Add,
        '-' => Op::Sub,
        '*' => Op::Mul,
        '/' => Op::Div,
        '%' => Op::Rem,
        '=' => Op::Eq,
        '<' => Op::Lt,
        '>' => Op::Gt,
        _ => return None,
    })
}

/// The token before the caret, if it is a chip.
fn tok_before(field: &Field<Tok>) -> Option<&Tok> {
    let Caret { path, index } = &field.sel.head;
    let n = field.list(path)?.get(index.checked_sub(1)?)?;
    match n {
        Node::Chip(c) => Some(&c.data),
        _ => None,
    }
}

/// Puts operator `op` at the caret. `<` and `=` make `≤`, and so on.
pub fn insert_op(field: &mut Field<Tok>, op: Op, help: &ExprHelp) {
    commit(field, help);
    let joined = match (tok_before(field), op) {
        (Some(Tok::Op(Op::Lt)), Op::Eq) => Some(Op::Le),
        (Some(Tok::Op(Op::Gt)), Op::Eq) => Some(Op::Ge),
        (Some(Tok::Op(Op::Lt)), Op::Gt) => Some(Op::Ne),
        (Some(Tok::Op(Op::Eq)), Op::Eq) => Some(Op::Eq),
        _ => None,
    };
    if let Some(op) = joined {
        let Caret { path, index } = field.sel.head.clone();
        field.sel = Selection::range(path, index - 1, index);
        field.replace(vec![chip(Tok::Op(op), help)]);
    } else {
        field.replace(vec![chip(Tok::Op(op), help)]);
    }
}

/// Types `text` at the caret. Digits make a number, `'` starts a text,
/// an operator char makes an operator chip, and a space ends a word. `(`
/// after a function name makes its block, `,` goes to the next slot, and
/// `)` leaves the block.
pub fn type_text(field: &mut Field<Tok>, text: &str, help: &ExprHelp) {
    for c in text.chars().filter(|c| !c.is_control()) {
        type_char(field, c, help);
    }
}

fn type_char(field: &mut Field<Tok>, c: char, help: &ExprHelp) {
    if !field.sel.is_empty() {
        field.replace(Vec::new());
    }
    let run = pending(field);
    let quoted = run.starts_with('\'') && run.matches('\'').count() % 2 == 1;
    if quoted {
        push_char(field, c);
        if c == '\'' {
            commit(field, help);
        }
        return;
    }
    match (run.as_str(), c) {
        ("!", '=') => {
            take_run(field);
            field.replace(vec![chip(Tok::Op(Op::Ne), help)]);
        }
        ("|", '|') => {
            take_run(field);
            field.replace(vec![chip(Tok::Op(Op::Concat), help)]);
        }
        (_, '\'' | '!' | '|') => {
            commit(field, help);
            push_char(field, c);
        }
        (_, c) if c.is_whitespace() => {
            commit(field, help);
        }
        (_, '(') => match function_named(&run) {
            Some(f) if !run.is_empty() => {
                take_run(field);
                insert_function(field, f);
            }
            _ => {
                commit(field, help);
                field.replace(vec![paren(Vec::new())]);
                enter_block_before(field);
            }
        },
        (_, ')') => {
            commit(field, help);
            leave_box(field);
        }
        (_, ',') => {
            commit(field, help);
            next_slot_of_block(field, true);
        }
        (_, c) => match op_of(c) {
            Some(op) => insert_op(field, op, help),
            None => push_char(field, c),
        },
    }
}

/// Puts a block of `f` at the caret. The selection goes in the first slot,
/// and the caret goes in the next empty slot.
pub fn insert_function(field: &mut Field<Tok>, f: &'static Function) {
    let (path, a, _) = field.sel.span();
    let path = path.to_vec();
    let had = !field.sel.is_empty();
    field.wrap(|inner| {
        let mut slots = vec![inner];
        while slots.len() < slot_count(f) {
            slots.push(Vec::new());
        }
        match block(f, slots) {
            Node::Box(b) => b,
            _ => unreachable!(),
        }
    });
    let mut block_path = path.clone();
    block_path.push(a);
    let first = usize::from(had);
    let mut slot_path = block_path.clone();
    slot_path.push(first);
    field.sel = if field.takes_caret(&slot_path) {
        Selection::at(Caret::new(slot_path, 0))
    } else {
        Selection::at(Caret::new(path, a + 1))
    };
}

/// A completion of a typed word.
#[derive(Clone, Debug, PartialEq)]
pub enum Completion {
    Field(String),
    Function(&'static Function),
    Var(Var),
    /// A raster value: a call of `raster`.
    Raster(Subject),
}

/// The fields and the functions whose names start with `word`, fields
/// first. A word that starts with `@` completes the variables. No
/// completions for a word that is not a name.
pub fn completions(word: &str, help: &ExprHelp) -> Vec<Completion> {
    let w = word.trim().to_lowercase();
    if let Some(rest) = w.strip_prefix('@') {
        return (Var::ALL.into_iter())
            .filter(|v| v.name().starts_with(rest))
            .map(Completion::Var)
            .collect();
    }
    let name = w.starts_with(|c: char| c.is_alphabetic() || c == '_');
    if !name {
        return Vec::new();
    }
    let fields = (help.fields.iter())
        .filter(|f| f.to_lowercase().starts_with(&w))
        .map(|f| Completion::Field(f.clone()));
    let functions = layer_functions()
        .filter(|f| f.name.starts_with(&w))
        .map(Completion::Function);
    fields.chain(functions).collect()
}

/// Puts a completion in place of the run of chars before the caret.
pub fn pick(field: &mut Field<Tok>, completion: &Completion, help: &ExprHelp) {
    take_run(field);
    match completion {
        Completion::Field(name) => insert_field(field, name, help),
        Completion::Function(f) => insert_function(field, f),
        Completion::Var(v) => insert_var(field, *v, help),
        Completion::Raster(s) => insert_subject(field, s, help),
    }
}

/// Puts the expression of a subject at the caret, in place of the selection
/// and of the run of chars before the caret.
pub fn insert_subject(field: &mut Field<Tok>, subject: &Subject, help: &ExprHelp) {
    if field.sel.is_empty() {
        take_run(field);
    }
    let seq = Formula::from_text(&subject.to_text()).seq;
    field.replace(nodes_of(&seq, help));
}

/// Puts variable `v` at the caret, in place of the selection and of the run
/// of chars before the caret.
pub fn insert_var(field: &mut Field<Tok>, v: Var, help: &ExprHelp) {
    if field.sel.is_empty() {
        take_run(field);
    }
    field.replace(vec![chip(Tok::Var(v), help)]);
}

/// Puts field `name` at the caret, in place of the selection and of the run
/// of chars before the caret.
pub fn insert_field(field: &mut Field<Tok>, name: &str, help: &ExprHelp) {
    if field.sel.is_empty() {
        take_run(field);
    }
    field.replace(vec![chip(Tok::Field(name.to_owned()), help)]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn help() -> ExprHelp {
        let mut h = ExprHelp::empty();
        h.fields = vec!["pop".into(), "name".into()];
        h.types = vec![vale_store::FieldType::Real, vale_store::FieldType::Text];
        h
    }

    fn typed(text: &str) -> Field<Tok> {
        let mut f = Field::default();
        type_text(&mut f, text, &help());
        f
    }

    #[test]
    fn typing_makes_tokens() {
        let f = typed("pop * 2 + 'a b' ");
        assert_eq!(text_of(&f.nodes), "pop * 2 + 'a b'");
        assert!(f.nodes.iter().all(|n| matches!(n, Node::Chip(_))));
        let f = typed("pop<=3");
        assert_eq!(text_of(&f.nodes), "pop <= 3");
        let f = typed("name || 'x'");
        assert_eq!(text_of(&f.nodes), "name || 'x'");
        // A word that the user still types is text.
        let f = typed("po");
        assert_eq!(pending(&f), "po");
        assert_eq!(text_of(&f.nodes), "po");
    }

    #[test]
    fn typing_at_name_makes_a_variable_chip() {
        let f = typed("@lat + 1");
        assert_eq!(text_of(&f.nodes), "@lat + 1");
        let Node::Chip(c) = &f.nodes[0] else {
            panic!("no chip");
        };
        assert_eq!(c.data, Tok::Var(Var::Lat));
        assert_eq!(c.look.kind, ChipKind::Variable);
        assert_eq!(c.look.label, "@lat");
        // Any case names the variable, and the text writes it in lowercase.
        let f = typed("@VERTICES ");
        assert_eq!(text_of(&f.nodes), "@vertices");
        // An unknown name is code.
        let f = typed("@nope ");
        assert!(matches!(&f.nodes[0], Node::Chip(c) if c.look.kind == ChipKind::Code));
        // A word that starts with `@` completes the variables.
        let mut f = typed("@ve");
        let found = completions(&pending(&f), &help());
        assert_eq!(found, [Completion::Var(Var::Vertices)]);
        pick(&mut f, &found[0], &help());
        assert_eq!(text_of(&f.nodes), "@vertices");
    }

    #[test]
    fn a_function_name_and_a_bracket_make_a_block() {
        let f = typed("round(pop, 1)");
        assert_eq!(text_of(&f.nodes), "round(pop, 1)");
        assert!(matches!(&f.nodes[0], Node::Box(b) if b.kind == BoxKind::Block));
        assert_eq!(f.sel.head, Caret::new(Vec::new(), 1));
        let f = typed("(pop + 1) * 2");
        assert_eq!(text_of(&f.nodes), "(pop + 1) * 2");
    }

    #[test]
    fn empty_slot_shows_type() {
        let mut f = Field::default();
        let clamp = function_named("clamp").unwrap();
        insert_function(&mut f, clamp);
        let Node::Box(b) = &f.nodes[0] else {
            panic!("no block");
        };
        let labels: Vec<&str> = (b.children.iter())
            .filter_map(|n| n.as_box().map(|s| s.label.as_str()))
            .collect();
        assert_eq!(labels, ["low", "x", "high"]);
        assert_eq!(f.sel.head, Caret::new(vec![0, 0], 0));
        assert!(f.plain().contains("high"), "{}", f.plain());
    }

    #[test]
    fn tray_function_wraps_selection() {
        let mut f = field_of(&Formula::from_text("pop / 1000"), &help());
        f.select_all();
        insert_function(&mut f, function_named("round").unwrap());
        assert_eq!(text_of(&f.nodes), "round(pop / 1000)");
        // The caret is in the empty second slot.
        assert_eq!(f.sel.head, Caret::new(vec![0, 1], 0));
        type_text(&mut f, "1", &help());
        assert_eq!(text_of(&f.nodes), "round(pop / 1000, 1)");
    }

    #[test]
    fn tab_moves_to_next_slot() {
        let mut f = Field::default();
        insert_function(&mut f, function_named("clamp").unwrap());
        type_text(&mut f, "4", &help());
        commit(&mut f, &help());
        next_slot(&mut f);
        assert_eq!(f.sel.head, Caret::new(vec![0, 1], 0));
        type_text(&mut f, "pop", &help());
        commit(&mut f, &help());
        next_slot(&mut f);
        type_text(&mut f, "9", &help());
        next_slot(&mut f);
        assert_eq!(f.sel.head, Caret::new(Vec::new(), 1));
        assert_eq!(text_of(&f.nodes), "clamp(4, pop, 9)");
    }

    #[test]
    fn completion_picks_a_field_or_a_function() {
        let mut f = typed("po");
        let found = completions(&pending(&f), &help());
        assert_eq!(found[0], Completion::Field("pop".into()));
        pick(&mut f, &found[0], &help());
        assert_eq!(text_of(&f.nodes), "pop");
        let mut f = typed("rou");
        let found = completions(&pending(&f), &help());
        pick(&mut f, &found[0], &help());
        assert_eq!(f.sel.head, Caret::new(vec![0, 0], 0));
    }

    #[test]
    fn paste_text_in_value_slot_makes_tokens() {
        let mut f = Field::default();
        insert_function(&mut f, function_named("round").unwrap());
        assert_eq!(f.sel.head, Caret::new(vec![0, 0], 0));
        paste(&mut f, "pop * 2", &help());
        assert_eq!(text_of(&f.nodes), "round(pop * 2)");
        let Node::Box(b) = &f.nodes[0] else {
            panic!("no block");
        };
        let slot = b.children[0].as_box().unwrap();
        assert!(slot.children.iter().all(|n| matches!(n, Node::Chip(_))));
        // Text that does not parse is a code chip.
        let mut f = Field::default();
        paste(&mut f, "pop +* )", &help());
        assert!(
            f.nodes
                .iter()
                .any(|n| matches!(n, Node::Chip(c) if c.look.kind == ChipKind::Code))
        );
    }

    #[test]
    fn a_chip_moves_into_a_slot() {
        let mut f = field_of(&Formula::from_text("pop + round(name)"), &help());
        assert!(f.move_node(&[0], &Caret::new(vec![2, 1], 0)));
        assert_eq!(text_of(&f.nodes), "+ round(name, pop)");
        // A block does not go into its own slot.
        assert!(!f.move_node(&[1], &Caret::new(vec![1, 0], 0)));
    }
}
