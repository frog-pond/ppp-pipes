//! The iCalendar text layer: unfolding, content lines, components, TEXT.

use ppp_core::ics::lines::{
    Property, parse_components, parse_line, split_text_list, unescape_text, unfold,
};

#[test]
fn unfolds_crlf_and_lf_with_space_or_tab() {
    let text = "SUMMARY:Fall Or\r\n chestra\r\nDESCRIPTION:a\n\tb\n\nEND:VEVENT";
    assert_eq!(
        unfold(text),
        vec![
            (1, "SUMMARY:Fall Orchestra".to_string()),
            (3, "DESCRIPTION:ab".to_string()),
            (6, "END:VEVENT".to_string()),
        ]
    );
}

#[test]
fn splits_name_params_and_value() {
    let prop = parse_line(r#"organizer;cn="Doe; Jane: Office";ROLE=CHAIR:mailto:doe@example.edu"#)
        .unwrap();
    assert_eq!(
        prop,
        Property {
            name: "ORGANIZER".into(),
            params: vec![
                ("CN".into(), "Doe; Jane: Office".into()),
                ("ROLE".into(), "CHAIR".into())
            ],
            value: "mailto:doe@example.edu".into(),
        }
    );
    assert_eq!(prop.param("CN"), Some("Doe; Jane: Office"));
}

#[test]
fn lines_without_a_colon_or_name_are_not_properties() {
    assert_eq!(parse_line("just some text"), None);
    assert_eq!(parse_line(":value"), None);
}

#[test]
fn nests_components() {
    let text = "BEGIN:VCALENDAR\nX-WR-TIMEZONE:America/Chicago\nBEGIN:VTIMEZONE\nTZID:America/Chicago\nBEGIN:STANDARD\nRRULE:FREQ=YEARLY\nEND:STANDARD\nEND:VTIMEZONE\nBEGIN:VEVENT\nUID:a\nBEGIN:VALARM\nACTION:DISPLAY\nEND:VALARM\nEND:VEVENT\nEND:VCALENDAR\n";
    let top = parse_components(text).unwrap();
    assert_eq!(top.len(), 1);
    let cal = &top[0];
    assert_eq!(cal.name, "VCALENDAR");
    assert_eq!(
        cal.property("X-WR-TIMEZONE").unwrap().value,
        "America/Chicago"
    );
    let names: Vec<&str> = cal.components.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["VTIMEZONE", "VEVENT"]);
    assert_eq!(
        cal.components[0].components[0]
            .property("RRULE")
            .unwrap()
            .value,
        "FREQ=YEARLY"
    );
    assert_eq!(cal.components[1].components[0].name, "VALARM");
    assert_eq!(cal.components[1].properties.len(), 1);
}

#[test]
fn mismatched_end_is_an_error() {
    let err = parse_components("BEGIN:VCALENDAR\nBEGIN:VEVENT\nEND:VCALENDAR\n").unwrap_err();
    assert_eq!(err.line, 3);
    assert_eq!(err.message, "END:VCALENDAR closes BEGIN:VEVENT");
}

#[test]
fn unclosed_begin_is_an_error() {
    let err = parse_components("BEGIN:VCALENDAR\nBEGIN:VEVENT\n").unwrap_err();
    assert_eq!(err.message, "BEGIN:VEVENT is never closed");
}

#[test]
fn unescapes_text() {
    assert_eq!(unescape_text(r"a\, b\; c\nd\Ne\\f"), "a, b; c\nd\ne\\f");
}

#[test]
fn splits_text_lists_on_unescaped_commas() {
    assert_eq!(
        split_text_list(r"Music, Concert\, Fall,,  Arts "),
        vec!["Music", "Concert, Fall", "Arts"]
    );
}

#[test]
fn a_byte_order_mark_is_ignored() {
    let top = parse_components("\u{feff}BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n").unwrap();
    assert_eq!(top.len(), 1);
    assert_eq!(top[0].name, "VCALENDAR");
}
