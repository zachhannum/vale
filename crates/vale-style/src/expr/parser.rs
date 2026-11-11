//! Builds the syntax tree from the tokens.

use std::ops::Range;

use vale_store::{FieldType, GeometryKind, Value};

use super::ParseError;
use super::functions::{self, Function};
use super::lexer::{Tok, Token};

/// The greatest height of the syntax tree. Each parenthesis, function call, prefix operator, and each step of a chain such as `a + b + c` adds one level.
const MAX_DEPTH: usize = 64;

#[derive(Copy, Clone, Debug, PartialEq)]
pub(super) enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Concat,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

/// A variable of the feature or the map, written with a leading `@`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Var {
    Scale,
    Geometry,
    Lat,
    Lon,
    Parts,
    Vertices,
}

impl Var {
    pub const ALL: [Var; 6] = [
        Var::Scale,
        Var::Geometry,
        Var::Lat,
        Var::Lon,
        Var::Parts,
        Var::Vertices,
    ];

    /// The name without the `@`.
    pub fn name(self) -> &'static str {
        match self {
            Var::Scale => "scale",
            Var::Geometry => "geometry",
            Var::Lat => "lat",
            Var::Lon => "lon",
            Var::Parts => "parts",
            Var::Vertices => "vertices",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Var::Scale => "Scale",
            Var::Geometry => "Geometry type",
            Var::Lat => "Latitude",
            Var::Lon => "Longitude",
            Var::Parts => "Parts",
            Var::Vertices => "Vertices",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Var::Scale => "Map scale denominator",
            Var::Geometry => "Geometry type of the feature",
            Var::Lat => "Latitude of the center",
            Var::Lon => "Longitude of the center",
            Var::Parts => "Number of parts",
            Var::Vertices => "Number of vertices",
        }
    }

    /// The type of the value of the variable.
    pub fn field_type(self) -> FieldType {
        match self {
            Var::Scale | Var::Lat | Var::Lon => FieldType::Real,
            Var::Geometry => FieldType::Text,
            Var::Parts | Var::Vertices => FieldType::Integer,
        }
    }

    /// True if the variable has a value for features of this kind.
    pub fn fits(self, kind: GeometryKind) -> bool {
        !(self == Var::Parts && kind == GeometryKind::Point)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Node {
    Lit(Value),
    Field(String),
    Var(Var),
    Not(Box<Node>),
    Neg(Box<Node>),
    Binary(BinOp, Box<Node>, Box<Node>),
    IsNull {
        value: Box<Node>,
        negated: bool,
    },
    In {
        value: Box<Node>,
        list: Vec<Node>,
        negated: bool,
    },
    Like {
        value: Box<Node>,
        pattern: Box<Node>,
        ignore_case: bool,
        negated: bool,
    },
    Between {
        value: Box<Node>,
        low: Box<Node>,
        high: Box<Node>,
        negated: bool,
    },
    Call(&'static Function, Vec<Node>),
    /// An empty place where a value belongs. Only a lenient parse makes it.
    Hole,
}

impl Node {
    /// The child nodes, in the order of the text.
    pub fn children(&self) -> Vec<&Node> {
        match self {
            Node::Lit(_) | Node::Field(_) | Node::Var(_) | Node::Hole => Vec::new(),
            Node::Not(a) | Node::Neg(a) | Node::IsNull { value: a, .. } => vec![a],
            Node::Binary(_, a, b)
            | Node::Like {
                value: a,
                pattern: b,
                ..
            } => vec![a, b],
            Node::Between {
                value, low, high, ..
            } => vec![value, low, high],
            Node::In { value, list, .. } => {
                let mut out = vec![&**value];
                out.extend(list);
                out
            }
            Node::Call(_, args) => args.iter().collect(),
        }
    }
}

/// The span of a node in the source, in chars, and the spans of its children. The span of a node in parentheses leaves out the parentheses. A hole has an empty span at the place where the value belongs.
#[derive(Clone, Debug)]
pub(super) struct Spans {
    pub span: Range<usize>,
    pub kids: Vec<Spans>,
}

impl Spans {
    /// The tree for `node` from its spans in the order that the parser makes the nodes, children before parents.
    fn build(node: &Node, spans: &mut impl Iterator<Item = Range<usize>>) -> Spans {
        let kids = node
            .children()
            .into_iter()
            .map(|n| Spans::build(n, spans))
            .collect();
        Spans {
            span: spans.next().unwrap_or(0..0),
            kids,
        }
    }
}

pub(super) struct Parser<'a> {
    chars: &'a [char],
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
    lenient: bool,
    /// The span of each node, in the order that the nodes are made.
    spans: Vec<Range<usize>>,
}

impl<'a> Parser<'a> {
    pub fn new(chars: &'a [char], tokens: Vec<Token>) -> Self {
        Parser {
            chars,
            tokens,
            pos: 0,
            depth: 0,
            lenient: false,
            spans: Vec::new(),
        }
    }

    /// A parser that reads an empty argument or a missing operand as a hole.
    pub fn lenient(mut self) -> Self {
        self.lenient = true;
        self
    }

    /// Parses the whole text.
    pub fn parse(self) -> Result<Node, ParseError> {
        self.parse_spanned().map(|(node, _)| node)
    }

    /// Parses the whole text, and gives the span of each node.
    pub fn parse_spanned(mut self) -> Result<(Node, Spans), ParseError> {
        let node = if self.peek().tok == Tok::End && !self.lenient {
            return Err(self.error_here("the expression is empty"));
        } else {
            self.expr()?
        };
        if self.peek().tok != Tok::End {
            return Err(self.error_here("expected an operator or the end of the expression"));
        }
        let spans = Spans::build(&node, &mut std::mem::take(&mut self.spans).into_iter());
        Ok((node, spans))
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn next(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if t.tok != Tok::End {
            self.pos += 1;
        }
        t
    }

    /// The start of the next token.
    fn here(&self) -> usize {
        self.peek().start
    }

    /// Records the span of a new node from `start` to the end of the last token or the last hole.
    fn made(&mut self, start: usize, node: Node) -> Node {
        let token_end = if self.pos == 0 {
            start
        } else {
            self.tokens[self.pos - 1].end
        };
        let hole_end = self.spans.last().map_or(start, |s| s.end);
        let end = token_end.max(hole_end).max(start);
        self.spans.push(start..end);
        node
    }

    fn hole(&mut self, at: usize) -> Node {
        self.spans.push(at..at);
        Node::Hole
    }

    fn error(&self, start: usize, end: usize, msg: impl Into<String>) -> ParseError {
        ParseError::new(self.chars, start, end, msg)
    }

    fn error_here(&self, msg: &str) -> ParseError {
        let t = self.peek();
        let msg = if t.tok == Tok::End {
            msg.to_string()
        } else {
            let text: String = self.chars[t.start..t.end].iter().collect();
            format!("{msg}, found '{text}'")
        };
        self.error(t.start, t.end, msg)
    }

    /// True if the next token is this keyword.
    fn at_keyword(&self, kw: &str) -> bool {
        self.keyword_at(self.pos, kw)
    }

    fn keyword_at(&self, pos: usize, kw: &str) -> bool {
        matches!(&self.tokens[pos].tok, Tok::Name(n) if n.eq_ignore_ascii_case(kw))
    }

    fn eat_keyword(&mut self, kw: &str) -> bool {
        let yes = self.at_keyword(kw);
        if yes {
            self.pos += 1;
        }
        yes
    }

    fn at_op(&self, op: &str) -> bool {
        matches!(self.peek().tok, Tok::Op(o) if o == op)
    }

    fn enter(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.error_here("the expression is nested too deeply"));
        }
        Ok(())
    }

    fn expr(&mut self) -> Result<Node, ParseError> {
        self.enter()?;
        let node = self.or();
        self.depth -= 1;
        node
    }

    fn or(&mut self) -> Result<Node, ParseError> {
        let base = self.depth;
        let start = self.here();
        let mut left = self.and()?;
        while self.eat_keyword("or") {
            self.enter()?;
            let right = self.and()?;
            left = self.made(
                start,
                Node::Binary(BinOp::Or, Box::new(left), Box::new(right)),
            );
        }
        self.depth = base;
        Ok(left)
    }

    fn and(&mut self) -> Result<Node, ParseError> {
        let base = self.depth;
        let start = self.here();
        let mut left = self.not()?;
        while self.eat_keyword("and") {
            self.enter()?;
            let right = self.not()?;
            left = self.made(
                start,
                Node::Binary(BinOp::And, Box::new(left), Box::new(right)),
            );
        }
        self.depth = base;
        Ok(left)
    }

    fn not(&mut self) -> Result<Node, ParseError> {
        let start = self.here();
        if self.eat_keyword("not") {
            self.enter()?;
            let inner = self.not();
            self.depth -= 1;
            let node = Node::Not(Box::new(inner?));
            return Ok(self.made(start, node));
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Result<Node, ParseError> {
        let start = self.here();
        let left = self.concat()?;
        let op = match &self.peek().tok {
            Tok::Op("=") => Some(BinOp::Eq),
            Tok::Op("!=" | "<>") => Some(BinOp::Ne),
            Tok::Op("<") => Some(BinOp::Lt),
            Tok::Op("<=") => Some(BinOp::Le),
            Tok::Op(">") => Some(BinOp::Gt),
            Tok::Op(">=") => Some(BinOp::Ge),
            _ => None,
        };
        if let Some(op) = op {
            self.next();
            let right = self.concat()?;
            return Ok(self.made(start, Node::Binary(op, Box::new(left), Box::new(right))));
        }
        if self.eat_keyword("is") {
            let negated = self.eat_keyword("not");
            if !self.eat_keyword("null") {
                return Err(self.error_here("expected 'null' after 'is'"));
            }
            let node = Node::IsNull {
                value: Box::new(left),
                negated,
            };
            return Ok(self.made(start, node));
        }
        let negated = self.at_keyword("not")
            && ["in", "like", "ilike", "between"]
                .iter()
                .any(|kw| self.keyword_at(self.pos + 1, kw));
        if negated {
            self.pos += 1;
        }
        if self.eat_keyword("in") {
            let list = self.list()?;
            let node = Node::In {
                value: Box::new(left),
                list,
                negated,
            };
            return Ok(self.made(start, node));
        }
        if self.eat_keyword("between") {
            let low = self.concat()?;
            if !self.eat_keyword("and") {
                return Err(self.error_here("expected 'and' after the low value of 'between'"));
            }
            let high = self.concat()?;
            let node = Node::Between {
                value: Box::new(left),
                low: Box::new(low),
                high: Box::new(high),
                negated,
            };
            return Ok(self.made(start, node));
        }
        for (kw, ignore_case) in [("like", false), ("ilike", true)] {
            if self.eat_keyword(kw) {
                let pattern = self.concat()?;
                let node = Node::Like {
                    value: Box::new(left),
                    pattern: Box::new(pattern),
                    ignore_case,
                    negated,
                };
                return Ok(self.made(start, node));
            }
        }
        Ok(left)
    }

    /// A list in parentheses, for `in`.
    fn list(&mut self) -> Result<Vec<Node>, ParseError> {
        if self.peek().tok != Tok::LParen {
            return Err(self.error_here("expected '(' after 'in'"));
        }
        self.next();
        let mut items = self.args()?;
        if items.is_empty() {
            if !self.lenient {
                return Err(self.error_here("the list is empty"));
            }
            let at = self.tokens[self.pos - 1].start;
            items.push(self.hole(at));
        }
        Ok(items)
    }

    /// Comma-separated expressions up to and with the closing parenthesis.
    fn args(&mut self) -> Result<Vec<Node>, ParseError> {
        let mut items = Vec::new();
        if self.peek().tok == Tok::RParen {
            self.next();
            return Ok(items);
        }
        loop {
            items.push(self.expr()?);
            match self.peek().tok {
                Tok::Comma => {
                    self.next();
                }
                Tok::RParen => {
                    self.next();
                    return Ok(items);
                }
                _ => return Err(self.error_here("expected ',' or ')'")),
            }
        }
    }

    fn concat(&mut self) -> Result<Node, ParseError> {
        let base = self.depth;
        let start = self.here();
        let mut left = self.sum()?;
        while self.at_op("||") {
            self.next();
            self.enter()?;
            let right = self.sum()?;
            left = self.made(
                start,
                Node::Binary(BinOp::Concat, Box::new(left), Box::new(right)),
            );
        }
        self.depth = base;
        Ok(left)
    }

    fn sum(&mut self) -> Result<Node, ParseError> {
        let base = self.depth;
        let start = self.here();
        let mut left = self.term()?;
        loop {
            let op = match self.peek().tok {
                Tok::Op("+") => BinOp::Add,
                Tok::Op("-") => BinOp::Sub,
                _ => {
                    self.depth = base;
                    return Ok(left);
                }
            };
            self.next();
            self.enter()?;
            let right = self.term()?;
            left = self.made(start, Node::Binary(op, Box::new(left), Box::new(right)));
        }
    }

    fn term(&mut self) -> Result<Node, ParseError> {
        let base = self.depth;
        let start = self.here();
        let mut left = self.unary()?;
        loop {
            let op = match self.peek().tok {
                Tok::Op("*") => BinOp::Mul,
                Tok::Op("/") => BinOp::Div,
                Tok::Op("%") => BinOp::Rem,
                _ => {
                    self.depth = base;
                    return Ok(left);
                }
            };
            self.next();
            self.enter()?;
            let right = self.unary()?;
            left = self.made(start, Node::Binary(op, Box::new(left), Box::new(right)));
        }
    }

    fn unary(&mut self) -> Result<Node, ParseError> {
        if self.at_op("-") || self.at_op("+") {
            let start = self.here();
            let neg = self.at_op("-");
            self.next();
            self.enter()?;
            let inner = self.unary();
            self.depth -= 1;
            let inner = inner?;
            return Ok(if neg {
                self.made(start, Node::Neg(Box::new(inner)))
            } else {
                inner
            });
        }
        self.primary()
    }

    /// True if the next token cannot start a value but can follow one, so that a lenient parse reads a hole before it.
    fn at_hole(&self) -> bool {
        match &self.peek().tok {
            Tok::End | Tok::Comma | Tok::RParen | Tok::Op(_) => true,
            Tok::Name(_) => {
                ["and", "or", "is", "in", "like", "ilike", "between"]
                    .iter()
                    .any(|kw| self.at_keyword(kw))
                    && self.tokens[self.pos + 1].tok != Tok::LParen
            }
            _ => false,
        }
    }

    fn primary(&mut self) -> Result<Node, ParseError> {
        if self.lenient && self.at_hole() {
            let at = self.here();
            return Ok(self.hole(at));
        }
        let t = self.peek().clone();
        let node = match t.tok {
            Tok::Number(n) => {
                self.next();
                Node::Lit(Value::Number(n))
            }
            Tok::Str(s) => {
                self.next();
                Node::Lit(Value::Text(s))
            }
            Tok::Quoted(name) => {
                self.next();
                Node::Field(name)
            }
            Tok::Var(name) => {
                self.next();
                match Var::ALL
                    .into_iter()
                    .find(|v| v.name().eq_ignore_ascii_case(&name))
                {
                    Some(var) => Node::Var(var),
                    None => {
                        let names: Vec<String> =
                            Var::ALL.iter().map(|v| format!("@{}", v.name())).collect();
                        return Err(self.error(
                            t.start,
                            t.end,
                            format!(
                                "unknown variable '@{name}'. The variables are {}",
                                names.join(", ")
                            ),
                        ));
                    }
                }
            }
            Tok::LParen => {
                self.next();
                let inner = self.expr()?;
                if self.peek().tok != Tok::RParen {
                    return Err(self.error_here("expected ')'"));
                }
                self.next();
                return Ok(inner);
            }
            Tok::Name(name) => {
                self.next();
                if self.peek().tok == Tok::LParen {
                    return self.call(&name, t.start, t.end);
                }
                match name.to_ascii_lowercase().as_str() {
                    "true" => Node::Lit(Value::Bool(true)),
                    "false" => Node::Lit(Value::Bool(false)),
                    "null" => Node::Lit(Value::Null),
                    kw @ ("and" | "or" | "not" | "is" | "in" | "like" | "ilike" | "between") => {
                        return Err(self.error(
                            t.start,
                            t.end,
                            format!(
                                "'{kw}' is a keyword. Put a field with this name in double quotes"
                            ),
                        ));
                    }
                    _ => Node::Field(name),
                }
            }
            Tok::End => return Err(self.error_here("the expression ends too early")),
            _ => return Err(self.error_here("expected a value")),
        };
        Ok(self.made(t.start, node))
    }

    fn call(&mut self, name: &str, start: usize, end: usize) -> Result<Node, ParseError> {
        let Some(f) = functions::find(name) else {
            return Err(self.error(start, end, format!("unknown function '{name}'")));
        };
        self.next();
        let mut args = self.args()?;
        if self.lenient {
            let at = self.tokens[self.pos - 1].start;
            while args.len() < f.min_args {
                args.push(self.hole(at));
            }
        }
        let n = args.len();
        if n < f.min_args || f.max_args.is_some_and(|max| n > max) {
            let want = match f.max_args {
                Some(max) if max == f.min_args => format!("{max}"),
                Some(max) => format!("{} to {max}", f.min_args),
                None => format!("at least {}", f.min_args),
            };
            let s = if want == "1" { "" } else { "s" };
            return Err(self.error(
                start,
                end,
                format!("'{}' takes {want} argument{s}, not {n}", f.name),
            ));
        }
        Ok(self.made(start, Node::Call(f, args)))
    }
}
