//! The expression language of filters, style properties, and label text.
//!
//! `docs/EXPRESSIONS.md` has the grammar and the functions.

mod eval;
mod functions;
mod lexer;
mod parser;
mod parts;
mod print;

use std::collections::BTreeMap;
use std::fmt;

use vale_render::Color;
use vale_store::GeometryKind;
pub use vale_store::Value;

pub use functions::{Category, FUNCTIONS, Function};
use parser::{Node, Spans};
pub use parts::{TextPart, join, split};

/// The words of the syntax that a plain field name cannot be.
const KEYWORDS: [&str; 16] = [
    "and", "or", "not", "in", "is", "null", "true", "false", "like", "ilike", "between", "case",
    "when", "then", "else", "end",
];

fn is_keyword(name: &str) -> bool {
    KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(name))
}

/// A field name as an expression writes it: plain, or in double quotes.
pub fn field_ref(name: &str) -> String {
    let mut chars = name.chars();
    let plain = chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
        && !is_keyword(name);
    if plain {
        name.to_owned()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

/// The text of an expression with each reference to the field `old` replaced by `new`. Text that does not lex stays as it is.
pub fn rename_field(text: &str, old: &str, new: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let Ok(tokens) = lexer::lex(&chars) else {
        return text.to_owned();
    };
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    for (i, t) in tokens.iter().enumerate() {
        let hit = match &t.tok {
            lexer::Tok::Quoted(name) => name == old,
            lexer::Tok::Name(name) => {
                name == old
                    && !is_keyword(name)
                    && tokens.get(i + 1).map(|n| &n.tok) != Some(&lexer::Tok::LParen)
            }
            _ => false,
        };
        if hit {
            out.extend(&chars[copied..t.start]);
            out.push_str(&field_ref(new));
            copied = t.end;
        }
    }
    out.extend(&chars[copied..]);
    out
}

/// The attributes of one feature, by field name.
pub trait Attributes {
    fn get(&self, field: &str) -> Option<&Value>;
}

impl Attributes for BTreeMap<String, Value> {
    fn get(&self, field: &str) -> Option<&Value> {
        BTreeMap::get(self, field)
    }
}

/// What an expression can read.
#[derive(Copy, Clone)]
pub struct Context<'a> {
    pub attributes: &'a dyn Attributes,
    /// The map scale denominator. On the globe and the flat view, it is the scale at the center of the view.
    pub scale: f64,
    pub geometry: GeometryKind,
}

/// An error in the text of an expression.
#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    /// The span of the error, in chars.
    pub start: usize,
    pub end: usize,
    /// From 1.
    pub line: usize,
    /// From 1, in chars.
    pub column: usize,
    pub message: String,
}

impl ParseError {
    fn new(chars: &[char], start: usize, end: usize, message: impl Into<String>) -> Self {
        let before = &chars[..start.min(chars.len())];
        let line = before.iter().filter(|c| **c == '\n').count() + 1;
        let column = before.iter().rev().take_while(|c| **c != '\n').count() + 1;
        ParseError {
            start,
            end: end.max(start),
            line,
            column,
            message: message.into(),
        }
    }

    /// The line of `source` with the error, and a line of carets under the span.
    pub fn pointer(&self, source: &str) -> String {
        let text = source.lines().nth(self.line - 1).unwrap_or("");
        let width = (self.end - self.start).max(1);
        format!(
            "{text}\n{}{}",
            " ".repeat(self.column - 1),
            "^".repeat(width)
        )
    }

    /// The message as a sentence for the UI, without the position.
    pub fn short(&self) -> String {
        let mut chars = self.message.chars();
        let mut text: String = chars
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        text.extend(chars);
        if !text.ends_with('.') {
            text.push('.');
        }
        text
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for ParseError {}

/// A parsed expression. Two expressions are equal when their syntax trees are equal. Display writes the canonical text.
#[derive(Clone, PartialEq)]
pub struct Expr {
    root: Node,
}

impl fmt::Debug for Expr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Expr({self})")
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&print::print(&self.root))
    }
}

impl Expr {
    pub fn parse(source: &str) -> Result<Expr, ParseError> {
        let chars: Vec<char> = source.chars().collect();
        let tokens = lexer::lex(&chars)?;
        let root = parser::Parser::new(&chars, tokens).parse()?;
        Ok(Expr { root })
    }

    /// Parses text that can have empty places, for an editor. An empty text, an empty argument, a missing argument, and a missing operand are holes. A hole writes as nothing and evaluates to null.
    pub fn parse_lenient(source: &str) -> Result<Expr, ParseError> {
        parse_spanned(source, true).map(|(root, _)| Expr { root })
    }

    pub fn eval(&self, ctx: &Context) -> Value {
        eval::eval(&self.root, ctx)
    }

    /// The names of the fields that the expression reads, sorted and without repeats.
    pub fn fields(&self) -> Vec<String> {
        let mut out = Vec::new();
        collect_fields(&self.root, &mut out);
        out.sort();
        out.dedup();
        out
    }
}

/// The tree of the text and the span of each node.
fn parse_spanned(source: &str, lenient: bool) -> Result<(Node, Spans), ParseError> {
    let chars: Vec<char> = source.chars().collect();
    let tokens = lexer::lex(&chars)?;
    let parser = parser::Parser::new(&chars, tokens);
    if lenient {
        parser.lenient().parse_spanned()
    } else {
        parser.parse_spanned()
    }
}

fn collect_fields(node: &Node, out: &mut Vec<String>) {
    match node {
        Node::Field(name) => out.push(name.clone()),
        Node::Lit(_) | Node::Var(_) | Node::Hole => {}
        Node::Not(a) | Node::Neg(a) | Node::IsNull { value: a, .. } => collect_fields(a, out),
        Node::Binary(_, a, b)
        | Node::Like {
            value: a,
            pattern: b,
            ..
        } => {
            collect_fields(a, out);
            collect_fields(b, out);
        }
        Node::In { value, list, .. } => {
            collect_fields(value, out);
            list.iter().for_each(|n| collect_fields(n, out));
        }
        Node::Between {
            value, low, high, ..
        } => {
            collect_fields(value, out);
            collect_fields(low, out);
            collect_fields(high, out);
        }
        Node::Call(_, args) => args.iter().for_each(|n| collect_fields(n, out)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text of each span of the tree, children before parents.
    fn span_texts(source: &str) -> Vec<String> {
        fn walk(s: &Spans, chars: &[char], out: &mut Vec<String>) {
            s.kids.iter().for_each(|k| walk(k, chars, out));
            out.push(chars[s.span.clone()].iter().collect());
        }
        let (_, spans) = parse_spanned(source, true).unwrap();
        let chars: Vec<char> = source.chars().collect();
        let mut out = Vec::new();
        walk(&spans, &chars, &mut out);
        out
    }

    #[test]
    fn spans_cover_each_node() {
        assert_eq!(
            span_texts("round(pop / 1000, 1) * 2"),
            [
                "pop",
                "1000",
                "pop / 1000",
                "1",
                "round(pop / 1000, 1)",
                "2",
                "round(pop / 1000, 1) * 2"
            ]
        );
        assert_eq!(
            span_texts("not (a) or x between -1 and 'é'"),
            [
                "a",
                "not (a)",
                "x",
                "1",
                "-1",
                "'é'",
                "x between -1 and 'é'",
                "not (a) or x between -1 and 'é'"
            ]
        );
        assert_eq!(
            span_texts("k in (1, 2) and n is null"),
            [
                "k",
                "1",
                "2",
                "k in (1, 2)",
                "n",
                "n is null",
                "k in (1, 2) and n is null"
            ]
        );
        assert_eq!(span_texts("if(, 1, 2)"), ["", "1", "2", "if(, 1, 2)"]);
        assert_eq!(span_texts("a + "), ["a", "", "a + "]);
    }

    #[test]
    fn holes_have_empty_spans_at_their_place() {
        let (_, spans) = parse_spanned("if( , 12, 10)", true).unwrap();
        assert_eq!(spans.kids[0].span, 4..4);
        let (_, spans) = parse_spanned("round()", true).unwrap();
        assert_eq!(spans.kids[0].span, 6..6);
    }
}

/// The text of an expression and the result of its parse. A wrong text stays, so that you can edit it.
#[derive(Clone, Debug)]
pub struct Source {
    text: String,
    parsed: Result<Expr, ParseError>,
}

/// Two sources are equal when their texts are equal.
impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}

impl Source {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let parsed = Expr::parse(&text);
        Source { text, parsed }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn expr(&self) -> Option<&Expr> {
        self.parsed.as_ref().ok()
    }

    pub fn error(&self) -> Option<&ParseError> {
        self.parsed.as_ref().err()
    }

    pub fn rename_field(&mut self, old: &str, new: &str) {
        let text = rename_field(&self.text, old, new);
        if text != self.text {
            *self = Source::new(text);
        }
    }
}

/// A filter of a rule. A blank filter matches each feature, and a wrong filter matches none.
#[derive(Clone, Debug, PartialEq)]
pub struct Filter {
    source: Option<Source>,
}

impl Filter {
    pub fn new(text: &str) -> Self {
        let source = (!text.trim().is_empty()).then(|| Source::new(text));
        Filter { source }
    }

    /// The filter that matches each feature.
    pub fn all() -> Self {
        Filter { source: None }
    }

    pub fn source(&self) -> Option<&Source> {
        self.source.as_ref()
    }

    /// True for the filter that matches each feature.
    pub fn is_all(&self) -> bool {
        self.source.is_none()
    }

    pub fn error(&self) -> Option<&ParseError> {
        self.source.as_ref().and_then(Source::error)
    }

    pub fn rename_field(&mut self, old: &str, new: &str) {
        if let Some(s) = &mut self.source {
            s.rename_field(old, new);
        }
    }

    /// True if the result is `true`. Null and all other values do not match.
    pub fn matches(&self, ctx: &Context) -> bool {
        match &self.source {
            None => true,
            Some(s) => s.expr().is_some_and(|e| functions::truthy(&e.eval(ctx))),
        }
    }
}

/// A type that a property can have.
pub trait FromValue: Sized {
    fn from_value(v: &Value) -> Option<Self>;
}

impl FromValue for f64 {
    fn from_value(v: &Value) -> Option<Self> {
        v.as_f64()
    }
}

impl FromValue for bool {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

impl FromValue for String {
    fn from_value(v: &Value) -> Option<Self> {
        functions::text_of(v)
    }
}

/// A color reads hex text, for example `'#2f6f9f'`.
impl FromValue for Color {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Text(s) => Color::from_hex(s.trim()),
            _ => None,
        }
    }
}

/// A style property: a constant, or an expression with a fallback. The fallback replaces a wrong expression and a result of the wrong type.
#[derive(Clone, Debug, PartialEq)]
pub struct Property<T> {
    pub fallback: T,
    source: Option<Source>,
}

impl<T: FromValue + Clone> Property<T> {
    pub fn constant(value: T) -> Self {
        Property {
            fallback: value,
            source: None,
        }
    }

    pub fn expression(text: &str, fallback: T) -> Self {
        Property {
            fallback,
            source: Some(Source::new(text)),
        }
    }

    pub fn source(&self) -> Option<&Source> {
        self.source.as_ref()
    }

    pub fn is_constant(&self) -> bool {
        self.source.is_none()
    }

    pub fn error(&self) -> Option<&ParseError> {
        self.source.as_ref().and_then(Source::error)
    }

    pub fn rename_field(&mut self, old: &str, new: &str) {
        if let Some(s) = &mut self.source {
            s.rename_field(old, new);
        }
    }

    pub fn eval(&self, ctx: &Context) -> T {
        self.source
            .as_ref()
            .and_then(Source::expr)
            .and_then(|e| T::from_value(&e.eval(ctx)))
            .unwrap_or_else(|| self.fallback.clone())
    }
}
