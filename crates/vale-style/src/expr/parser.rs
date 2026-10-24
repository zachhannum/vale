//! Builds the syntax tree from the tokens.

use vale_store::Value;

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

#[derive(Copy, Clone, Debug, PartialEq)]
pub(super) enum Var {
    Scale,
    Geometry,
}

#[derive(Clone)]
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
    Call(&'static Function, Vec<Node>),
}

pub(super) struct Parser<'a> {
    chars: &'a [char],
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    pub fn new(chars: &'a [char], tokens: Vec<Token>) -> Self {
        Parser {
            chars,
            tokens,
            pos: 0,
            depth: 0,
        }
    }

    /// Parses the whole text.
    pub fn parse(mut self) -> Result<Node, ParseError> {
        if self.peek().tok == Tok::End {
            return Err(self.error_here("the expression is empty"));
        }
        let node = self.expr()?;
        if self.peek().tok != Tok::End {
            return Err(self.error_here("expected an operator or the end of the expression"));
        }
        Ok(node)
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
        let mut left = self.and()?;
        while self.eat_keyword("or") {
            self.enter()?;
            let right = self.and()?;
            left = Node::Binary(BinOp::Or, Box::new(left), Box::new(right));
        }
        self.depth = base;
        Ok(left)
    }

    fn and(&mut self) -> Result<Node, ParseError> {
        let base = self.depth;
        let mut left = self.not()?;
        while self.eat_keyword("and") {
            self.enter()?;
            let right = self.not()?;
            left = Node::Binary(BinOp::And, Box::new(left), Box::new(right));
        }
        self.depth = base;
        Ok(left)
    }

    fn not(&mut self) -> Result<Node, ParseError> {
        if self.eat_keyword("not") {
            self.enter()?;
            let inner = self.not();
            self.depth -= 1;
            return Ok(Node::Not(Box::new(inner?)));
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Result<Node, ParseError> {
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
            return Ok(Node::Binary(op, Box::new(left), Box::new(right)));
        }
        if self.eat_keyword("is") {
            let negated = self.eat_keyword("not");
            if !self.eat_keyword("null") {
                return Err(self.error_here("expected 'null' after 'is'"));
            }
            return Ok(Node::IsNull {
                value: Box::new(left),
                negated,
            });
        }
        let negated = self.at_keyword("not")
            && ["in", "like", "ilike"]
                .iter()
                .any(|kw| self.keyword_at(self.pos + 1, kw));
        if negated {
            self.pos += 1;
        }
        if self.eat_keyword("in") {
            let list = self.list()?;
            return Ok(Node::In {
                value: Box::new(left),
                list,
                negated,
            });
        }
        for (kw, ignore_case) in [("like", false), ("ilike", true)] {
            if self.eat_keyword(kw) {
                let pattern = self.concat()?;
                return Ok(Node::Like {
                    value: Box::new(left),
                    pattern: Box::new(pattern),
                    ignore_case,
                    negated,
                });
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
        let items = self.args()?;
        if items.is_empty() {
            return Err(self.error_here("the list is empty"));
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
        let mut left = self.sum()?;
        while self.at_op("||") {
            self.next();
            self.enter()?;
            let right = self.sum()?;
            left = Node::Binary(BinOp::Concat, Box::new(left), Box::new(right));
        }
        self.depth = base;
        Ok(left)
    }

    fn sum(&mut self) -> Result<Node, ParseError> {
        let base = self.depth;
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
            left = Node::Binary(op, Box::new(left), Box::new(right));
        }
    }

    fn term(&mut self) -> Result<Node, ParseError> {
        let base = self.depth;
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
            left = Node::Binary(op, Box::new(left), Box::new(right));
        }
    }

    fn unary(&mut self) -> Result<Node, ParseError> {
        if self.at_op("-") || self.at_op("+") {
            let neg = self.at_op("-");
            self.next();
            self.enter()?;
            let inner = self.unary();
            self.depth -= 1;
            let inner = inner?;
            return Ok(if neg {
                Node::Neg(Box::new(inner))
            } else {
                inner
            });
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Node, ParseError> {
        let t = self.peek().clone();
        match t.tok {
            Tok::Number(n) => {
                self.next();
                Ok(Node::Lit(Value::Number(n)))
            }
            Tok::Str(s) => {
                self.next();
                Ok(Node::Lit(Value::Text(s)))
            }
            Tok::Quoted(name) => {
                self.next();
                Ok(Node::Field(name))
            }
            Tok::Var(name) => {
                self.next();
                match name.to_ascii_lowercase().as_str() {
                    "scale" => Ok(Node::Var(Var::Scale)),
                    "geometry" => Ok(Node::Var(Var::Geometry)),
                    _ => Err(self.error(
                        t.start,
                        t.end,
                        format!(
                            "unknown variable '@{name}'. The variables are @scale and @geometry"
                        ),
                    )),
                }
            }
            Tok::LParen => {
                self.next();
                let inner = self.expr()?;
                if self.peek().tok != Tok::RParen {
                    return Err(self.error_here("expected ')'"));
                }
                self.next();
                Ok(inner)
            }
            Tok::Name(name) => {
                self.next();
                if self.peek().tok == Tok::LParen {
                    return self.call(&name, t.start, t.end);
                }
                match name.to_ascii_lowercase().as_str() {
                    "true" => Ok(Node::Lit(Value::Bool(true))),
                    "false" => Ok(Node::Lit(Value::Bool(false))),
                    "null" => Ok(Node::Lit(Value::Null)),
                    kw @ ("and" | "or" | "not" | "is" | "in" | "like" | "ilike") => Err(self
                        .error(
                            t.start,
                            t.end,
                            format!(
                                "'{kw}' is a keyword. Put a field with this name in double quotes"
                            ),
                        )),
                    _ => Ok(Node::Field(name)),
                }
            }
            Tok::End => Err(self.error_here("the expression ends too early")),
            _ => Err(self.error_here("expected a value")),
        }
    }

    fn call(&mut self, name: &str, start: usize, end: usize) -> Result<Node, ParseError> {
        let Some(f) = functions::find(name) else {
            return Err(self.error(start, end, format!("unknown function '{name}'")));
        };
        self.next();
        let args = self.args()?;
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
        Ok(Node::Call(f, args))
    }
}
