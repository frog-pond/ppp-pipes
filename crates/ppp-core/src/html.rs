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

/// Table cells, which a space keeps apart.
const CELL_TAGS: &[&str] = &["td", "th"];

/// HTML element names, including obsolete ones and SVG's common shapes. A
/// `<` followed by anything else is text, as in "x<y and z>w".
const ELEMENTS: &[&str] = &[
    "a",
    "abbr",
    "acronym",
    "address",
    "area",
    "article",
    "aside",
    "audio",
    "b",
    "base",
    "basefont",
    "bdi",
    "bdo",
    "big",
    "blink",
    "blockquote",
    "body",
    "br",
    "button",
    "canvas",
    "caption",
    "center",
    "circle",
    "cite",
    "code",
    "col",
    "colgroup",
    "data",
    "datalist",
    "dd",
    "defs",
    "del",
    "details",
    "dfn",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "ellipse",
    "em",
    "embed",
    "fieldset",
    "figcaption",
    "figure",
    "font",
    "footer",
    "form",
    "frame",
    "frameset",
    "g",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "html",
    "i",
    "iframe",
    "img",
    "input",
    "ins",
    "kbd",
    "label",
    "legend",
    "li",
    "line",
    "link",
    "main",
    "map",
    "mark",
    "marquee",
    "math",
    "menu",
    "meta",
    "meter",
    "nav",
    "nobr",
    "noframes",
    "noscript",
    "object",
    "ol",
    "optgroup",
    "option",
    "output",
    "p",
    "param",
    "path",
    "picture",
    "polygon",
    "polyline",
    "pre",
    "progress",
    "q",
    "rect",
    "rp",
    "rt",
    "ruby",
    "s",
    "samp",
    "script",
    "search",
    "section",
    "select",
    "slot",
    "small",
    "source",
    "span",
    "strike",
    "strong",
    "style",
    "sub",
    "summary",
    "sup",
    "svg",
    "table",
    "tbody",
    "td",
    "template",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "time",
    "title",
    "tr",
    "track",
    "tt",
    "u",
    "ul",
    "use",
    "var",
    "video",
    "wbr",
];

pub fn to_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        text.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        // Comments and CDATA end at their own marks, and may hold a `>`.
        // Browsers drop CDATA in HTML, so it is dropped here too.
        let past_comment = [("!--", "-->"), ("![CDATA[", "]]>")]
            .iter()
            .find_map(|(start, end)| {
                let body = after.strip_prefix(start)?;
                Some(body.split_once(end).map_or("", |(_, r)| r))
            });
        if let Some(past) = past_comment {
            rest = past;
            continue;
        }
        let closing = after.starts_with('/');
        let name = after
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == ':' || *c == '-')
            .collect::<String>()
            .to_ascii_lowercase();
        let starts_tag = after.starts_with('!') || is_element(&name);
        let Some(close) = tag_end(after).filter(|_| starts_tag) else {
            // A `<` that doesn't start a tag is text, e.g. "1 < 2".
            text.push('<');
            rest = after;
            continue;
        };
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
        } else if CELL_TAGS.contains(&name.as_str()) && !closing {
            text.push(' ');
        }
    }
    text.push_str(rest);
    tidy(&htmlize::unescape(&text))
}

/// Whether `name` names an element: an HTML one, or a namespaced or custom
/// one such as Word's `o:p` or `my-widget`.
fn is_element(name: &str) -> bool {
    ELEMENTS.contains(&name)
        || (name.starts_with(|c: char| c.is_ascii_alphabetic())
            && (name.contains(':') || name.contains('-')))
}

/// Where the tag that `after` starts ends: its first `>` outside a quoted
/// attribute value. A quote opens a value only straight after `=`.
fn tag_end(after: &str) -> Option<usize> {
    let mut quote = None;
    let mut last = ' ';
    for (i, c) in after.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '>' => return Some(i),
            None if (c == '"' || c == '\'') && last == '=' => quote = Some(c),
            None => {}
        }
        if !c.is_whitespace() {
            last = c;
        }
    }
    None
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
