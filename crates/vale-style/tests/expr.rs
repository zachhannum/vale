use std::collections::BTreeMap;

use vale_store::GeometryKind;
use vale_style::Color;
use vale_style::expr::{Context, Expr, FUNCTIONS, Filter, Property, Value, rename_field};

fn city() -> BTreeMap<String, Value> {
    let mut a = BTreeMap::new();
    a.insert("name".into(), Value::Text("Ravenholm".into()));
    a.insert("kind".into(), Value::Text("city".into()));
    a.insert("population".into(), Value::Number(72000.0));
    a.insert("flow".into(), Value::Number(3.0));
    a.insert("capital".into(), Value::Bool(true));
    a.insert("old name".into(), Value::Text("Raven Hold".into()));
    a.insert("note".into(), Value::Null);
    a
}

fn eval_with(src: &str, scale: f64, geometry: GeometryKind) -> Value {
    let attrs = city();
    let ctx = Context {
        attributes: &attrs,
        scale,
        geometry,
        shape: None,
        radius_km: 0.0,
        page: None,
        rasters: None,
        index: 0,
    };
    Expr::parse(src)
        .unwrap_or_else(|e| panic!("{src}: {e}"))
        .eval(&ctx)
}

fn eval(src: &str) -> Value {
    eval_with(src, 1_000_000.0, GeometryKind::Point)
}

fn t(s: &str) -> Value {
    Value::Text(s.into())
}

fn n(x: f64) -> Value {
    Value::Number(x)
}

fn b(x: bool) -> Value {
    Value::Bool(x)
}

#[test]
fn comparisons() {
    assert_eq!(eval("population > 50000 and kind = 'city'"), b(true));
    assert_eq!(eval("population >= 72000"), b(true));
    assert_eq!(eval("population < 72000"), b(false));
    assert_eq!(eval("population <= 72000"), b(true));
    assert_eq!(eval("kind != 'town'"), b(true));
    assert_eq!(eval("kind <> 'city'"), b(false));
    assert_eq!(eval("'abc' < 'abd'"), b(true));
    assert_eq!(eval("capital = true"), b(true));
    assert_eq!(eval("kind IN ('town', 'city')"), b(true));
    assert_eq!(eval("kind not in ('town', 'city')"), b(false));
    assert_eq!(eval("name like 'Raven%'"), b(true));
    assert_eq!(eval("name like 'raven%'"), b(false));
    assert_eq!(eval("name ilike 'raven%'"), b(true));
    assert_eq!(eval("name like '_avenh_l_'"), b(true));
    assert_eq!(eval("name not like '%x%'"), b(true));
    assert_eq!(eval("note is null"), b(true));
    assert_eq!(eval("name is not null"), b(true));
    assert_eq!(eval("\"old name\" = 'Raven Hold'"), b(true));
}

#[test]
fn logic() {
    assert_eq!(eval("not capital"), b(false));
    assert_eq!(eval("capital or false"), b(true));
    assert_eq!(eval("false or not capital"), b(false));
    assert_eq!(eval("true or false and false"), b(true));
    assert_eq!(eval("(true or false) and false"), b(false));
    assert_eq!(eval("not not true"), b(true));
}

#[test]
fn null_rules() {
    assert_eq!(eval("note = 'x'"), Value::Null);
    assert_eq!(eval("missing > 3"), Value::Null);
    assert_eq!(eval("null and false"), b(false));
    assert_eq!(eval("null and true"), Value::Null);
    assert_eq!(eval("null or true"), b(true));
    assert_eq!(eval("not null"), Value::Null);
    assert_eq!(eval("'5' = 5"), Value::Null);
    assert_eq!(eval("'a' * 2"), Value::Null);
    assert_eq!(eval("1 / 0"), Value::Null);
    assert_eq!(eval("name || note"), Value::Null);
    assert_eq!(eval("concat(name, note, '!')"), t("Ravenholm!"));
}

#[test]
fn arithmetic() {
    assert_eq!(eval("1 + 2 * 3"), n(7.0));
    assert_eq!(eval("(1 + 2) * 3"), n(9.0));
    assert_eq!(eval("10 - 4 - 3"), n(3.0));
    assert_eq!(eval("7 % 4"), n(3.0));
    assert_eq!(eval("-flow + 1"), n(-2.0));
    assert_eq!(eval("population / 1e3"), n(72.0));
    assert_eq!(eval("1.5e-1 * 10"), n(1.5));
    assert_eq!(eval(".5 + 0.25"), n(0.75));
    assert_eq!(eval("flow * 0.4 + 0.5"), n(1.7000000000000002));
}

#[test]
fn text_functions() {
    assert_eq!(eval("upper(name)"), t("RAVENHOLM"));
    assert_eq!(eval("LOWER(name)"), t("ravenholm"));
    assert_eq!(eval("title('the iron  hills')"), t("The Iron  Hills"));
    assert_eq!(eval("trim('  a b ')"), t("a b"));
    assert_eq!(eval("length(name)"), n(9.0));
    assert_eq!(eval("substr(name, 1, 5)"), t("Raven"));
    assert_eq!(eval("substr(name, 6)"), t("holm"));
    assert_eq!(eval("substr(name, -4)"), t("holm"));
    assert_eq!(eval("substr(name, 20)"), t(""));
    assert_eq!(eval("left(name, 3)"), t("Rav"));
    assert_eq!(eval("right(name, 3)"), t("olm"));
    assert_eq!(eval("replace(name, 'holm', 'wood')"), t("Ravenwood"));
    assert_eq!(eval("concat(name, ' ', population)"), t("Ravenholm 72000"));
    assert_eq!(
        eval("name || ' (' || to_text(population) || ')'"),
        t("Ravenholm (72000)")
    );
    assert_eq!(eval("to_text(2.5)"), t("2.5"));
    assert_eq!(eval("to_number(' 42 ')"), n(42.0));
    assert_eq!(eval("to_number('x')"), Value::Null);
    assert_eq!(eval("'it''s'"), t("it's"));
}

#[test]
fn number_functions() {
    assert_eq!(eval("round(2.567, 2)"), n(2.57));
    assert_eq!(eval("round(2.5)"), n(3.0));
    assert_eq!(eval("floor(-1.5)"), n(-2.0));
    assert_eq!(eval("ceil(1.2)"), n(2.0));
    assert_eq!(eval("abs(-3)"), n(3.0));
    assert_eq!(eval("sqrt(16)"), n(4.0));
    assert_eq!(eval("sqrt(-1)"), Value::Null);
    assert_eq!(eval("min(4, flow, note)"), n(3.0));
    assert_eq!(eval("max(4, flow)"), n(4.0));
    assert_eq!(eval("clamp(1, flow * 2, 5)"), n(5.0));
}

#[test]
fn conditional() {
    assert_eq!(eval("if(population > 100000, 'big', 'small')"), t("small"));
    assert_eq!(eval("if(capital, 3, 1)"), n(3.0));
    assert_eq!(eval("if(note, 3, 1)"), n(1.0));
    assert_eq!(eval("coalesce(note, name)"), t("Ravenholm"));
}

#[test]
fn scale_and_geometry() {
    assert_eq!(
        eval_with("@scale < 5000000", 1_000_000.0, GeometryKind::Point),
        b(true)
    );
    assert_eq!(
        eval_with("@scale < 5000000", 50_000_000.0, GeometryKind::Point),
        b(false)
    );
    assert_eq!(
        eval_with("@geometry", 1.0, GeometryKind::Polygon),
        t("polygon")
    );
    assert_eq!(
        eval_with("@GEOMETRY = 'line'", 1.0, GeometryKind::Line),
        b(true)
    );
}

#[test]
fn fields_lists_the_fields() {
    let e = Expr::parse("if(\"old name\" is null, name, upper(name)) || kind").unwrap();
    assert_eq!(e.fields(), vec!["kind", "name", "old name"]);
}

fn error(src: &str) -> (usize, usize, String) {
    let e = Expr::parse(src).expect_err(src);
    (e.start, e.end, e.message)
}

#[test]
fn errors_name_the_position() {
    assert_eq!(error("population > ").0, 13);
    assert_eq!(error("population >> 3").0, 12);
    assert_eq!(error("upper(name").0, 10);
    assert_eq!(error("name = 'abc").0, 7);
    assert_eq!(error("kind = city)").0, 11);
    assert_eq!(error("1 + # 2").0, 4);
    assert_eq!(error("@zoom > 3").0, 0);
    assert_eq!(error("").0, 0);
    assert_eq!(error("a and").0, 5);
    assert_eq!(error("a is 3").0, 5);
    assert_eq!(error("a ! b").0, 2);
    assert_eq!(error("x in 3").0, 5);
    assert_eq!(error("1 < 2 < 3").0, 6);

    let (start, end, msg) = error("flow * shout(name)");
    assert_eq!((start, end), (7, 12));
    assert!(msg.contains("unknown function 'shout'"), "{msg}");

    let (start, _, msg) = error("upper(name, 2)");
    assert_eq!(start, 0);
    assert_eq!(msg, "'upper' takes 1 argument, not 2");
    assert!(error("substr(name)").2.contains("2 to 3"));
}

#[test]
fn error_line_and_column() {
    let src = "kind = 'city'\nand population >";
    let e = Expr::parse(src).unwrap_err();
    assert_eq!((e.line, e.column), (2, 17));
    assert!(e.to_string().starts_with("2:17: "), "{e}");

    let e = Expr::parse("flow * shout(name)").unwrap_err();
    assert_eq!(
        e.pointer("flow * shout(name)"),
        "flow * shout(name)\n       ^^^^^"
    );
}

#[test]
fn deep_nesting_is_an_error() {
    let src = format!("{}1{}", "(".repeat(10_000), ")".repeat(10_000));
    assert!(error(&src).2.contains("nested too deeply"));
    let src = format!("{}true", "not ".repeat(10_000));
    assert!(Expr::parse(&src).is_err());
    let src = format!("{}1", "-".repeat(10_000));
    assert!(Expr::parse(&src).is_err());
    let src = format!("1{}", " + 1".repeat(10_000));
    assert!(Expr::parse(&src).is_err());
    let src = format!("true{}", " or false".repeat(10_000));
    assert!(Expr::parse(&src).is_err());
    let src = format!("{}1{}", "abs(".repeat(30), ")".repeat(30));
    assert_eq!(eval(&src), n(1.0));
    let src = format!("1{}", " + 1".repeat(40));
    assert_eq!(eval(&src), n(41.0));
}

/// A wrong expression must not stop the map from drawing.
#[test]
fn wrong_expressions_still_draw() {
    let attrs = city();
    let ctx = Context {
        attributes: &attrs,
        scale: 1e6,
        geometry: GeometryKind::Line,
        shape: None,
        radius_km: 0.0,
        page: None,
        rasters: None,
        index: 0,
    };

    let bad = Filter::new("population >");
    assert_eq!(bad.error().unwrap().start, 12);
    assert_eq!(bad.source().unwrap().text(), "population >");
    assert!(!bad.matches(&ctx));
    assert!(Filter::new("  ").matches(&ctx));
    assert!(Filter::all().matches(&ctx));
    assert!(Filter::new("kind = 'city'").matches(&ctx));
    assert!(!Filter::new("note = 'x'").matches(&ctx));

    let width = Property::expression("flow * 0.5", 1.0);
    assert!(width.error().is_none());
    assert_eq!(width.eval(&ctx), 1.5);
    let broken = Property::expression("flow * (0.5", 1.0);
    assert!(broken.error().is_some());
    assert_eq!(broken.eval(&ctx), 1.0);
    let wrong_type = Property::expression("name", 1.0);
    assert_eq!(wrong_type.eval(&ctx), 1.0);
    assert_eq!(Property::constant(2.0).eval(&ctx), 2.0);

    let color = Property::expression("if(capital, '#b03030', '#3c3c3c')", Color::rgb(0, 0, 0));
    assert_eq!(color.eval(&ctx), Color::rgb(0xb0, 0x30, 0x30));
    let label = Property::expression("upper(name)", String::new());
    assert_eq!(label.eval(&ctx), "RAVENHOLM");
}

/// Random text must give a result or an error, and never a panic.
#[test]
fn random_text_does_not_panic() {
    const PARTS: [&str; 40] = [
        "(",
        ")",
        ",",
        "'",
        "\"",
        "@",
        "@scale",
        "@geometry",
        "1",
        "2.5",
        "1e400",
        "-",
        "+",
        "*",
        "/",
        "%",
        "||",
        "=",
        "<",
        ">=",
        "!=",
        "!",
        "and",
        "or",
        "not",
        "is",
        "null",
        "in",
        "like",
        "ilike",
        "name",
        "flow",
        "note",
        "if",
        "substr",
        "round",
        "'%_'",
        " ",
        "\n",
        "é",
    ];
    let attrs = city();
    let ctx = Context {
        attributes: &attrs,
        scale: 0.0,
        geometry: GeometryKind::Polygon,
        shape: None,
        radius_km: 0.0,
        page: None,
        rasters: None,
        index: 0,
    };
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..20_000 {
        let len = next() % 12 + 1;
        let src: String = (0..len)
            .map(|_| PARTS[(next() % PARTS.len() as u64) as usize])
            .collect();
        if let Ok(e) = Expr::parse(&src) {
            e.eval(&ctx);
        } else {
            let err = Expr::parse(&src).unwrap_err();
            assert!(err.start <= src.chars().count(), "{src}");
            err.pointer(&src);
        }
    }
}

#[test]
fn each_function_is_documented() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/EXPRESSIONS.md");
    let doc = std::fs::read_to_string(path).expect("docs/EXPRESSIONS.md");
    for f in FUNCTIONS {
        assert!(
            doc.contains(&format!("`{}`", f.signature)),
            "{} is not in docs/EXPRESSIONS.md",
            f.signature
        );
    }
}

#[test]
fn short_error_is_a_sentence() {
    let err = Expr::parse("shout(name)").unwrap_err();
    assert_eq!(err.short(), "Unknown function 'shout'.");
}

#[test]
fn rename_bare_name() {
    assert_eq!(
        rename_field("pop > 10 and kind = 'pop'", "pop", "people"),
        "people > 10 and kind = 'pop'"
    );
}

#[test]
fn rename_quoted_name() {
    assert_eq!(
        rename_field("\"old name\" || \"pop\"", "old name", "former"),
        "former || \"pop\""
    );
}

#[test]
fn rename_to_name_with_space_quotes_it() {
    assert_eq!(
        rename_field("concat(name,  ' ')", "name", "full name"),
        "concat(\"full name\",  ' ')"
    );
}

#[test]
fn rename_skips_function_with_same_name() {
    assert_eq!(rename_field("round(round)", "round", "r"), "round(r)");
}

#[test]
fn rename_skips_text_and_keywords() {
    assert_eq!(rename_field("'name' || name", "name", "n"), "'name' || n");
    assert_eq!(rename_field("a is null", "null", "n"), "a is null");
}

#[test]
fn rename_keeps_text_that_does_not_lex() {
    assert_eq!(rename_field("name || 'open", "name", "n"), "name || 'open");
}

#[test]
fn format_number_groups_and_rounds() {
    assert_eq!(eval("format_number(1234567.891, 2)"), t("1,234,567.89"));
    assert_eq!(eval("format_number(population)"), t("72,000"));
    assert_eq!(eval("format_number(1234.5)"), t("1,235"));
    assert_eq!(eval("format_number(-1234.5)"), t("-1,235"));
    assert_eq!(eval("format_number(0.5, 0)"), t("1"));
    assert_eq!(eval("format_number(12.3, 3)"), t("12.300"));
    assert_eq!(eval("format_number(999.996, 2)"), t("1,000.00"));
    assert_eq!(eval("format_number(-0.001, 2)"), t("0.00"));
    assert_eq!(eval("format_number(123, -2)"), t("123"));
    assert_eq!(eval("format_number(1.5, 40)"), t("1.500000000000000"));
    assert_eq!(eval("format_number(name, 2)"), Value::Null);
    assert_eq!(eval("format_number(note, 2)"), Value::Null);
    assert_eq!(eval("format_number(1, 'x')"), Value::Null);
}

#[test]
fn each_function_has_a_category_and_a_hint() {
    use vale_style::expr::Category;
    let find = |name: &str| FUNCTIONS.iter().find(|f| f.name == name).unwrap();
    assert_eq!(find("round").category, Category::Math);
    assert_eq!(find("upper").category, Category::Text);
    assert_eq!(find("if").category, Category::Logic);
    assert_eq!(find("format_number").category, Category::Convert);
    assert_eq!(find("round").hint, "Round to decimal places");
    for f in FUNCTIONS {
        assert!(!f.hint.is_empty() && !f.hint.ends_with('.'), "{}", f.name);
    }
}

#[test]
fn between_is_inclusive() {
    assert_eq!(eval("population between 72000 and 80000"), b(true));
    assert_eq!(eval("population BETWEEN 1000 AND 72000"), b(true));
    assert_eq!(eval("population between 72001 and 80000"), b(false));
    assert_eq!(eval("population not between 72001 and 80000"), b(true));
    assert_eq!(eval("name between 'R' and 'S'"), b(true));
    assert_eq!(eval("flow between 1 + 1 and 4 and capital"), b(true));
    assert_eq!(eval("note between 1 and 2"), Value::Null);
    assert_eq!(eval("name between 1 and 2"), Value::Null);
    assert!(Expr::parse("x between 1").is_err());
    assert!(Expr::parse("between > 1").is_err());
    assert_eq!(
        Expr::parse("x between 1 and 2").unwrap().fields(),
        vec!["x".to_string()]
    );
}

#[test]
fn lenient_parse_reads_holes() {
    let printed = |s: &str| Expr::parse_lenient(s).unwrap().to_string();
    assert_eq!(printed(""), "");
    assert_eq!(printed("if( , 12, 10)"), "if(, 12, 10)");
    assert_eq!(printed("if(a,,)"), "if(a, , )");
    assert_eq!(printed("round()"), "round()");
    assert_eq!(printed("clamp(1)"), "clamp(1, , )");
    assert_eq!(printed("a +"), "a + ");
    assert_eq!(printed("* 2"), " * 2");
    assert_eq!(printed("and b"), " and b");
    assert_eq!(printed("x in ()"), "x in ()");
    assert_eq!(printed("x between 1 and"), "x between 1 and ");
    assert_eq!(printed("a = 1"), "a = 1");
    assert!(Expr::parse_lenient("shout(1)").is_err());
    assert!(Expr::parse_lenient("(a").is_err());
    assert_eq!(eval("1"), n(1.0));
    let attrs = city();
    let ctx = Context {
        attributes: &attrs,
        scale: 1.0,
        geometry: GeometryKind::Point,
        shape: None,
        radius_km: 0.0,
        page: None,
        rasters: None,
        index: 0,
    };
    assert_eq!(
        Expr::parse_lenient("coalesce(, 3)").unwrap().eval(&ctx),
        n(3.0)
    );
}

#[test]
fn strict_parse_has_no_holes() {
    for text in ["", "if( , 12, 10)", "round()", "a +", "x in ()", "* 2"] {
        assert!(Expr::parse(text).is_err(), "{text}");
    }
}

/// Page values with a fixed scale and page number.
struct StubPage;

impl vale_style::expr::PageValues for StubPage {
    fn value(&self, name: &str, args: &[Value]) -> Value {
        match (name, args) {
            ("scale", [Value::Text(f)]) if f == "fraction" => Value::Text("1:25,000,000".into()),
            ("page_number", []) => Value::Number(3.0),
            _ => Value::Null,
        }
    }
}

#[test]
fn page_function_reads_the_page() {
    let attrs = city();
    let expr = Expr::parse("concat('Scale ', scale('fraction'))").unwrap();
    let mut ctx = Context {
        attributes: &attrs,
        scale: 1.0,
        geometry: GeometryKind::Point,
        shape: None,
        radius_km: 0.0,
        page: Some(&StubPage),
        rasters: None,
        index: 0,
    };
    assert_eq!(expr.eval(&ctx), Value::Text("Scale 1:25,000,000".into()));
    let number = Expr::parse("page_number() + 1").unwrap();
    assert_eq!(number.eval(&ctx), Value::Number(4.0));
    ctx.page = None;
    assert_eq!(expr.eval(&ctx), Value::Text("Scale ".into()));
    assert_eq!(number.eval(&ctx), Value::Null);
}

#[test]
fn layer_functions_leave_out_the_page_functions() {
    use vale_style::expr::{Category, layer_functions};
    let page: Vec<_> = (FUNCTIONS.iter())
        .filter(|f| f.category == Category::Page)
        .map(|f| f.name)
        .collect();
    assert_eq!(
        page,
        [
            "scale",
            "map_name",
            "page_name",
            "page_number",
            "page_count",
            "date"
        ]
    );
    assert!(layer_functions().all(|f| f.category != Category::Page));
    assert_eq!(layer_functions().count() + page.len(), FUNCTIONS.len());
    assert_eq!(eval("date('iso')"), Value::Null);
}
