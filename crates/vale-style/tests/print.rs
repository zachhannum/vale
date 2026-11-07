use vale_style::expr::{Expr, FUNCTIONS};

fn parse(text: &str) -> Expr {
    Expr::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
}

/// The canonical text, after a check that it parses back to the same tree.
fn print(text: &str) -> String {
    let e = parse(text);
    let out = e.to_string();
    assert_eq!(parse(&out), e, "{text} printed as {out}");
    out
}

#[test]
fn every_operator_round_trips() {
    for text in [
        "a or b",
        "a and b",
        "not a",
        "a = 1",
        "a != 1",
        "a <> 1",
        "a < 1",
        "a <= 1",
        "a > 1",
        "a >= 1",
        "a is null",
        "a is not null",
        "a in (1, 2)",
        "a not in ('x')",
        "a like 'x%'",
        "a not like 'x%'",
        "a ilike 'x%'",
        "a not ilike 'x%'",
        "a || b",
        "a + b",
        "a - b",
        "a * b",
        "a / b",
        "a % b",
        "-a",
        "+a",
        "@scale",
        "@geometry",
        "null",
        "true",
        "false",
        "1e300",
        "0.000001",
    ] {
        print(text);
    }
}

#[test]
fn every_function_round_trips() {
    for f in FUNCTIONS {
        let args = vec!["x"; f.min_args.max(1)];
        let args = &args[..args.len().min(f.max_args.unwrap_or(usize::MAX))];
        let text = format!("{}({})", f.name.to_uppercase(), args.join(","));
        let expected = format!("{}({})", f.name, args.join(", "));
        assert_eq!(print(&text), expected);
    }
}

#[test]
fn canonical_spacing_and_keywords() {
    assert_eq!(print("A  AND(b)OR NOT c"), "A and b or not c");
    assert_eq!(print("x IS NOT NULL"), "x is not null");
    assert_eq!(print("x NOT IN(1,2)"), "x not in (1, 2)");
    assert_eq!(print("round( x ,2 )*3"), "round(x, 2) * 3");
}

#[test]
fn parentheses_by_precedence() {
    assert_eq!(print("(a + b) * c"), "(a + b) * c");
    assert_eq!(print("a + (b * c)"), "a + b * c");
    assert_eq!(print("(a - b) - c"), "a - b - c");
    assert_eq!(print("a - (b - c)"), "a - (b - c)");
    assert_eq!(print("a or (b and c)"), "a or b and c");
    assert_eq!(print("(a or b) and c"), "(a or b) and c");
    assert_eq!(print("a and (b and c)"), "a and (b and c)");
    assert_eq!(print("not (a = 1)"), "not a = 1");
    assert_eq!(print("not (a or b)"), "not (a or b)");
    assert_eq!(print("(a = 1) = true"), "(a = 1) = true");
    assert_eq!(print("(a || b) = c"), "a || b = c");
    assert_eq!(print("a || (b + 1)"), "a || b + 1");
    assert_eq!(print("(a || b) + 1"), "(a || b) + 1");
    assert_eq!(print("-(a + b)"), "-(a + b)");
    assert_eq!(print("-(-a)"), "--a");
    assert_eq!(print("(a + 1) is null"), "a + 1 is null");
    assert_eq!(print("(a is null) is null"), "(a is null) is null");
    assert_eq!(print("(a or b) in (true)"), "(a or b) in (true)");
    assert_eq!(print("coalesce(a or b, (c))"), "coalesce(a or b, c)");
}

#[test]
fn fields_and_text_are_quoted() {
    assert_eq!(print("\"old name\""), "\"old name\"");
    assert_eq!(print("\"and\""), "\"and\"");
    assert_eq!(print("\"say \"\"hi\"\"\""), "\"say \"\"hi\"\"\"");
    assert_eq!(print("\"Name\""), "Name");
    assert_eq!(print("'it''s'"), "'it''s'");
    assert_eq!(print("'two\nlines'"), "'two\nlines'");
    assert_eq!(print("1.50"), "1.5");
    assert_eq!(print("2.0"), "2");
}
