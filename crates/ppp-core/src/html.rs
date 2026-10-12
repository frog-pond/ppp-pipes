//! Plain text from HTML, for `html to-text` and `into event`.
//!
//! Sources put HTML in fields that should be plain text (Tribe titles,
//! Carleton descriptions, Northfield locations). This is not a renderer:
//! it drops tags, turns block-level tags into line breaks, drops the
//! contents of `<script>` and `<style>`, decodes entities, and tidies
//! whitespace.

/// Tags that stand apart as paragraphs: a blank line before and after.
const PARAGRAPH_TAGS: &[&str] = &[
    "address",
    "article",
    "blockquote",
    "div",
    "dl",
    "figcaption",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "ul",
];

/// Tags that start a new line where they open.
const LINE_TAGS: &[&str] = &["br", "dd", "dt", "li", "tr"];

pub fn to_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        text.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let starts_tag =
            after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
        let Some(close) = after.find('>').filter(|_| starts_tag) else {
            // A `<` that doesn't start a tag is text, e.g. "1 < 2".
            text.push('<');
            rest = after;
            continue;
        };
        let tag = &after[..close];
        let closing = tag.starts_with('/');
        let name = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        rest = &after[close + 1..];
        if (name == "script" || name == "style") && !closing {
            let end = format!("</{name}");
            rest = match rest.to_ascii_lowercase().find(&end) {
                Some(at) => rest[at..].split_once('>').map_or("", |(_, r)| r),
                None => "",
            };
        } else if PARAGRAPH_TAGS.contains(&name.as_str()) {
            text.push_str("\n\n");
        } else if LINE_TAGS.contains(&name.as_str()) && !closing {
            text.push('\n');
        }
    }
    text.push_str(rest);
    tidy(&htmlize::unescape(&text))
}

/// Collapses runs of spaces within lines, trims each line, and keeps at most
/// one blank line between paragraphs.
fn tidy(text: &str) -> String {
    let mut out = Vec::new();
    let mut blank = false;
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if blank {
            out.push(String::new());
            blank = false;
        }
        out.push(line);
    }
    out.join("\n")
}
