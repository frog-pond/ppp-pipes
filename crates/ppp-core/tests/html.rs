//! Plain text from HTML: tags, line breaks, entities, whitespace.

use nu_protocol::Value;
use ppp_core::engine::Engine;
use ppp_core::html::to_text;

#[test]
fn tags_go_and_entities_are_decoded() {
    assert_eq!(
        to_text("<p>Men&#8217;s Soccer &amp; <b>more</b></p>"),
        "Men’s Soccer & more"
    );
    assert_eq!(to_text("&lt;not a tag&gt; &nbsp;x"), "<not a tag> x");
}

#[test]
fn block_tags_break_lines() {
    assert_eq!(to_text("<p>One</p><p>Two</p>"), "One\n\nTwo");
    assert_eq!(to_text("a<br>b<BR/>c"), "a\nb\nc");
    assert_eq!(to_text("<ul><li>x</li><li>y</li></ul>"), "x\ny");
}

#[test]
fn scripts_and_styles_are_dropped() {
    assert_eq!(
        to_text("a<script>alert('<p>')</script>b<style>p{}</style>c"),
        "abc"
    );
}

#[test]
fn a_lone_less_than_is_text() {
    assert_eq!(to_text("1 < 2 and 3 > 2"), "1 < 2 and 3 > 2");
}

#[test]
fn comments_and_cdata_end_at_their_own_close() {
    assert_eq!(to_text("a<!-- x > y -->b"), "ab");
    assert_eq!(to_text("a<![CDATA[ x > y ]]>b"), "ab");
}

#[test]
fn a_quoted_greater_than_does_not_end_a_tag() {
    assert_eq!(to_text(r#"<a title="x > y" href='p>q'>link</a>"#), "link");
}

#[test]
fn only_html_element_names_start_tags() {
    assert_eq!(to_text("x<y and z>w"), "x<y and z>w");
    assert_eq!(to_text("a<o:p></o:p>b<my-widget>c</my-widget>"), "abc");
}

#[test]
fn table_cells_stay_apart() {
    assert_eq!(
        to_text(
            "<table><tr><td>Mon</td><td>9am</td></tr><tr><th>Tue</th><th>10am</th></tr></table>"
        ),
        "Mon 9am\nTue 10am"
    );
}

#[test]
fn whitespace_is_tidied() {
    assert_eq!(to_text("  a   b \n\n\n\n c  "), "a b\n\nc");
    assert_eq!(to_text("<div>  </div>"), "");
}

#[test]
fn the_command_passes_null_through() {
    let mut engine = Engine::new();
    let text = engine
        .eval("'<b>x</b>' | html to-text", Value::test_nothing())
        .unwrap();
    assert_eq!(text.coerce_into_string().unwrap(), "x");
    let nothing = engine
        .eval("null | html to-text", Value::test_nothing())
        .unwrap();
    assert!(nothing.is_nothing());
}
