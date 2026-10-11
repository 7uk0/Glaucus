//! A small Fluent (`.ftl`) reader, enough for SS14's locale files.
//!
//! Supports messages, terms, attributes (`.desc`), multi-line values,
//! variable / term / string-literal placeables, select expressions and the
//! handful of SS14 helper functions that matter for plain text
//! (`CAPITALIZE`, `THE`, `SUBJECT`, ...). Unknown functions pass their first
//! argument through, so output degrades gracefully instead of failing.

use std::collections::HashMap;

use crate::{ResPath, Resources};

#[derive(Debug, Clone, Default)]
pub struct Message {
    pub value: Option<String>,
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct Locale {
    messages: HashMap<String, Message>,
    terms: HashMap<String, Message>,
}

impl Locale {
    /// Load every `.ftl` under `/Locale/<culture>/`.
    pub fn load(res: &Resources, culture: &str) -> Locale {
        let mut loc = Locale::default();
        for path in res.find_files(&ResPath::new(format!("/Locale/{culture}")), "ftl") {
            if let Ok(text) = res.read_string(&path) {
                loc.add_source(&text);
            }
        }
        loc
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn add_source(&mut self, text: &str) {
        let mut current: Option<(bool, String)> = None; // (is_term, id)
        let mut current_attr: Option<String> = None;

        for raw in text.lines() {
            let line = raw.trim_end_matches('\r');
            if line.trim().is_empty() {
                // Blank lines may appear inside multi-line patterns; keep going.
                continue;
            }
            // A closing brace at column 0 still belongs to the open pattern.
            let indented = line.starts_with(' ') || line.starts_with('\t') || line.starts_with('}');
            if !indented {
                current_attr = None;
                if line.starts_with('#') {
                    current = None;
                    continue;
                }
                let Some(eq) = line.find('=') else {
                    current = None;
                    continue;
                };
                let id = line[..eq].trim();
                let value = line[eq + 1..].trim();
                let (is_term, id) = match id.strip_prefix('-') {
                    Some(t) => (true, t.to_string()),
                    None => (false, id.to_string()),
                };
                if id.is_empty() || !id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
                    current = None;
                    continue;
                }
                let map = if is_term { &mut self.terms } else { &mut self.messages };
                map.insert(
                    id.clone(),
                    Message {
                        value: (!value.is_empty()).then(|| value.to_string()),
                        attributes: HashMap::new(),
                    },
                );
                current = Some((is_term, id));
                continue;
            }

            let Some((is_term, id)) = &current else { continue };
            let map = if *is_term { &mut self.terms } else { &mut self.messages };
            let Some(msg) = map.get_mut(id) else { continue };
            let t = line.trim();
            if let Some(rest) = t.strip_prefix('.')
                && let Some(eq) = rest.find('=')
                && rest[..eq].trim().chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
            {
                let name = rest[..eq].trim().to_string();
                msg.attributes.insert(name.clone(), rest[eq + 1..].trim().to_string());
                current_attr = Some(name);
                continue;
            }
            let target = match &current_attr {
                Some(a) => msg.attributes.get_mut(a),
                None => {
                    if msg.value.is_none() {
                        msg.value = Some(String::new());
                    }
                    msg.value.as_mut()
                }
            };
            if let Some(s) = target {
                if !s.is_empty() {
                    s.push('\n');
                }
                s.push_str(t);
            }
        }
    }

    pub fn has(&self, id: &str) -> bool {
        self.messages.contains_key(id)
    }

    pub fn message(&self, id: &str) -> Option<&Message> {
        self.messages.get(id)
    }

    /// Format a message value. Missing messages return the id, like Robust.
    pub fn get(&self, id: &str, args: &[(&str, &str)]) -> String {
        match self.messages.get(id).and_then(|m| m.value.as_deref()) {
            Some(p) => self.format(p, args),
            None => id.to_string(),
        }
    }

    pub fn try_get(&self, id: &str, args: &[(&str, &str)]) -> Option<String> {
        self.messages
            .get(id)
            .and_then(|m| m.value.as_deref())
            .map(|p| self.format(p, args))
    }

    pub fn get_attr(&self, id: &str, attr: &str, args: &[(&str, &str)]) -> Option<String> {
        self.messages
            .get(id)
            .and_then(|m| m.attributes.get(attr))
            .map(|p| self.format(p, args))
    }

    /// Format an arbitrary pattern string.
    pub fn format(&self, pattern: &str, args: &[(&str, &str)]) -> String {
        let mut out = String::new();
        let chars: Vec<char> = pattern.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '{' {
                let end = matching_brace(&chars, i);
                let inner: String = chars[i + 1..end].iter().collect();
                out.push_str(&self.eval_placeable(&inner, args));
                i = end + 1;
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out
    }

    fn eval_placeable(&self, inner: &str, args: &[(&str, &str)]) -> String {
        if let Some(arrow) = find_top_level(inner, "->") {
            let selector = self.eval_expr(inner[..arrow].trim(), args);
            return self.eval_select(&inner[arrow + 2..], &selector, args);
        }
        self.eval_expr(inner.trim(), args)
    }

    fn eval_select(&self, body: &str, selector: &str, args: &[(&str, &str)]) -> String {
        // Variants look like `[key] pattern` or `*[key] pattern`.
        let mut variants: Vec<(String, bool, String)> = Vec::new();
        let chars: Vec<char> = body.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let default = chars[i] == '*' && chars.get(i + 1) == Some(&'[');
            if chars[i] == '[' || default {
                let open = if default { i + 1 } else { i };
                let Some(close) = chars[open..].iter().position(|&c| c == ']').map(|p| p + open) else {
                    break;
                };
                let key: String = chars[open + 1..close].iter().collect();
                // Pattern runs until next variant start at top level.
                let mut j = close + 1;
                let mut depth = 0;
                while j < chars.len() {
                    match chars[j] {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        '[' | '*' if depth == 0 && is_variant_start(&chars, j) => break,
                        _ => {}
                    }
                    j += 1;
                }
                let pat: String = chars[close + 1..j].iter().collect();
                variants.push((key.trim().to_string(), default, pat.trim().to_string()));
                i = j;
            } else {
                i += 1;
            }
        }
        let chosen = variants
            .iter()
            .find(|(k, _, _)| k == selector)
            .or_else(|| plural_match(&variants, selector))
            .or_else(|| variants.iter().find(|(_, d, _)| *d))
            .or_else(|| variants.first());
        chosen.map(|(_, _, p)| self.format(p, args)).unwrap_or_default()
    }

    fn eval_expr(&self, expr: &str, args: &[(&str, &str)]) -> String {
        let expr = expr.trim();
        if let Some(var) = expr.strip_prefix('$') {
            let var = var.trim();
            return args
                .iter()
                .find(|(k, _)| *k == var)
                .map(|(_, v)| v.to_string())
                .unwrap_or_default();
        }
        if let Some(term) = expr.strip_prefix('-') {
            let term = term.split(['(', '.', ' ']).next().unwrap_or("");
            return self
                .terms
                .get(term)
                .and_then(|m| m.value.as_deref())
                .map(|p| self.format(p, args))
                .unwrap_or_default();
        }
        if expr.starts_with('"') && expr.ends_with('"') && expr.len() >= 2 {
            return expr[1..expr.len() - 1].to_string();
        }
        if let Some(paren) = expr.find('(')
            && expr.ends_with(')')
        {
            let func = &expr[..paren];
            let inner = &expr[paren + 1..expr.len() - 1];
            let first = split_top_level_args(inner).into_iter().next().unwrap_or_default();
            let val = self.eval_expr(&first, args);
            return match func {
                "CAPITALIZE" => capitalize(&val),
                "THE" | "SUBJECT" | "OBJECT" | "POSS-ADJ" | "POSS-PRONOUN" | "REFLEXIVE" | "PROPER"
                | "NUMBER" | "LOC" | "INDEFINITE" | "CONJUGATE-BE" | "CONJUGATE-HAVE"
                | "CONJUGATE-BASIC" | "GENDER" | "TOSTRING" | "NATURALFIXED" | "NATURALPERCENT" => val,
                _ => val,
            };
        }
        // Message reference.
        if let Some(m) = self.messages.get(expr).and_then(|m| m.value.as_deref()) {
            return self.format(m, args);
        }
        expr.to_string()
    }
}

fn is_variant_start(chars: &[char], j: usize) -> bool {
    // A variant key must be preceded by a newline/whitespace run.
    let prev_ws = j == 0 || chars[..j].iter().rev().take_while(|c| **c == ' ' || **c == '\t').count() > 0
        || chars[j - 1] == '\n';
    let starts = chars[j] == '[' || (chars[j] == '*' && chars.get(j + 1) == Some(&'['));
    starts && prev_ws && {
        let open = if chars[j] == '*' { j + 1 } else { j };
        chars[open..].iter().position(|&c| c == ']').is_some()
    }
}

fn plural_match<'a>(variants: &'a [(String, bool, String)], selector: &str) -> Option<&'a (String, bool, String)> {
    let n: f64 = selector.parse().ok()?;
    let cat = if n == 1.0 { "one" } else { "other" };
    variants.iter().find(|(k, _, _)| k == cat)
}

fn matching_brace(chars: &[char], open: usize) -> usize {
    let mut depth = 0;
    for (i, &c) in chars.iter().enumerate().skip(open) {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    chars.len().saturating_sub(1).max(open)
}

fn find_top_level(s: &str, pat: &str) -> Option<usize> {
    let mut depth = 0i32;
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        match bytes[i] {
            b'{' | b'(' => depth += 1,
            b'}' | b')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && s[i..].starts_with(pat) {
            return Some(i);
        }
    }
    None
}

fn split_top_level_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '(' | '{' => {
                depth += 1;
                cur.push(c)
            }
            ')' | '}' => {
                depth -= 1;
                cur.push(c)
            }
            ',' if depth == 0 => out.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"
# comment
ent-MobCarp = space carp
    .desc = It's a space carp.
    .suffix = Hostile
-brand = Nanotrasen
greet = Hello { $name }, welcome to { -brand }!
popup =
    { $setting ->
        [combined] Combined
        [total] Total
       *[delta] Delta
    }
bananas = { $count ->
    [one] one banana
   *[other] { $count } bananas
}
cap = { CAPITALIZE($thing) } breaks.
"#;

    #[test]
    fn formats_messages() {
        let mut l = Locale::default();
        l.add_source(SRC);
        assert_eq!(l.get("ent-MobCarp", &[]), "space carp");
        assert_eq!(l.get_attr("ent-MobCarp", "desc", &[]).unwrap(), "It's a space carp.");
        assert_eq!(l.get("greet", &[("name", "Zuko")]), "Hello Zuko, welcome to Nanotrasen!");
        assert_eq!(l.get("popup", &[("setting", "total")]), "Total");
        assert_eq!(l.get("popup", &[("setting", "nope")]), "Delta");
        assert_eq!(l.get("bananas", &[("count", "1")]), "one banana");
        assert_eq!(l.get("bananas", &[("count", "3")]), "3 bananas");
        assert_eq!(l.get("cap", &[("thing", "the airlock")]), "The airlock breaks.");
        assert_eq!(l.get("missing-id", &[]), "missing-id");
    }
}
