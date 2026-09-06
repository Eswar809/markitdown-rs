//! Port of `markdownify` (1.2.3) + markitdown's `_CustomMarkdownify` customizations.
//!
//! Version quirk preserved faithfully: markdownify 1.2.3 calls convert functions
//! with `parent_tags=` keyword; the custom overrides (`convert_a`, `convert_img`,
//! `convert_u`, `convert_input`, `convert_strike`) declare `convert_as_inline`
//! instead, so `parent_tags` lands in `**kwargs` and their inline checks see
//! `False` — while base converters (p, li, blockquote, hN, td/th...) receive
//! the real parent context. Also, the custom `convert_hn` (lowercase n) is dead
//! code upstream — markdownify dispatches to `convert_hN` — so headings always
//! use the base ATX converter.

use std::collections::HashSet;

use super::parser::{Dom, NodeKind};

pub struct Options;

impl Options {
    pub fn escape_asterisks(&self) -> bool {
        true
    }
    pub fn escape_underscores(&self) -> bool {
        true
    }
}

const BULLETS: [&str; 3] = ["*", "+", "-"];

pub fn convert(dom: &Dom, root: usize) -> String {
    let parent_tags = HashSet::new();
    let text = process_tag(dom, root, &parent_tags);
    // convert__document_: strip('\n') — only applies to the document node
    // (the body element has no convert function).
    if dom.name(root) == Some("[document]") {
        text.trim_matches('\n').to_string()
    } else {
        text
    }
}

// ---------------------------------------------------------------------------
// Core traversal — port of process_tag / process_text
// ---------------------------------------------------------------------------

fn is_heading(name: &str) -> bool {
    matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

fn should_remove_whitespace_inside_name(name: Option<&str>) -> bool {
    let Some(name) = name else { return false };
    if is_heading(name) {
        return true;
    }
    matches!(
        name,
        "p" | "blockquote"
            | "article"
            | "div"
            | "section"
            | "ol"
            | "ul"
            | "li"
            | "dl"
            | "dt"
            | "dd"
            | "table"
            | "thead"
            | "tbody"
            | "tfoot"
            | "tr"
            | "td"
            | "th"
    )
}

fn should_remove_whitespace_outside_node(dom: &Dom, idx: Option<usize>) -> bool {
    match idx {
        None => false,
        Some(i) => {
            should_remove_whitespace_inside_name(dom.name(i)) || dom.name(i) == Some("pre")
        }
    }
}

fn process_tag(dom: &Dom, idx: usize, parent_tags: &HashSet<String>) -> String {
    match &dom.nodes[idx].kind {
        NodeKind::Text(t) => process_text(dom, idx, t, parent_tags),
        _ => process_element(dom, idx, parent_tags),
    }
}

fn can_ignore_child(
    dom: &Dom,
    child: usize,
    should_remove_inside: bool,
    prev: Option<usize>,
    next: Option<usize>,
) -> bool {
    match &dom.nodes[child].kind {
        NodeKind::Element { .. } => false,
        NodeKind::Comment => true,
        NodeKind::Document => true,
        NodeKind::Text(t) => {
            if !t.trim().is_empty() {
                false
            } else if should_remove_inside && (prev.is_none() || next.is_none()) {
                true
            } else if should_remove_whitespace_outside_node(dom, prev)
                || should_remove_whitespace_outside_node(dom, next)
            {
                true
            } else {
                false
            }
        }
    }
}

fn process_element(dom: &Dom, idx: usize, parent_tags: &HashSet<String>) -> String {
    let name = dom.name(idx).unwrap_or("").to_string();
    let should_remove_inside = should_remove_whitespace_inside_name(Some(&name));
    let kids = &dom.nodes[idx].children;

    let mut children_to_convert: Vec<usize> = Vec::new();
    for (pos, &child) in kids.iter().enumerate() {
        let prev = if pos > 0 { Some(kids[pos - 1]) } else { None };
        let next = kids.get(pos + 1).copied();
        if !can_ignore_child(dom, child, should_remove_inside, prev, next) {
            children_to_convert.push(child);
        }
    }

    let mut child_tags = parent_tags.clone();
    child_tags.insert(name.clone());
    if is_heading(&name) || name == "td" || name == "th" {
        child_tags.insert("_inline".into());
    }
    if matches!(name.as_str(), "pre" | "code" | "kbd" | "samp") {
        child_tags.insert("_noformat".into());
    }

    let mut child_strings: Vec<String> = children_to_convert
        .iter()
        .map(|&c| process_tag(dom, c, &child_tags))
        .collect();
    child_strings.retain(|s| !s.is_empty());

    // Collapse newlines at child element boundaries, except inside <pre>.
    if !(name == "pre" || dom.find_parent(idx, "pre")) {
        child_strings = collapse_child_newlines(child_strings);
    }

    let text = child_strings.join("");
    apply_convert_fn(dom, idx, &name, text, parent_tags)
}

fn process_text(dom: &Dom, idx: usize, raw: &str, parent_tags: &HashSet<String>) -> String {
    let mut text = raw.to_string();

    // normalize whitespace unless inside a preformatted element
    if !parent_tags.contains("pre") {
        text = normalize_whitespace(&text);
    }
    // escape special characters unless inside preformatted/code elements
    if !parent_tags.contains("_noformat") {
        text = escape_md(&text);
    }

    let prev = dom.prev_sibling(idx);
    let next = dom.next_sibling(idx);
    let parent_name = dom.nodes[idx].parent.and_then(|p| dom.name(p).map(String::from));

    if should_remove_whitespace_outside_node(dom, prev)
        || (should_remove_whitespace_inside_name(parent_name.as_deref()) && prev.is_none())
    {
        text = text
            .trim_start_matches([' ', '\t', '\r', '\n'])
            .to_string();
    }
    if should_remove_whitespace_outside_node(dom, next)
        || (should_remove_whitespace_inside_name(parent_name.as_deref()) && next.is_none())
    {
        text = text.trim_end().to_string();
    }
    text
}

/// whitespace runs containing a newline -> "\n"; runs of spaces/tabs -> " "
fn normalize_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if matches!(c, ' ' | '\t' | '\r' | '\n') {
            let mut has_newline = matches!(c, '\r' | '\n');
            while let Some(&n) = chars.peek() {
                if matches!(n, ' ' | '\t' | '\r' | '\n') {
                    if matches!(n, '\r' | '\n') {
                        has_newline = true;
                    }
                    chars.next();
                } else {
                    break;
                }
            }
            out.push(if has_newline { '\n' } else { ' ' });
        } else {
            out.push(c);
        }
    }
    out
}

fn escape_md(text: &str) -> String {
    text.replace('*', "\\*").replace('_', "\\_")
}

/// Port of the re_extract_newlines-based child boundary collapsing.
fn collapse_child_newlines(strings: Vec<String>) -> Vec<String> {
    fn split_parts(s: &str) -> (usize, &str, usize) {
        let lead = s.chars().take_while(|&c| c == '\n').count();
        let trail = s.chars().rev().take_while(|&c| c == '\n').count();
        (lead, &s[lead..s.len() - trail], trail)
    }

    let mut out: Vec<String> = vec![String::new()];
    for cs in strings {
        let (lead, content, trail) = split_parts(&cs);
        if !out.last().unwrap().is_empty() && lead > 0 {
            let prev = out.pop().unwrap();
            let n = std::cmp::min(2, std::cmp::max(prev.len(), lead));
            out.push("\n".repeat(n));
            out.push(content.to_string());
            out.push("\n".repeat(trail));
        } else {
            out.push("\n".repeat(lead));
            out.push(content.to_string());
            out.push("\n".repeat(trail));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Convert functions
// ---------------------------------------------------------------------------

fn chomp(text: &str) -> (String, String, String) {
    let prefix = if text.starts_with(' ') { " ".to_string() } else { String::new() };
    let suffix = if text.ends_with(' ') { " ".to_string() } else { String::new() };
    (prefix, suffix, text.trim().to_string())
}

/// abstract_inline_conversion — base inline converters get real parent_tags.
fn abstract_inline(text: &str, markup: &str, parent_tags: &HashSet<String>) -> String {
    if parent_tags.contains("_noformat") {
        return text.to_string();
    }
    let markup_suffix = if markup.starts_with('<') && markup.ends_with('>') {
        format!("</{}", &markup[1..])
    } else {
        markup.to_string()
    };
    let (prefix, suffix, text) = chomp(text);
    if text.is_empty() {
        return String::new();
    }
    format!("{}{}{}{}{}", prefix, markup, text, markup_suffix, suffix)
}

fn apply_convert_fn(
    dom: &Dom,
    idx: usize,
    name: &str,
    text: String,
    parent_tags: &HashSet<String>,
) -> String {
    match name {
        "script" | "style" => String::new(),
        "a" => convert_a(dom, idx, &text, parent_tags),
        "b" | "strong" => abstract_inline(&text, "**", parent_tags),
        "em" | "i" => abstract_inline(&text, "*", parent_tags),
        "del" | "s" | "strike" => abstract_inline(&text, "~~", parent_tags),
        "sub" => abstract_inline(&text, "", parent_tags),
        "sup" => abstract_inline(&text, "", parent_tags),
        "blockquote" => convert_blockquote(&text, parent_tags),
        "br" => convert_br(&text, parent_tags),
        "code" | "kbd" | "samp" => convert_code(&text, parent_tags),
        "div" | "article" | "section" | "dl" => convert_div(&text, parent_tags),
        "dd" => convert_dd(&text, parent_tags),
        "dt" => convert_dt(&text, parent_tags),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let n: usize = name[1..].parse().unwrap_or(1);
            convert_hn(n, &text, parent_tags)
        }
        "hr" => "\n\n---\n\n".to_string(),
        "img" => convert_img(dom, idx, parent_tags),
        "input" => convert_input(dom, idx),
        "li" => convert_li(dom, idx, &text, parent_tags),
        "ul" | "ol" => convert_list(dom, idx, &text, parent_tags),
        "p" => convert_p(&text, parent_tags),
        "pre" => convert_pre(&text),
        "q" => format!("\"{}\"", text),
        "table" => format!("\n\n{}\n\n", text.trim()),
        "caption" => format!("{}\n\n", text.trim()),
        "figcaption" => format!("\n\n{}\n\n", text.trim()),
        "td" | "th" => convert_td(dom, idx, &text),
        "tr" => convert_tr(dom, idx, &text),
        "u" => convert_u(&text),
        "video" => convert_video(dom, idx, &text, parent_tags),
        _ => text,
    }
}

// --- custom converters (convert_as_inline effectively always False) --------

fn convert_a(dom: &Dom, idx: usize, text: &str, _parent_tags: &HashSet<String>) -> String {
    if dom.find_parent(idx, "pre") {
        return text.to_string();
    }
    let (prefix, suffix, text) = chomp(text);
    if text.is_empty() {
        return String::new();
    }
    let href_raw = dom.attr(idx, "href").map(String::from);
    let title = dom.attr(idx, "title").map(String::from);
    let href_truthy = href_raw.as_deref().map(|h| !h.is_empty()).unwrap_or(false);

    let mut href = href_raw.clone().unwrap_or_default();
    if href_truthy {
        let h = href_raw.as_deref().unwrap();
        let parts = parse_url(h);
        if let Some(scheme) = &parts.scheme {
            let lower = scheme.to_lowercase();
            if !matches!(lower.as_str(), "http" | "https" | "file") {
                return format!("{}{}{}", prefix, text, suffix);
            }
        }
        href = rebuild_url_with_quoted_path(&parts);
    }

    // autolinks shortcut (default_title = False)
    if text.replace("\\_", "_") == href && title.is_none() {
        return format!("<{}>", href);
    }
    let title_part = title
        .as_ref()
        .map(|t| format!(" \"{}\"", t.replace('"', "\\\"")))
        .unwrap_or_default();
    if href_truthy {
        format!("{}[{}]({}{}){}", prefix, text, href, title_part, suffix)
    } else {
        text
    }
}

fn convert_img(dom: &Dom, idx: usize, _parent_tags: &HashSet<String>) -> String {
    let alt = dom.attr(idx, "alt").unwrap_or("").replace('\n', " ");
    let src = dom
        .attr(idx, "src")
        .map(String::from)
        .or_else(|| dom.attr(idx, "data-src").map(String::from))
        .unwrap_or_default();
    let title = dom.attr(idx, "title").unwrap_or("");
    let title_part = if !title.is_empty() {
        format!(" \"{}\"", title.replace('"', "\\\""))
    } else {
        String::new()
    };
    // keep_inline_images_in = [] → inline context always reduces to alt
    if _parent_tags.contains("_inline") {
        return alt;
    }
    let src = if src.len() >= 5 && src[..5].to_lowercase() == "data:" {
        format!("{}...", src.split(',').next().unwrap_or(""))
    } else {
        src
    };
    format!("![{}]({}{})", alt, src, title_part)
}

fn convert_input(dom: &Dom, idx: usize) -> String {
    if dom.attr(idx, "type") == Some("checkbox") {
        if dom.has_attr(idx, "checked") {
            "[x] ".to_string()
        } else {
            "[ ] ".to_string()
        }
    } else {
        String::new()
    }
}

fn convert_u(text: &str) -> String {
    let (prefix, suffix, text) = chomp(text);
    if text.is_empty() {
        return String::new();
    }
    format!("{}<u>{}</u>{}", prefix, text, suffix)
}

// --- base converters (receive real parent_tags) ----------------------------

fn convert_blockquote(text: &str, parent_tags: &HashSet<String>) -> String {
    let text = text.trim_matches([' ', '\t', '\r', '\n']);
    if parent_tags.contains("_inline") {
        return format!(" {} ", text);
    }
    if text.is_empty() {
        return "\n".to_string();
    }
    let indented = text
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                ">".to_string()
            } else {
                format!("> {}", line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("\n{}\n\n", indented)
}

fn convert_br(text: &str, parent_tags: &HashSet<String>) -> String {
    if parent_tags.contains("_inline") {
        return if text.is_empty() { " ".to_string() } else { format!("{} ", text) };
    }
    format!("  \n{}", text) // newline_style = SPACES
}

fn convert_code(text: &str, parent_tags: &HashSet<String>) -> String {
    if parent_tags.contains("_noformat") {
        return text.to_string();
    }
    let (prefix, suffix, text) = chomp(text);
    if text.is_empty() {
        return String::new();
    }
    let max_backticks = text
        .split('`')
        .map(|seg| seg.len()) // not used; see run computation below
        .count();
    let _ = max_backticks;
    // longest run of consecutive backticks
    let mut max_run = 0usize;
    let mut run = 0usize;
    for c in text.chars() {
        if c == '`' {
            run += 1;
            max_run = max_run.max(run);
        } else {
            run = 0;
        }
    }
    let delimiter = "`".repeat(max_run + 1);
    let text = if max_run > 0 {
        format!(" {} ", text)
    } else {
        text
    };
    format!("{}{}{}{}{}", prefix, delimiter, text, delimiter, suffix)
}

fn convert_div(text: &str, parent_tags: &HashSet<String>) -> String {
    if parent_tags.contains("_inline") {
        return format!(" {} ", text.trim());
    }
    let text = text.trim();
    if text.is_empty() {
        String::new()
    } else {
        format!("\n\n{}\n\n", text)
    }
}

fn convert_dd(text: &str, parent_tags: &HashSet<String>) -> String {
    let text = text.trim();
    if parent_tags.contains("_inline") {
        return format!(" {} ", text);
    }
    if text.is_empty() {
        return "\n".to_string();
    }
    let indented = text
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("    {}", line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(":{}\n", &indented[1..])
}

fn convert_dt(text: &str, parent_tags: &HashSet<String>) -> String {
    let text = normalize_all(text).trim().to_string();
    if parent_tags.contains("_inline") {
        return format!(" {} ", text);
    }
    if text.is_empty() {
        return "\n".to_string();
    }
    format!("\n\n{}\n", text)
}

fn normalize_all(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if matches!(c, ' ' | '\t' | '\r' | '\n') {
            while let Some(&n) = chars.peek() {
                if matches!(n, ' ' | '\t' | '\r' | '\n') {
                    chars.next();
                } else {
                    break;
                }
            }
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

fn convert_hn(n: usize, text: &str, parent_tags: &HashSet<String>) -> String {
    if parent_tags.contains("_inline") {
        return text.to_string();
    }
    let n = n.clamp(1, 6);
    let text = normalize_all(text).trim().to_string();
    format!("\n\n{} {}\n\n", "#".repeat(n), text) // heading_style = ATX
}

fn convert_li(dom: &Dom, idx: usize, text: &str, parent_tags: &HashSet<String>) -> String {
    let text = text.trim();
    if text.is_empty() {
        return "\n".to_string();
    }
    let parent = dom.nodes[idx].parent;
    let parent_name = parent.and_then(|p| dom.name(p).map(String::from));
    let bullet = if parent_name.as_deref() == Some("ol") {
        let start = parent
            .and_then(|p| dom.attr(p, "start"))
            .filter(|s| !s.is_empty() && s.chars().all(|c: char| c.is_numeric()))
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(1);
        format!("{}.", start + dom.count_prev_siblings_named(idx, "li"))
    } else {
        // depth = number of ancestor <ul> elements including the direct parent
        let mut depth: i32 = -1;
        let mut cur = Some(idx);
        while let Some(c) = cur {
            if dom.name(c) == Some("ul") {
                depth += 1;
            }
            cur = dom.nodes[c].parent;
        }
        let depth = depth.max(0) as usize;
        BULLETS[depth % BULLETS.len()].to_string()
    };
    let bullet_str = format!("{} ", bullet);
    let bullet_width = bullet_str.len();
    let bullet_indent = " ".repeat(bullet_width);

    let indented: String = text
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{}{}", bullet_indent, line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{}{}\n", bullet_str, &indented[bullet_width..])
}

fn convert_list(dom: &Dom, idx: usize, text: &str, parent_tags: &HashSet<String>) -> String {
    let next = next_block_content_sibling(dom, idx);
    let before_paragraph = match next {
        Some(n) => {
            let n_name = dom.name(n);
            n_name != Some("ul") && n_name != Some("ol")
        }
        None => false,
    };
    if parent_tags.contains("li") {
        return format!("\n{}", text.trim_end());
    }
    format!(
        "\n\n{}{}",
        text,
        if before_paragraph { "\n" } else { "" }
    )
}

fn convert_p(text: &str, parent_tags: &HashSet<String>) -> String {
    if parent_tags.contains("_inline") {
        return format!(" {} ", text.trim_matches([' ', '\t', '\r', '\n']));
    }
    let text = text.trim_matches([' ', '\t', '\r', '\n']);
    if text.is_empty() {
        String::new()
    } else {
        format!("\n\n{}\n\n", text)
    }
}

fn convert_pre(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let text = strip_pre(text);
    format!("\n\n```\n{}\n```\n\n", text)
}

/// ^[ \n]*\n removed from the start, \n[ \n]*$ removed from the end.
fn strip_pre(text: &str) -> String {
    let mut t = text;
    let run_end = t
        .find(|c: char| c != ' ' && c != '\n')
        .unwrap_or(t.len());
    if let Some(last_nl) = t[..run_end].rfind('\n') {
        t = &t[last_nl + 1..];
    }
    let trimmed = t.trim_end_matches([' ', '\n']);
    if trimmed.len() < t.len() && t.as_bytes()[trimmed.len()] == b'\n' {
        t = trimmed;
    }
    t.to_string()
}

fn convert_td(dom: &Dom, idx: usize, text: &str) -> String {
    let colspan = dom
        .attr(idx, "colspan")
        .and_then(|c| c.parse::<usize>().ok())
        .map(|c| c.clamp(1, 1000))
        .unwrap_or(1);
    let cell = format!(" {}", text.trim().replace('\n', " "));
    format!("{}{}", cell, " |".repeat(colspan))
}

fn convert_tr(dom: &Dom, idx: usize, text: &str) -> String {
    let cells = dom.find_all(idx, &["td", "th"]);
    // bs4 find_previous_sibling() matches Tags only — whitespace text nodes
    // between rows do not count
    let is_first_row = dom.prev_element_sibling(idx).is_none();
    let parent = dom.nodes[idx].parent;
    let parent_name = parent.and_then(|p| dom.name(p).map(String::from));
    let is_headrow = (!cells.is_empty()
        && cells
            .iter()
            .all(|&c| dom.name(c) == Some("th")))
        || (parent_name.as_deref() == Some("thead")
            && parent
                .map(|p| dom.find_all(p, &["tr"]).len() == 1)
                .unwrap_or(false));
    let grand = parent.and_then(|p| dom.nodes[p].parent);
    let is_head_row_missing = (is_first_row && parent_name.as_deref() != Some("tbody"))
        || (is_first_row
            && parent_name.as_deref() == Some("tbody")
            && grand
                .map(|g| dom.find_all(g, &["thead"]).is_empty())
                .unwrap_or(true));
    let mut full_colspan = 0usize;
    for &cell in &cells {
        let cs = dom
            .attr(cell, "colspan")
            .and_then(|c| c.parse::<usize>().ok())
            .map(|c| c.clamp(1, 1000))
            .unwrap_or(1);
        full_colspan += cs;
    }
    let mut overline = String::new();
    let mut underline = String::new();
    if is_headrow && is_first_row {
        underline = format!("| {} |\n", vec!["---"; full_colspan].join(" | "));
    } else if is_head_row_missing
        || (is_first_row
            && (parent_name.as_deref() == Some("table")
                || (parent_name.as_deref() == Some("tbody")
                    && parent
                        .and_then(|p| dom.prev_sibling(p))
                        .is_none())))
    {
        overline = format!(
            "| {} |\n| {} |\n",
            vec![""; full_colspan].join(" | "),
            vec!["---"; full_colspan].join(" | ")
        );
    }
    format!("{}|{}\n{}", overline, text, underline)
}

fn convert_video(dom: &Dom, idx: usize, text: &str, parent_tags: &HashSet<String>) -> String {
    if parent_tags.contains("_inline") {
        return text.to_string();
    }
    let src = dom.attr(idx, "src").map(String::from).unwrap_or_default();
    let src = if src.is_empty() {
        dom.find_all(idx, &["source"])
            .iter()
            .find_map(|&s| dom.attr(s, "src").map(String::from))
            .unwrap_or_default()
    } else {
        src
    };
    let poster = dom.attr(idx, "poster").unwrap_or("").to_string();
    if !src.is_empty() && !poster.is_empty() {
        return format!("[![{}]({})]({})", text, poster, src);
    }
    if !src.is_empty() {
        return format!("[{}]({})", text, src);
    }
    if !poster.is_empty() {
        return format!("![{}]({})", text, poster);
    }
    text.to_string()
}

// --- URL handling (urlparse/urlunparse/quote subset) -----------------------

struct UrlParts {
    scheme: Option<String>,
    netloc: String,
    path: String,
    query: String,
    fragment: String,
}

fn parse_url(s: &str) -> UrlParts {
    let mut rest = s;
    let mut scheme = None;
    if let Some(pos) = s.find(':') {
        let candidate = &s[..pos];
        if !candidate.is_empty()
            && candidate.chars().next().unwrap().is_ascii_alphabetic()
            && candidate
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
        {
            scheme = Some(candidate.to_string());
            rest = &s[pos + 1..];
        }
    }
    let (netloc, rest) = if let Some(r) = rest.strip_prefix("//") {
        let end = r
            .find(|c: char| c == '/' || c == '?' || c == '#')
            .unwrap_or(r.len());
        (r[..end].to_string(), &r[end..])
    } else {
        (String::new(), rest)
    };
    let (rest, fragment) = match rest.split_once('#') {
        Some((a, b)) => (a, b.to_string()),
        None => (rest, String::new()),
    };
    let (rest, query) = match rest.split_once('?') {
        Some((a, b)) => (a, b.to_string()),
        None => (rest, String::new()),
    };
    let path = rest.to_string();
    UrlParts {
        scheme,
        netloc,
        path,
        query,
        fragment,
    }
}

fn rebuild_url_with_quoted_path(parts: &UrlParts) -> String {
    let mut out = String::new();
    if let Some(scheme) = &parts.scheme {
        out.push_str(scheme);
        out.push(':');
    }
    if !parts.netloc.is_empty() {
        out.push_str("//");
        out.push_str(&parts.netloc);
    }
    out.push_str(&quote_path_preserving_percent_encoded(&parts.path));
    if !parts.query.is_empty() {
        out.push('?');
        out.push_str(&parts.query);
    }
    if !parts.fragment.is_empty() {
        out.push('#');
        out.push_str(&parts.fragment);
    }
    out
}

fn quote_path_preserving_percent_encoded(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let bytes = path.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        // preserve existing %HH octets
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            out.push_str(&path[i..i + 3]);
            i += 3;
        } else {
            let c = path[i..].chars().next().unwrap();
            push_quoted_char(&mut out, c);
            i += c.len_utf8();
        }
    }
    out
}

fn push_quoted_char(out: &mut String, c: char) {
    // urllib.parse.quote(s, safe='/'): unreserved = letters digits _.~- plus '/'
    if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '~' | '/') {
        out.push(c);
    } else {
        let mut buf = [0u8; 4];
        for b in c.encode_utf8(&mut buf).as_bytes() {
            out.push_str(&format!("%{:02X}", b));
        }
    }
}

// --- sibling helpers --------------------------------------------------------

/// Port of `_next_block_content_sibling`: next sibling that is a Tag or
/// non-whitespace text (comments/doctype skipped).
fn next_block_content_sibling(dom: &Dom, idx: usize) -> Option<usize> {
    let parent = dom.nodes[idx].parent?;
    let siblings = &dom.nodes[parent].children;
    let pos = siblings.iter().position(|&c| c == idx)?;
    for &s in &siblings[pos + 1..] {
        match &dom.nodes[s].kind {
            NodeKind::Element { .. } => return Some(s),
            NodeKind::Text(t) => {
                if !t.trim().is_empty() {
                    return Some(s);
                }
            }
            NodeKind::Comment | NodeKind::Document => {}
        }
    }
    None
}
