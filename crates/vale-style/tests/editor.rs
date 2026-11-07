use vale_store::FieldType;
use vale_style::expr::Value;
use vale_style::expr::editor::{CmpOp, Cond, Conditions, Formula, Op, Seq, Tok, ops_for};
use vale_style::expr::{Expr, FUNCTIONS};

fn tree(text: &str) -> Expr {
    Expr::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
}

/// Checks the round trip of a text through a model: the written text has the same tree, and a second trip writes the same text.
fn round_trip(text: &str, to_text: impl Fn(&str) -> String) -> String {
    let once = to_text(text);
    assert_eq!(tree(&once), tree(text), "{text} wrote {once}");
    assert_eq!(to_text(&once), once, "{text}: second trip");
    once
}

fn rule(field: &str, op: CmpOp, values: Vec<Value>) -> Cond {
    Cond::Rule {
        field: field.into(),
        op,
        values,
    }
}

fn t(s: &str) -> Value {
    Value::Text(s.into())
}

fn n(x: f64) -> Value {
    Value::Number(x)
}

const FILTERS: &[&str] = &[
    "type = 'City'",
    "type = 'City' and pop >= 10000 and (capital or port)",
    "a or b and c",
    "(a or b) and c",
    "a and (b and c)",
    "a or (b or c)",
    "(a and b) or (c and d)",
    "not capital",
    "pop != 3 and pop <> 4",
    "pop > -5 and pop < 1e6 and pop <= 0.5",
    "pop between 1 and 10",
    "pop not between 1 and 10",
    "name is null or name is not null",
    "name ilike '%ford%' and name ilike 'Ash%'",
    "name ilike '%a_b%'",
    "name like 'Ash%'",
    "type in ('City', 'Town') or type not in ('Ruin')",
    "length(name) > 3 and pop + 1 > 2",
    "not (a or b)",
    "not (a = 1)",
    "\"old name\" = 'x' and \"and\" > 1",
    "x = null",
    "1 = x",
    "x = y",
    "(a or b) = true",
    "capital != true",
];

fn filter_trip(text: &str) -> String {
    Conditions::from_text(text).to_text()
}

#[test]
fn filters_round_trip() {
    for text in FILTERS {
        round_trip(text, filter_trip);
    }
}

#[test]
fn filter_reads_rules_and_groups() {
    let c = Conditions::from_text("type = 'City' and pop >= 10000 and (capital or not port)");
    assert!(c.all);
    assert_eq!(
        c.items,
        vec![
            rule("type", CmpOp::Is, vec![t("City")]),
            rule("pop", CmpOp::Ge, vec![n(10000.0)]),
            Cond::Group {
                all: false,
                items: vec![
                    rule("capital", CmpOp::Is, vec![Value::Bool(true)]),
                    rule("port", CmpOp::Is, vec![Value::Bool(false)]),
                ],
            },
        ]
    );
    let c = Conditions::from_text("a = 1 or b = 2");
    assert!(!c.all);
    assert_eq!(c.items.len(), 2);
    assert_eq!(Conditions::from_text(""), Conditions::new(true, vec![]));
}

#[test]
fn filter_reads_each_comparison() {
    let one = |text: &str| {
        let c = Conditions::from_text(text);
        assert_eq!(c.items.len(), 1, "{text}");
        c.items[0].clone()
    };
    assert_eq!(one("x <> 1"), rule("x", CmpOp::IsNot, vec![n(1.0)]));
    assert_eq!(one("x > -2"), rule("x", CmpOp::Gt, vec![n(-2.0)]));
    assert_eq!(one("x < 1"), rule("x", CmpOp::Lt, vec![n(1.0)]));
    assert_eq!(one("x <= 1"), rule("x", CmpOp::Le, vec![n(1.0)]));
    assert_eq!(
        one("x between 1 and 2"),
        rule("x", CmpOp::Between, vec![n(1.0), n(2.0)])
    );
    assert_eq!(one("x is null"), rule("x", CmpOp::Empty, vec![]));
    assert_eq!(one("x is not null"), rule("x", CmpOp::NotEmpty, vec![]));
    assert_eq!(
        one("x ilike '%ab%'"),
        rule("x", CmpOp::Contains, vec![t("ab")])
    );
    assert_eq!(
        one("x ilike 'ab%'"),
        rule("x", CmpOp::StartsWith, vec![t("ab")])
    );
    assert_eq!(
        one("x in ('a', 'b')"),
        rule("x", CmpOp::OneOf, vec![t("a"), t("b")])
    );
    assert_eq!(
        one("x = true"),
        rule("x", CmpOp::Is, vec![Value::Bool(true)])
    );
    assert_eq!(
        one("x = false"),
        rule("x", CmpOp::Is, vec![Value::Bool(false)])
    );
    for code in [
        "x ilike '%a%b%'",
        "x ilike 'a_%'",
        "x like 'a%'",
        "x not in (1)",
        "1 = x",
        "x = y",
    ] {
        assert_eq!(one(code), Cond::Code(tree(code).to_string()), "{code}");
    }
}

#[test]
fn comparison_with_true_or_false_writes_the_short_form() {
    assert_eq!(filter_trip("capital = true"), "capital");
    assert_eq!(filter_trip("capital = false"), "not capital");
}

#[test]
fn filter_writes_rules() {
    let c = Conditions::new(
        true,
        vec![
            rule("name", CmpOp::Contains, vec![t("it's")]),
            Cond::Group {
                all: false,
                items: vec![
                    rule("pop", CmpOp::Between, vec![n(1.0), n(2.5)]),
                    rule("old name", CmpOp::StartsWith, vec![t("A")]),
                ],
            },
            Cond::Code("a or b".into()),
            Cond::Code("x + 1 > 2".into()),
        ],
    );
    assert_eq!(
        c.to_text(),
        "name ilike '%it''s%' and (pop between 1 and 2.5 or \"old name\" ilike 'A%') and (a or b) and x + 1 > 2"
    );
}

#[test]
fn rules_without_values_write_holes() {
    let c = Conditions::new(true, vec![rule("pop", CmpOp::Ge, vec![]), Cond::hole()]);
    assert_eq!(c.to_text(), "pop >=  and ");
    assert_eq!(
        Conditions::from_text("pop >= ").items,
        vec![rule("pop", CmpOp::Ge, vec![])]
    );
}

#[test]
fn comparisons_follow_the_field_type() {
    let labels = |ty| ops_for(ty).iter().map(|o| o.label()).collect::<Vec<_>>();
    assert_eq!(
        labels(FieldType::Integer),
        [
            "is",
            "is not",
            "is greater than",
            "is at least",
            "is less than",
            "is at most",
            "is between",
            "is one of",
            "is empty",
            "is not empty"
        ]
    );
    assert_eq!(ops_for(FieldType::Real), ops_for(FieldType::Integer));
    assert_eq!(
        labels(FieldType::Text),
        [
            "is",
            "is not",
            "contains",
            "starts with",
            "is one of",
            "is empty",
            "is not empty"
        ]
    );
    assert_eq!(labels(FieldType::Bool), ["is", "is empty", "is not empty"]);
    assert_eq!(CmpOp::Between.value_count(), Some(2));
    assert_eq!(CmpOp::OneOf.value_count(), None);
    assert_eq!(CmpOp::Empty.value_count(), Some(0));
}

const VALUES: &[&str] = &[
    "clamp(4, round(pop / 1000, 1) * 0.5, 22)",
    "(a + b) * c",
    "a - (b - c)",
    "a - b - c",
    "-a * b",
    "-(a * b)",
    "a - -5",
    "-5 * x",
    "- -5",
    "-0",
    "x * -1.5",
    "not capital",
    "not (a or b)",
    "a = 1 or b",
    "(a || b) = c",
    "a || (b || c)",
    "if(capital, 18, if(pop >= 10000, 14, if(port, 12, 10)))",
    "if(capital, 18, 10) * 2",
    "if(if(a, b, c), 1, 2)",
    "coalesce(if(a, 1, 2), 3)",
    "x is null",
    "(x is null) || 'a'",
    "@scale / 1000",
    "null",
    "'it''s' || \"old name\"",
    "true",
    "concat(a, b, c)",
    "if(type = 'City' and pop > 3, 'big', 'small')",
    "1e300",
];

fn value_trip(text: &str) -> String {
    Formula::from_text(text).to_text()
}

#[test]
fn values_round_trip() {
    for text in VALUES {
        round_trip(text, value_trip);
    }
}

fn f(name: &str) -> &'static vale_style::expr::Function {
    FUNCTIONS.iter().find(|f| f.name == name).unwrap()
}

#[test]
fn value_reads_blocks_and_operators() {
    let v = Formula::from_text("clamp(4, round(pop / 1000, 1) * 0.5, 22)");
    let round = Tok::Call {
        f: f("round"),
        args: vec![
            Seq(vec![
                Tok::Field("pop".into()),
                Tok::Op(Op::Div),
                Tok::Num(1000.0),
            ]),
            Seq(vec![Tok::Num(1.0)]),
        ],
    };
    assert_eq!(
        v.seq,
        Seq(vec![Tok::Call {
            f: f("clamp"),
            args: vec![
                Seq(vec![Tok::Num(4.0)]),
                Seq(vec![round, Tok::Op(Op::Mul), Tok::Num(0.5)]),
                Seq(vec![Tok::Num(22.0)]),
            ],
        }])
    );
    let v = Formula::from_text("(a + b) * -2");
    assert_eq!(
        v.seq,
        Seq(vec![
            Tok::Paren(Seq(vec![
                Tok::Field("a".into()),
                Tok::Op(Op::Add),
                Tok::Field("b".into())
            ])),
            Tok::Op(Op::Mul),
            Tok::Num(-2.0),
        ])
    );
    assert_eq!(
        Formula::from_text("@scale").seq,
        Seq(vec![Tok::Code("@scale".into())])
    );
    assert!(Formula::from_text("").seq.is_hole());
}

#[test]
fn nested_ifs_read_as_cases() {
    let v = Formula::from_text("if(capital, 18, if(pop >= 10000, 14, if( , 12, 10)))");
    let Tok::Cases { cases, otherwise } = &v.seq.0[0] else {
        panic!("{:?}", v.seq);
    };
    assert_eq!(cases.len(), 3);
    assert_eq!(
        cases[0].0,
        rule("capital", CmpOp::Is, vec![Value::Bool(true)])
    );
    assert_eq!(cases[1].0, rule("pop", CmpOp::Ge, vec![n(10000.0)]));
    assert_eq!(cases[2].0, Cond::hole());
    assert_eq!(cases[2].1, Seq(vec![Tok::Num(12.0)]));
    assert_eq!(*otherwise, Seq(vec![Tok::Num(10.0)]));
    assert_eq!(
        v.to_text(),
        "if(capital, 18, if(pop >= 10000, 14, if(, 12, 10)))"
    );
    let one = Tok::Cases {
        cases: vec![],
        otherwise: Seq(vec![Tok::Num(1.0)]),
    };
    assert_eq!(one.to_text(), "1");
}

#[test]
fn slots_are_named_by_the_signature() {
    assert_eq!(f("round").slot(0), "x");
    assert_eq!(f("round").slot(1), "digits");
    assert_eq!(f("clamp").slot(2), "high");
    assert_eq!(f("coalesce").slot(5), "b");
}

#[test]
fn paths_address_tokens_slots_and_cases() {
    let text = "clamp(4, round(pop / 1000, 1) * 0.5, 22)";
    let v = Formula::from_text(text);
    let span_text = |path: &[usize]| {
        let s = v.span(path).unwrap();
        text.chars().skip(s.start).take(s.len()).collect::<String>()
    };
    assert_eq!(span_text(&[]), text);
    assert_eq!(span_text(&[0, 1]), "round(pop / 1000, 1) * 0.5");
    assert_eq!(span_text(&[0, 1, 0]), "round(pop / 1000, 1)");
    assert_eq!(span_text(&[0, 1, 0, 0, 2]), "1000");
    assert_eq!(span_text(&[0, 1, 2]), "0.5");
    let pos = text.find("1000").unwrap() + 1;
    assert_eq!(v.path_at(pos), Some(vec![0, 1, 0, 0, 2]));

    let text = "if(capital, 18, if( , 12, 10))";
    let v = Formula::from_text(text);
    assert_eq!(v.span(&[0, 0]), Some(3..10));
    assert_eq!(v.span(&[0, 4]), Some(26..28));
    let err = Expr::parse(text).unwrap_err();
    assert_eq!(v.path_at(err.start), Some(vec![0, 2]));
}
