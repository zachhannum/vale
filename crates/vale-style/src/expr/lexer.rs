//! Splits the text of an expression into tokens.

use super::ParseError;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Tok {
    Number(f64),
    Str(String),
    /// A bare name. Keywords are names too.
    Name(String),
    /// A field name in double quotes.
    Quoted(String),
    /// A name after `@`.
    Var(String),
    LParen,
    RParen,
    Comma,
    Op(&'static str),
    End,
}

/// A token and its span, in chars.
#[derive(Clone, Debug)]
pub(super) struct Token {
    pub tok: Tok,
    pub start: usize,
    pub end: usize,
}

const OPS: [&str; 14] = [
    "<=", ">=", "<>", "!=", "||", "=", "<", ">", "+", "-", "*", "/", "%", "!",
];

pub(super) fn lex(chars: &[char]) -> Result<Vec<Token>, ParseError> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let tok = if c.is_ascii_digit() || (c == '.' && next_is_digit(chars, i)) {
            i = number_end(chars, i);
            let text: String = chars[start..i].iter().collect();
            match text.parse::<f64>() {
                Ok(n) => Tok::Number(n),
                Err(_) => {
                    return Err(ParseError::new(chars, start, i, "this is not a number"));
                }
            }
        } else if c == '\'' || c == '"' {
            let (text, end) = quoted(chars, i, c)?;
            i = end;
            if c == '\'' {
                Tok::Str(text)
            } else {
                Tok::Quoted(text)
            }
        } else if c == '@' {
            i += 1;
            let name_start = i;
            while i < chars.len() && is_name_char(chars[i]) {
                i += 1;
            }
            if i == name_start {
                return Err(ParseError::new(
                    chars,
                    start,
                    i,
                    "a variable name must follow '@'",
                ));
            }
            Tok::Var(chars[name_start..i].iter().collect())
        } else if c.is_alphabetic() || c == '_' {
            while i < chars.len() && is_name_char(chars[i]) {
                i += 1;
            }
            Tok::Name(chars[start..i].iter().collect())
        } else if c == '(' {
            i += 1;
            Tok::LParen
        } else if c == ')' {
            i += 1;
            Tok::RParen
        } else if c == ',' {
            i += 1;
            Tok::Comma
        } else if let Some(op) = OPS.iter().find(|op| starts_with(chars, i, op)) {
            if *op == "!" {
                return Err(ParseError::new(
                    chars,
                    start,
                    i + 1,
                    "unexpected '!'. Use 'not' or '!='",
                ));
            }
            i += op.chars().count();
            Tok::Op(op)
        } else {
            return Err(ParseError::new(
                chars,
                start,
                i + 1,
                format!("unexpected character '{c}'"),
            ));
        };
        out.push(Token { tok, start, end: i });
    }
    out.push(Token {
        tok: Tok::End,
        start: chars.len(),
        end: chars.len(),
    });
    Ok(out)
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn next_is_digit(chars: &[char], i: usize) -> bool {
    chars.get(i + 1).is_some_and(|c| c.is_ascii_digit())
}

fn starts_with(chars: &[char], i: usize, s: &str) -> bool {
    s.chars()
        .enumerate()
        .all(|(k, c)| chars.get(i + k) == Some(&c))
}

fn number_end(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if chars.get(i) == Some(&'.') {
        i += 1;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
    }
    if matches!(chars.get(i), Some('e' | 'E')) {
        let mut j = i + 1;
        if matches!(chars.get(j), Some('+' | '-')) {
            j += 1;
        }
        if chars.get(j).is_some_and(|c| c.is_ascii_digit()) {
            i = j;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
        }
    }
    i
}

/// Reads quoted text from `start`. A doubled quote stands for one quote.
fn quoted(chars: &[char], start: usize, q: char) -> Result<(String, usize), ParseError> {
    let mut text = String::new();
    let mut i = start + 1;
    loop {
        match chars.get(i) {
            None => {
                let what = if q == '\'' { "text" } else { "field name" };
                return Err(ParseError::new(
                    chars,
                    start,
                    chars.len(),
                    format!("the {what} has no closing {q}"),
                ));
            }
            Some(&c) if c == q => {
                if chars.get(i + 1) == Some(&q) {
                    text.push(q);
                    i += 2;
                } else {
                    return Ok((text, i + 1));
                }
            }
            Some(&c) => {
                text.push(c);
                i += 1;
            }
        }
    }
}
