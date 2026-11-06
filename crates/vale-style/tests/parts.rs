use vale_style::expr::{TextPart, join, split};

fn field(name: &str, prefix: &str, suffix: &str, digits: Option<u32>) -> TextPart {
    TextPart::Field {
        name: name.to_owned(),
        prefix: prefix.to_owned(),
        suffix: suffix.to_owned(),
        digits,
    }
}

/// The text of the parts, and the parts back from that text.
fn round_trip(parts: &[TextPart], text: &str) {
    assert_eq!(join(parts), text);
    assert_eq!(split(text), parts);
    assert_eq!(join(&split(&join(parts))), join(parts));
}

#[test]
fn single_field() {
    round_trip(&[field("name", "", "", None)], "name");
    round_trip(&[field("old name", "", "", None)], "\"old name\"");
}

#[test]
fn field_with_prefix_and_suffix() {
    round_trip(&[field("pop", "Pop ", "", None)], "'Pop ' || pop");
    round_trip(&[field("pop", "", " people", None)], "pop || ' people'");
    round_trip(
        &[field("elevation", " (", " m)", Some(0))],
        "' (' || round(elevation) || ' m)'",
    );
}

#[test]
fn round_with_digits() {
    round_trip(&[field("area", "", "", Some(2))], "round(area, 2)");
    round_trip(&[field("area", "", "", Some(0))], "round(area)");
    assert_eq!(split("round(area, 0)"), [field("area", "", "", Some(0))]);
}

#[test]
fn text_with_quote() {
    round_trip(&[TextPart::Text("King's Road".into())], "'King''s Road'");
}

#[test]
fn many_parts_use_concat() {
    round_trip(
        &[
            field("name", "", "", None),
            field("elevation", " (", " m)", Some(0)),
        ],
        "concat(name, ' (' || round(elevation) || ' m)')",
    );
    round_trip(
        &[
            TextPart::Text("Mt. ".into()),
            field("name", "", "", None),
            field("height", " ", "", Some(1)),
        ],
        "concat('Mt. ', name, ' ' || round(height, 1))",
    );
}

#[test]
fn raw_part_inside_concat_stays_verbatim() {
    let text = "concat(name,  if(a,  b, c ), 'x')";
    let parts = split(text);
    assert_eq!(
        parts,
        [
            field("name", "", "", None),
            TextPart::Raw("if(a,  b, c )".into()),
            TextPart::Text("x".into()),
        ]
    );
    assert_eq!(join(&parts), "concat(name, if(a,  b, c ), 'x')");
}

#[test]
fn raw_whole_expression_stays_verbatim() {
    let text = "upper(name)  ||  'x'";
    assert_eq!(split(text), [TextPart::Raw(text.into())]);
    assert_eq!(join(&split(text)), text);
}

#[test]
fn unparsable_text_is_one_raw_part() {
    let text = "concat(name, 'open";
    assert_eq!(split(text), [TextPart::Raw(text.into())]);
    assert_eq!(join(&split(text)), text);
}

#[test]
fn empty_text_has_no_parts() {
    assert!(split("").is_empty());
    assert_eq!(join(&[]), "");
}
