//! OMML (Office Math Markup Language) → LaTeX — port of upstream
//! `converter_utils/docx/math/omml.py` + `latex_dict.py`.

use std::collections::HashMap;

pub const M_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";

const FUNC_PLACE: &str = "{fe}";
const ESCAPE_CHARS: [char; 9] = ['{', '}', '_', '^', '#', '&', '$', '%', '~'];

fn is_math_el(node: roxmltree::Node, local: &str) -> bool {
    node.tag_name().namespace() == Some(M_NS) && node.tag_name().name() == local
}

fn local_name<'a>(node: roxmltree::Node<'a, 'a>) -> Option<&'a str> {
    if node.tag_name().namespace() == Some(M_NS) {
        Some(node.tag_name().name())
    } else {
        None
    }
}

/// Port of `escape_latex`: collapse doubled backslashes, then escape the
/// special Markdown/LaTeX characters unless already preceded by a backslash.
fn escape_latex(s: &str) -> String {
    let s = s.replace("\\\\", "\\");
    let mut out = String::with_capacity(s.len());
    let mut last_was_backslash = false;
    for c in s.chars() {
        if ESCAPE_CHARS.contains(&c) && !last_was_backslash {
            out.push('\\');
        }
        out.push(c);
        last_was_backslash = c == '\\';
    }
    out
}

/// Port of `T`: math-italic unicode → LaTeX/plain equivalents.
fn t_char(c: char) -> Option<&'static str> {
    // Greek letters (math italic block U+1D6FC..U+1D71B)
    const GREEK: [&str; 32] = [
        "\\alpha ", "\\beta ", "\\gamma ", "\\delta ", "\\epsilon ", "\\zeta ",
        "\\eta ", "\\theta ", "\\iota ", "\\kappa ", "\\lambda ", "\\mu ",
        "\\nu ", "\\xi ", "\\omicron ", "\\pi ", "\\rho ", "\\varsigma ",
        "\\sigma ", "\\tau ", "\\upsilon ", "\\phi ", "\\chi ", "\\psi ",
        "\\omega ", "\\partial ", "\\varepsilon ", "\\vartheta ", "\\varkappa ",
        "\\varphi ", "\\varrho ", "\\varpi ",
    ];
    let u = c as u32;
    if (0x1D6FC..=0x1D71B).contains(&u) {
        return Some(GREEK[(u - 0x1D6FC) as usize]);
    }
    Some(match c {
        '←' => "\\leftarrow ",
        '↑' => "\\uparrow ",
        '→' => "\\rightarrow ",
        '↓' => "\\downarrow ",
        '↔' => "\\leftrightarrow ",
        '↕' => "\\updownarrow ",
        '↖' => "\\nwarrow ",
        '↗' => "\\nearrow ",
        '↘' => "\\searrow ",
        '↙' => "\\swarrow ",
        '⋮' => "\\vdots ",
        '⋯' => "\\cdots ",
        '⋰' => "\\adots ",
        '⋱' => "\\ddots ",
        '≠' => "\\ne ",
        '≤' => "\\leq ",
        '≥' => "\\geq ",
        '≦' => "\\leqq ",
        '≧' => "\\geqq ",
        '≨' => "\\lneqq ",
        '≩' => "\\gneqq ",
        '≪' => "\\ll ",
        '≫' => "\\gg ",
        '∈' => "\\in ",
        '∉' => "\\notin ",
        '∋' => "\\ni ",
        '∌' => "\\nni ",
        '∞' => "\\infty ",
        '±' => "\\pm ",
        '∓' => "\\mp ",
        _ => return italic_letter(c),
    })
}

/// Math-italic Latin letters (U+1D434.. block; U+210E is the planck h).
fn italic_letter(c: char) -> Option<&'static str> {
    const ITALIC: [(u32, char); 7] = [
        (0x1D44E, 'a'),
        (0x1D44F, 'b'),
        (0x1D450, 'c'),
        (0x1D451, 'd'),
        (0x1D452, 'e'),
        (0x1D453, 'f'),
        (0x1D454, 'g'),
    ];
    const ITALIC2: [(u32, char); 18] = [
        (0x1D456, 'i'),
        (0x1D457, 'j'),
        (0x1D458, 'k'),
        (0x1D459, 'l'),
        (0x1D45A, 'm'),
        (0x1D45B, 'n'),
        (0x1D45C, 'o'),
        (0x1D45D, 'p'),
        (0x1D45E, 'q'),
        (0x1D45F, 'r'),
        (0x1D460, 's'),
        (0x1D461, 't'),
        (0x1D462, 'u'),
        (0x1D463, 'v'),
        (0x1D464, 'w'),
        (0x1D465, 'x'),
        (0x1D466, 'y'),
        (0x1D467, 'z'),
    ];
    let u = c as u32;
    if (0x1D434..=0x1D44D).contains(&u) {
        let letter = (b'A' + (u - 0x1D434) as u8) as char;
        return Some(Box::leak(letter.to_string().into_boxed_str()));
    }
    if u == 0x210E {
        return Some("h");
    }
    for (code, ch) in ITALIC {
        if u == code {
            // static promotion of a single ASCII char
            return Some(Box::leak(ch.to_string().into_boxed_str()));
        }
    }
    for (code, ch) in ITALIC2 {
        if u == code {
            return Some(Box::leak(ch.to_string().into_boxed_str()));
        }
    }
    None
}

/// Port of `FUNC`: known function names.
fn func_latex(name: &str) -> Option<&'static str> {
    Some(match name {
        "sin" => "\\sin({fe})",
        "cos" => "\\cos({fe})",
        "tan" => "\\tan({fe})",
        "arcsin" => "\\arcsin({fe})",
        "arccos" => "\\arccos({fe})",
        "arctan" => "\\arctan({fe})",
        "arccot" => "\\operatorname{arccot}({fe})",
        "sinh" => "\\sinh({fe})",
        "cosh" => "\\cosh({fe})",
        "tanh" => "\\tanh({fe})",
        "coth" => "\\coth({fe})",
        "sec" => "\\sec({fe})",
        "csc" => "\\csc({fe})",
        "log" => "\\log({fe})",
        "ln" => "\\ln({fe})",
        "exp" => "\\exp({fe})",
        "det" => "\\det({fe})",
        "gcd" => "\\gcd({fe})",
        "lcm" => "\\operatorname{lcm}({fe})",
        "lim" => "\\lim({fe})",
        "max" => "\\max({fe})",
        "min" => "\\min({fe})",
        "sup" => "\\sup({fe})",
        "inf" => "\\inf({fe})",
        "dim" => "\\dim({fe})",
        "ker" => "\\ker({fe})",
        "hom" => "\\hom({fe})",
        "deg" => "\\deg({fe})",
        "arg" => "\\arg({fe})",
        _ => return None,
    })
}

/// Accent char (m:accPr/m:chr val) → LaTeX wrapper, port of `CHR`.
fn accent_latex(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{0300}' => "\\grave",
        '\u{0301}' => "\\acute",
        '\u{0302}' => "\\hat",
        '\u{0303}' => "\\tilde",
        '\u{0304}' => "\\bar",
        '\u{0305}' => "\\overbar",
        '\u{0306}' => "\\breve",
        '\u{0307}' => "\\dot",
        '\u{0308}' => "\\ddot",
        '\u{0309}' => "\\ovhook",
        '\u{030a}' => "\\ocirc",
        '\u{030c}' => "\\check",
        '\u{0310}' => "\\candra",
        '\u{0312}' => "\\oturnedcomma",
        '\u{0315}' => "\\ocommatopright",
        '\u{031a}' => "\\droang",
        '\u{0338}' => "\\not",
        '\u{20d0}' => "\\leftharpoonaccent",
        '\u{20d1}' => "\\rightharpoonaccent",
        '\u{20d2}' => "\\vertoverlay",
        '\u{20d6}' => "\\overleftarrow",
        '\u{20d7}' => "\\vec",
        '\u{20db}' => "\\dddot",
        '\u{20dc}' => "\\ddddot",
        '\u{20e1}' => "\\overleftrightarrow",
        '\u{20e7}' => "\\annuity",
        '\u{20e9}' => "\\widebridgeabove",
        '\u{20f0}' => "\\asteraccent",
        '\u{0330}' => "\\wideutilde",
        '\u{0331}' => "\\underbar",
        '\u{20e8}' => "\\threeunderdot",
        '\u{20ec}' => "\\underrightharpoondown",
        '\u{20ed}' => "\\underleftharpoondown",
        '\u{20ee}' => "\\underleftarrow",
        '\u{20ef}' => "\\underrightarrow",
        '\u{23b4}' => "\\overbracket",
        '\u{23dc}' => "\\overparen",
        '\u{23de}' => "\\overbrace",
        '\u{23b5}' => "\\underbracket",
        '\u{23dd}' => "\\underparen",
        '\u{23df}' => "\\underbrace",
        _ => return None,
    })
}

/// Big operator char (naryPr chr val) → LaTeX, port of `CHR_BO`.
fn big_operator(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{2140}' => "\\Bbbsum",
        '\u{220f}' => "\\prod",
        '\u{2210}' => "\\coprod",
        '\u{2211}' => "\\sum",
        '\u{222b}' => "\\int",
        '\u{22c0}' => "\\bigwedge",
        '\u{22c1}' => "\\bigvee",
        '\u{22c2}' => "\\bigcap",
        '\u{22c3}' => "\\bigcup",
        '\u{2a00}' => "\\bigodot",
        '\u{2a01}' => "\\bigoplus",
        '\u{2a02}' => "\\bigotimes",
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Converter
// ---------------------------------------------------------------------------

#[derive(Default, Clone, Debug)]
struct Pr {
    text: String,
    props: HashMap<String, String>,
}

enum Conv {
    Text(String),
    Pr(Pr),
}

fn conv_text(c: &Conv) -> String {
    match c {
        Conv::Text(s) => s.clone(),
        Conv::Pr(p) => p.text.clone(),
    }
}

type ChildList = Vec<(String, Conv)>;

fn process_children_list(node: roxmltree::Node, include: Option<&[&str]>) -> ChildList {
    let mut out = Vec::new();
    for child in node.children() {
        let Some(stag) = local_name(child) else {
            continue;
        };
        if let Some(inc) = include {
            if !inc.contains(&stag) {
                continue;
            }
        }
        if let Some(t) = call_method(child, stag) {
            out.push((stag.to_string(), Conv::Text(t)));
        } else if let Some(c) = process_unknown(child, stag) {
            out.push((stag.to_string(), c));
        }
    }
    out
}

fn process_children(node: roxmltree::Node, include: Option<&[&str]>) -> String {
    process_children_list(node, include)
        .into_iter()
        .map(|(_, c)| conv_text(&c))
        .collect()
}

fn children_dict(node: roxmltree::Node, include: Option<&[&str]>) -> HashMap<String, Conv> {
    let mut dict = HashMap::new();
    for (stag, c) in process_children_list(node, include) {
        dict.insert(stag, c);
    }
    dict
}

fn dict_text(dict: &HashMap<String, Conv>, key: &str) -> String {
    dict.get(key).map(conv_text).unwrap_or_default()
}

fn dict_pr<'a>(dict: &'a HashMap<String, Conv>, key: &str) -> Option<&'a Pr> {
    match dict.get(key) {
        Some(Conv::Pr(p)) => Some(p),
        _ => None,
    }
}

fn process_pr(node: roxmltree::Node) -> Pr {
    let mut pr = Pr::default();
    let mut parts: Vec<String> = Vec::new();
    for child in node.children() {
        let Some(stag) = local_name(child) else {
            continue;
        };
        if stag == "brk" {
            pr.props.insert("brk".into(), "\\".into());
            parts.push("\\".to_string());
        } else if matches!(stag, "chr" | "pos" | "begChr" | "endChr" | "type") {
            if let Some(v) = child.attribute((M_NS, "val")) {
                pr.props.insert(stag.to_string(), v.to_string());
            }
        }
    }
    pr.text = parts.join("");
    pr
}

fn process_unknown(node: roxmltree::Node, stag: &str) -> Option<Conv> {
    const DIRECT_TAGS: [&str; 8] = [
        "box", "sSub", "sSup", "sSubSup", "num", "den", "deg", "e",
    ];
    if DIRECT_TAGS.contains(&stag) {
        Some(Conv::Text(process_children(node, None)))
    } else if stag.ends_with("Pr") {
        Some(Conv::Pr(process_pr(node)))
    } else {
        None
    }
}

/// Public entry: convert an <m:oMath> element to LaTeX.
pub fn omath_to_latex(node: roxmltree::Node) -> String {
    process_children(node, None)
}

/// Port of `oMath2Latex.tag2meth` dispatch.
fn call_method(node: roxmltree::Node, stag: &str) -> Option<String> {
    match stag {
        "r" => Some(do_r(node)),
        "sub" => Some(format!("_{{{}}}", process_children(node, None))),
        "sup" => Some(format!("^{{{}}}", process_children(node, None))),
        "acc" => do_acc(node),
        "bar" => do_bar(node),
        "groupChr" => do_groupchr(node),
        "d" => do_d(node),
        "f" => do_f(node),
        "func" => do_func(node),
        "fName" => do_fname(node),
        "rad" => do_rad(node),
        "eqArr" => do_eqarr(node),
        "limLow" => do_limlow(node),
        "limUpp" => do_limupp(node),
        "lim" => Some(do_lim(node)),
        "m" => do_m(node),
        "mr" => do_mr(node),
        "nary" => do_nary(node),
        _ => None,
    }
}

/// Port of `do_r`: text runs with per-char symbol conversion + latex escaping.
fn do_r(node: roxmltree::Node) -> String {
    let mut converted = String::new();
    let text = node
        .children()
        .find(|&c| is_math_el(c, "t"))
        .and_then(|t| t.text())
        .unwrap_or("");
    for c in text.chars() {
        match t_char(c) {
            Some(sym) => converted.push_str(sym),
            None => converted.push(c),
        }
    }
    escape_latex(&converted)
}

/// Port of `do_acc`: accent over the base element.
fn do_acc(node: roxmltree::Node) -> Option<String> {
    let dict = children_dict(node, None);
    let chr = dict_pr(&dict, "accPr").and_then(|p| p.props.get("chr").cloned());
    let latex_s = chr
        .and_then(|c| accent_latex(c.chars().next()?))
        .unwrap_or("\\hat");
    let e = dict_text(&dict, "e");
    Some(format!("{}{{{}}}", latex_s, e))
}

/// Port of `do_bar`: overline/underline bar.
fn do_bar(node: roxmltree::Node) -> Option<String> {
    let dict = children_dict(node, None);
    let pr = dict_pr(&dict, "barPr")?;
    let pos = pr.props.get("pos").cloned();
    let latex_s = match pos.as_deref() {
        Some("bot") => "\\underline",
        _ => "\\overline", // POS_DEFAULT BAR_VAL
    };
    let e = dict_text(&dict, "e");
    Some(format!("{}{}{{{}}}", pr.text, latex_s, e))
}

/// Port of `do_groupchr`: group-character object.
fn do_groupchr(node: roxmltree::Node) -> Option<String> {
    let dict = children_dict(node, None);
    let pr = dict_pr(&dict, "groupChrPr")?;
    let chr = pr.props.get("chr").cloned();
    let latex_s = chr
        .and_then(|c| accent_latex(c.chars().next()?))
        .unwrap_or("\\underbrace"); // CHR_DEFAULT GROUP_CHR_VAL
    let e = dict_text(&dict, "e");
    Some(format!("{}{}{{{}}}", pr.text, latex_s, e))
}

/// Port of `do_d`: delimiter object with beg/end chars.
fn do_d(node: roxmltree::Node) -> Option<String> {
    let dict = children_dict(node, None);
    let pr = dict_pr(&dict, "dPr")?;
    let beg = pr.props.get("begChr").cloned();
    let end = pr.props.get("endChr").cloned();
    // get_char(None) returns the D_DEFAULT; a present-but-empty val → null "."
    let left = match beg {
        None => "(".to_string(), // D_DEFAULT left
        Some(s) if !s.is_empty() => escape_latex(&apply_t(&s)),
        Some(_) => ".".to_string(),
    };
    let right = match end {
        None => ")".to_string(), // D_DEFAULT right
        Some(s) if !s.is_empty() => escape_latex(&apply_t(&s)),
        Some(_) => ".".to_string(),
    };
    let e = dict_text(&dict, "e");
    Some(format!(
        "{}\\left{}{}\\right{}",
        pr.text, left, e, right
    ))
}

/// T.get(key, key): per-char conversion, keep the char if not in T.
fn apply_t(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match t_char(c) {
            Some(sym) => out.push_str(sym),
            None => out.push(c),
        }
    }
    out
}

/// Port of `do_f`: fraction object.
fn do_f(node: roxmltree::Node) -> Option<String> {
    let dict = children_dict(node, None);
    let pr = dict_pr(&dict, "fPr")?;
    let ftype = pr.props.get("type").cloned();
    let num = dict_text(&dict, "num");
    let den = dict_text(&dict, "den");
    let body = match ftype.as_deref() {
        Some("skw") => format!("^{{{}}}/_{{{}}}", num, den),
        Some("noBar") => format!("\\genfrac{{}}{{}}{{0pt}}{{}}{{{}}}{{{}}}", num, den),
        Some("lin") => format!("{{{}}}/{{{}}}", num, den),
        _ => format!("\\frac{{{}}}{{{}}}", num, den), // "bar" + default
    };
    Some(format!("{}{}", pr.text, body))
}

/// Port of `do_func`: function-apply object.
fn do_func(node: roxmltree::Node) -> Option<String> {
    let dict = children_dict(node, None);
    let func_name = dict_text(&dict, "fName");
    if !func_name.contains(FUNC_PLACE) {
        return None; // Python KeyError → step skipped
    }
    let e = dict_text(&dict, "e");
    Some(func_name.replace(FUNC_PLACE, &e))
}

/// Port of `do_fname`: function name with run-joining.
fn do_fname(node: roxmltree::Node) -> Option<String> {
    let mut latex_chars: Vec<String> = Vec::new();
    let mut name_parts: Vec<String> = Vec::new();

    fn flush_name(name_parts: &mut Vec<String>, latex_chars: &mut Vec<String>) {
        if name_parts.is_empty() {
            return;
        }
        let name = name_parts.join("");
        name_parts.clear();
        let mapped = match func_latex(&name) {
            Some(s) => s.to_string(),
            None => format!("\\operatorname{{{}}}({{fe}})", name),
        };
        latex_chars.push(mapped);
    }

    for (stag, c) in process_children_list(node, None) {
        if stag == "r" {
            name_parts.push(conv_text(&c));
        } else {
            flush_name(&mut name_parts, &mut latex_chars);
            latex_chars.push(conv_text(&c));
        }
    }
    flush_name(&mut name_parts, &mut latex_chars);
    let mut t = latex_chars.join("");
    if !t.contains(FUNC_PLACE) {
        t.push_str(FUNC_PLACE);
    }
    Some(t)
}

/// Port of `do_rad`: radical (sqrt) object.
fn do_rad(node: roxmltree::Node) -> Option<String> {
    let dict = children_dict(node, None);
    let text = dict_text(&dict, "e");
    let deg = dict_text(&dict, "deg");
    if !deg.is_empty() {
        Some(format!("\\sqrt[{}]{{{}}}", deg, text))
    } else {
        Some(format!("\\sqrt{{{}}}", text))
    }
}

/// Port of `do_eqarr`: equation array.
fn do_eqarr(node: roxmltree::Node) -> Option<String> {
    let parts: Vec<String> = process_children_list(node, Some(&["e"]))
        .into_iter()
        .map(|(_, c)| conv_text(&c))
        .collect();
    Some(format!(
        "\\begin{{array}}{{c}}{}\\end{{array}}",
        parts.join("\\\\")
    ))
}

/// Port of `do_limlow`: lower-limit object.
fn do_limlow(node: roxmltree::Node) -> Option<String> {
    let dict = children_dict(node, Some(&["e", "lim"]));
    let e = dict_text(&dict, "e");
    let lim = dict_text(&dict, "lim");
    let latex_s = match e.as_str() {
        "lim" => "\\lim_{{{lim}}}",
        "max" => "\\max_{{{lim}}}",
        "min" => "\\min_{{{lim}}}",
        _ => return None, // Python raises NotImplementedError → skipped
    };
    Some(latex_s.replace("{lim}", &lim))
}

/// Port of `do_limupp`: upper-limit object.
fn do_limupp(node: roxmltree::Node) -> Option<String> {
    let dict = children_dict(node, Some(&["e", "lim"]));
    let e = dict_text(&dict, "e");
    let lim = dict_text(&dict, "lim");
    Some(format!("\\overset{{{}}}{{{}}}", lim, e))
}

/// Port of `do_lim`: limit text with arrow normalization.
fn do_lim(node: roxmltree::Node) -> String {
    process_children(node, None).replace("\\rightarrow", "\\to")
}

/// Port of `do_m`: matrix object.
fn do_m(node: roxmltree::Node) -> Option<String> {
    let mut rows: Vec<String> = Vec::new();
    for (stag, c) in process_children_list(node, None) {
        if stag == "mr" {
            rows.push(conv_text(&c));
        }
    }
    Some(format!(
        "\\begin{{matrix}}{}\\end{{matrix}}",
        rows.join("\\\\")
    ))
}

/// Port of `do_mr`: matrix row (e children joined by &).
fn do_mr(node: roxmltree::Node) -> Option<String> {
    let parts: Vec<String> = process_children_list(node, Some(&["e"]))
        .into_iter()
        .map(|(_, c)| conv_text(&c))
        .collect();
    Some(parts.join("&"))
}

/// Port of `do_nary`: n-ary object (sum/int with limits).
fn do_nary(node: roxmltree::Node) -> Option<String> {
    let mut bo = String::new();
    let mut res: Vec<String> = Vec::new();
    for (stag, c) in process_children_list(node, None) {
        if stag == "naryPr" {
            if let Conv::Pr(pr) = &c {
                if let Some(chr) = pr.props.get("chr") {
                    if let Some(ch) = chr.chars().next() {
                        bo = big_operator(ch).unwrap_or("").to_string();
                    }
                }
            }
        } else {
            res.push(conv_text(&c));
        }
    }
    Some(format!("{}{}", bo, res.join("")))
}
