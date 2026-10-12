//! The iCalendar text layer (RFC 5545 §3.1): unfolds lines, splits each
//! content line into name, parameters and value, and nests components.

/// One content line, e.g. `DTSTART;TZID=America/Chicago:20261014T193000`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    /// Upper-cased, e.g. `DTSTART`.
    pub name: String,
    /// Upper-cased parameter names with their raw values, quotes removed, in
    /// source order. A parameter with several values keeps them comma-joined.
    pub params: Vec<(String, String)>,
    /// The raw value, still escaped.
    pub value: String,
}

impl Property {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// A `BEGIN:…`/`END:…` block and everything in it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Component {
    /// Upper-cased, e.g. `VEVENT`.
    pub name: String,
    pub properties: Vec<Property>,
    pub components: Vec<Component>,
}

impl Component {
    pub fn property(&self, name: &str) -> Option<&Property> {
        self.properties.iter().find(|p| p.name == name)
    }

    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Property> {
        self.properties.iter().filter(move |p| p.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {message}")]
pub struct StructureError {
    pub line: usize,
    pub message: String,
}

/// Joins folded lines: a line break followed by a space or tab continues
/// the previous line. Accepts CRLF and bare LF. Returns each logical line
/// with the 1-based number of the physical line it starts on. A leading
/// byte-order mark is dropped.
pub fn unfold(text: &str) -> Vec<(usize, String)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out: Vec<(usize, String)> = Vec::new();
    for (i, raw) in text.split('\n').enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        match line.strip_prefix([' ', '\t']) {
            Some(rest) if !out.is_empty() => out.last_mut().unwrap().1.push_str(rest),
            _ => out.push((i + 1, line.to_string())),
        }
    }
    out.retain(|(_, line)| !line.trim().is_empty());
    out
}

/// Splits `NAME;P1=a;P2="b:c":value`. Returns `None` for a line with no
/// colon outside quotes or with no name.
pub fn parse_line(line: &str) -> Option<Property> {
    let mut in_quotes = false;
    let mut colon = None;
    let mut semis = Vec::new();
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ';' if !in_quotes => semis.push(i),
            ':' if !in_quotes => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    let head = &line[..colon];
    let name_end = semis.first().copied().unwrap_or(colon);
    let name = head[..name_end].trim();
    if name.is_empty() {
        return None;
    }
    let mut params = Vec::new();
    let mut starts = semis.clone();
    starts.push(colon);
    for pair in starts.windows(2) {
        let part = &line[pair[0] + 1..pair[1]];
        if let Some((key, value)) = part.split_once('=') {
            let value = value.replace('"', "");
            params.push((key.trim().to_ascii_uppercase(), value));
        }
    }
    Some(Property {
        name: name.to_ascii_uppercase(),
        params,
        value: line[colon + 1..].to_string(),
    })
}

/// Parses the whole text into its top-level components (normally one
/// `VCALENDAR`). Lines that are not content lines are skipped.
pub fn parse_components(text: &str) -> Result<Vec<Component>, StructureError> {
    let mut stack: Vec<Component> = Vec::new();
    let mut top = Vec::new();
    for (line_no, line) in unfold(text) {
        let Some(prop) = parse_line(&line) else {
            continue;
        };
        match prop.name.as_str() {
            "BEGIN" => stack.push(Component {
                name: prop.value.trim().to_ascii_uppercase(),
                ..Default::default()
            }),
            "END" => {
                let name = prop.value.trim().to_ascii_uppercase();
                let Some(done) = stack.pop() else {
                    return Err(StructureError {
                        line: line_no,
                        message: format!("END:{name} with no matching BEGIN"),
                    });
                };
                if done.name != name {
                    return Err(StructureError {
                        line: line_no,
                        message: format!("END:{name} closes BEGIN:{}", done.name),
                    });
                }
                match stack.last_mut() {
                    Some(parent) => parent.components.push(done),
                    None => top.push(done),
                }
            }
            _ => {
                if let Some(current) = stack.last_mut() {
                    current.properties.push(prop);
                }
            }
        }
    }
    if let Some(open) = stack.last() {
        return Err(StructureError {
            line: 0,
            message: format!("BEGIN:{} is never closed", open.name),
        });
    }
    Ok(top)
}

/// Undoes TEXT escaping (`\n`, `\N`, `\,`, `\;`, `\\`).
pub fn unescape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Splits a TEXT list on unescaped commas, then unescapes and trims each
/// item, dropping empty ones.
pub fn split_text_list(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                current.push('\\');
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ',' => items.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    items.push(current);
    items
        .iter()
        .map(|item| unescape_text(item).trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}
