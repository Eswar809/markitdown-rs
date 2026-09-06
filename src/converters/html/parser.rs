//! A lenient HTML parser modeled on Python's `html.parser` + BeautifulSoup
//! behavior (the combination upstream markitdown uses). Unlike html5ever it
//! does NOT insert implicit elements (e.g. `<tbody>`), which matters because
//! markdownify's table logic keys off parent tag names.
//!
//! Features: void elements, implied end tags (li/li, p/block, td/th/tr...),
//! comments, doctype, entity decoding (common named + numeric).

#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    Document,
    Element { name: String, attrs: Vec<(String, String)> },
    Text(String),
    Comment,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub kind: NodeKind,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
}

/// Arena-based DOM. Index 0 is always the document node (named "[document]",
/// like BeautifulSoup's object).
pub struct Dom {
    pub nodes: Vec<Node>,
}

impl Dom {
    pub fn name(&self, idx: usize) -> Option<&str> {
        match &self.nodes[idx].kind {
            NodeKind::Document => Some("[document]"),
            NodeKind::Element { name, .. } => Some(name),
            _ => None,
        }
    }

    pub fn attr(&self, idx: usize, key: &str) -> Option<&str> {
        match &self.nodes[idx].kind {
            NodeKind::Element { attrs, .. } => attrs
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.as_str()),
            _ => None,
        }
    }

    pub fn has_attr(&self, idx: usize, key: &str) -> bool {
        match &self.nodes[idx].kind {
            NodeKind::Element { attrs, .. } => {
                attrs.iter().any(|(k, _)| k.eq_ignore_ascii_case(key))
            }
            _ => false,
        }
    }

    pub fn text(&self, idx: usize) -> Option<&str> {
        match &self.nodes[idx].kind {
            NodeKind::Text(t) => Some(t),
            _ => None,
        }
    }

    /// First ancestor (parent chain) whose tag equals `name` — equivalent of
    /// BeautifulSoup's `el.find_parent(name)`.
    pub fn find_parent(&self, idx: usize, name: &str) -> bool {
        let mut cur = self.nodes[idx].parent;
        while let Some(p) = cur {
            if self.name(p) == Some(name) {
                return true;
            }
            cur = self.nodes[p].parent;
        }
        false
    }

    /// Recursive collect of descendant element indices with any of `names`.
    pub fn find_all(&self, idx: usize, names: &[&str]) -> Vec<usize> {
        let mut out = Vec::new();
        self.find_all_inner(idx, names, &mut out);
        out
    }

    fn find_all_inner(&self, idx: usize, names: &[&str], out: &mut Vec<usize>) {
        for &child in &self.nodes[idx].children {
            if let Some(n) = self.name(child) {
                if names.contains(&n) {
                    out.push(child);
                }
            }
            self.find_all_inner(child, names, out);
        }
    }

    /// Sibling position helpers over the raw (unfiltered) child list — the
    /// markdownify port needs the original sibling chain, including
    /// whitespace-only text nodes.
    pub fn prev_sibling(&self, idx: usize) -> Option<usize> {
        let parent = self.nodes[idx].parent?;
        let siblings = &self.nodes[parent].children;
        let pos = siblings.iter().position(|&c| c == idx)?;
        if pos == 0 { None } else { Some(siblings[pos - 1]) }
    }

    pub fn next_sibling(&self, idx: usize) -> Option<usize> {
        let parent = self.nodes[idx].parent?;
        let siblings = &self.nodes[parent].children;
        let pos = siblings.iter().position(|&c| c == idx)?;
        siblings.get(pos + 1).copied()
    }

    /// Previous *element* sibling (skipping text/comments) — for <li> numbering
    /// (`find_previous_siblings('li')`) and <tr> "first row" checks we expose
    /// both raw and element-filtered variants.
    pub fn prev_element_sibling(&self, idx: usize) -> Option<usize> {
        let parent = self.nodes[idx].parent?;
        let siblings = &self.nodes[parent].children;
        let pos = siblings.iter().position(|&c| c == idx)?;
        siblings[..pos]
            .iter()
            .rev()
            .copied()
            .find(|&s| matches!(self.nodes[s].kind, NodeKind::Element { .. }))
    }

    /// Count previous element siblings with the given name — port of
    /// `len(el.find_previous_siblings('li'))`.
    pub fn count_prev_siblings_named(&self, idx: usize, name: &str) -> usize {
        let parent = self.nodes[idx].parent;
        let Some(parent) = parent else { return 0 };
        let siblings = &self.nodes[parent].children;
        let pos = siblings.iter().position(|&c| c == idx).unwrap_or(0);
        siblings[..pos]
            .iter()
            .filter(|&&s| self.name(s) == Some(name))
            .count()
    }

    /// Whether the node has any previous sibling of ANY kind (raw chain) —
    /// port of `el.find_previous_sibling() is None`.
    pub fn has_prev_sibling_raw(&self, idx: usize) -> bool {
        self.prev_sibling(idx).is_some()
    }
}

const VOID_ELEMENTS: [&str; 14] = [
    "br", "img", "input", "hr", "meta", "link", "area", "base", "col", "embed",
    "source", "track", "wbr", "param",
];

/// Tags that imply the end of a currently-open tag when they start — a
/// pragmatic subset of BS4/html.parser nesting recovery.
fn implied_end_on_start(open: &str, new: &str) -> bool {
    const BLOCK: [&str; 24] = [
        "p", "div", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "li",
        "table", "blockquote", "pre", "form", "section", "article", "header",
        "footer", "nav", "aside", "dl", "figure", "main",
    ];
    const HEADINGS: [&str; 6] = ["h1", "h2", "h3", "h4", "h5", "h6"];
    match open {
        "p" => BLOCK.contains(&new),
        h if HEADINGS.contains(&h) => HEADINGS.contains(&new),
        "li" => new == "li",
        "dt" | "dd" => matches!(new, "dt" | "dd"),
        "option" => matches!(new, "option" | "optgroup"),
        "tr" => matches!(new, "tr" | "thead" | "tbody" | "tfoot"),
        "td" | "th" => matches!(new, "td" | "th" | "tr" | "thead" | "tbody" | "tfoot"),
        "thead" => matches!(new, "tbody" | "tfoot"),
        "tbody" | "tfoot" => matches!(new, "tbody" | "tfoot"),
        "head" => new == "body",
        _ => false,
    }
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if let Some(end) = s[i..].find(';').map(|off| i + off).filter(|&e| e - i <= 10) {
                let entity = &s[i + 1..end];
                let decoded = match entity {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some('\u{a0}'),
                    "copy" => Some('\u{a9}'),
                    "reg" => Some('\u{ae}'),
                    "trade" => Some('\u{2122}'),
                    "hellip" => Some('\u{2026}'),
                    "mdash" => Some('\u{2014}'),
                    "ndash" => Some('\u{2013}'),
                    "lsquo" => Some('\u{2018}'),
                    "rsquo" => Some('\u{2019}'),
                    "ldquo" => Some('\u{201c}'),
                    "rdquo" => Some('\u{201d}'),
                    "times" => Some('\u{d7}'),
                    "divide" => Some('\u{f7}'),
                    "eacute" => Some('\u{e9}'),
                    _ => {
                        if let Some(num) = entity.strip_prefix('#') {
                            let code = if let Some(hex) = num.strip_prefix('x')
                                .or_else(|| num.strip_prefix('X'))
                            {
                                u32::from_str_radix(hex, 16).ok()
                            } else {
                                num.parse::<u32>().ok()
                            };
                            code.and_then(char::from_u32)
                        } else {
                            None
                        }
                    }
                };
                if let Some(c) = decoded {
                    out.push(c);
                    i = end + 1;
                    continue;
                }
            }
        }
        let c = s[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// Parses HTML into a Dom. Whitespace text nodes are preserved (markdownify's
/// sibling logic needs the raw chain).
pub fn parse(html: &str) -> Dom {
    let mut dom = Dom { nodes: vec![Node { kind: NodeKind::Document, parent: None, children: Vec::new() }] };
    let mut stack: Vec<usize> = vec![0]; // document root

    let bytes = html.as_bytes();
    let mut i = 0usize;
    let mut text_start = i;

    fn push_text(dom: &mut Dom, stack: &[usize], text: String) {
        if text.is_empty() {
            return;
        }
        let parent = *stack.last().unwrap();
        let idx = dom.nodes.len();
        dom.nodes.push(Node { kind: NodeKind::Text(decode_entities(&text)), parent: Some(parent), children: Vec::new() });
        dom.nodes[parent].children.push(idx);
    }

    while i < bytes.len() {
        if bytes[i] == b'<' {
            // flush pending text
            if i > text_start {
                let t = html[text_start..i].to_string();
                let st = stack.clone();
                push_text(&mut dom, &st, t);
            }
            if html[i..].starts_with("<!--") {
                let end = html[i..].find("-->").map(|off| i + off + 3).unwrap_or(bytes.len());
                let parent = *stack.last().unwrap();
                let idx = dom.nodes.len();
                dom.nodes.push(Node { kind: NodeKind::Comment, parent: Some(parent), children: Vec::new() });
                dom.nodes[parent].children.push(idx);
                i = end;
            } else if html[i..].len() > 1 && (bytes[i + 1] == b'!' || bytes[i + 1] == b'?') {
                // doctype / processing instruction — skip to '>'
                let end = html[i..].find('>').map(|off| i + off + 1).unwrap_or(bytes.len());
                i = end;
            } else if html[i..].starts_with("</") {
                // end tag
                let end = html[i..].find('>').map(|off| i + off + 1).unwrap_or(bytes.len());
                let raw = &html[i + 2..end.saturating_sub(1)];
                let name = raw.split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("").to_lowercase();
                if !name.is_empty() && !VOID_ELEMENTS.contains(&name.as_str()) {
                    if let Some(pos) = stack.iter().rposition(|&s| dom.name(s) == Some(name.as_str())) {
                        if pos > 0 {
                            stack.truncate(pos);
                        }
                    }
                }
                i = end;
            } else {
                // start tag (find_tag_end returns one past the '>')
                let end = find_tag_end(html, i).unwrap_or(bytes.len());
                let inner = &html[i + 1..end.saturating_sub(1)];
                let self_closing = inner.ends_with('/');
                let inner = inner.trim_end_matches('/');
                let (name, attrs) = parse_tag(inner);
                let name = name.to_lowercase();

                // implied end tags
                while stack.len() > 1 {
                    let top = *stack.last().unwrap();
                    match dom.name(top) {
                        Some(open) if implied_end_on_start(open, &name) => {
                            stack.pop();
                        }
                        _ => break,
                    }
                }

                let parent = *stack.last().unwrap();
                let idx = dom.nodes.len();
                dom.nodes.push(Node {
                    kind: NodeKind::Element { name: name.clone(), attrs },
                    parent: Some(parent),
                    children: Vec::new(),
                });
                dom.nodes[parent].children.push(idx);

                if !VOID_ELEMENTS.contains(&name.as_str()) && !self_closing {
                    stack.push(idx);
                }
                i = end;
            }
            text_start = i;
        } else {
            i += 1;
        }
    }
    if text_start < bytes.len() {
        let t = html[text_start..].to_string();
        let st = stack.clone();
        push_text(&mut dom, &st, t);
    }
    dom
}

/// Finds the '>' closing a start tag, respecting quoted attribute values.
fn find_tag_end(html: &str, start: usize) -> Option<usize> {
    let bytes = html.as_bytes();
    let mut i = start + 1;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        match (quote, bytes[i]) {
            (Some(q), c) if c == q => quote = None,
            (None, b'"') | (None, b'\'') => quote = Some(bytes[i]),
            (None, b'>') => return Some(i + 1),
            _ => {}
        }
        i += 1;
    }
    None
}

fn parse_tag(inner: &str) -> (&str, Vec<(String, String)>) {
    let mut parts = inner.splitn(2, |c: char| c.is_whitespace());
    let name = parts.next().unwrap_or("");
    let mut attrs = Vec::new();
    if let Some(rest) = parts.next() {
        let chars: Vec<char> = rest.chars().collect();
        let mut i = 0usize;
        while i < chars.len() {
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            let start = i;
            while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '=' {
                i += 1;
            }
            let key: String = chars[start..i].iter().collect();
            if key.is_empty() {
                i += 1;
                continue;
            }
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            let value = if i < chars.len() && chars[i] == '=' {
                i += 1;
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
                if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
                    let q = chars[i];
                    i += 1;
                    let vstart = i;
                    while i < chars.len() && chars[i] != q {
                        i += 1;
                    }
                    let v: String = chars[vstart..i].iter().collect();
                    i += 1; // closing quote
                    v
                } else {
                    let vstart = i;
                    while i < chars.len() && !chars[i].is_whitespace() {
                        i += 1;
                    }
                    chars[vstart..i].iter().collect()
                }
            } else {
                String::new() // boolean attribute
            };
            attrs.push((key.to_lowercase(), decode_entities(&value)));
        }
    }
    (name, attrs)
}
