use crate::language::Language;

/// Indent unit used by every formatter in this module (2 spaces, matching the common
/// default for web-oriented beautifiers such as Postman's "Beautify").
const INDENT_UNIT: &str = "  ";

/// Reformats `source` for `language`, mirroring Postman's "Beautify" body action: JSON is
/// fully parsed and re-serialized, XML/HTML/CSS are reformatted with a lightweight
/// indenting tokenizer, and curly-brace languages get a best-effort reindent of their
/// existing lines. Returns `Err` with a human-readable reason (invalid syntax, or an
/// unsupported language) instead of touching the document.
pub fn format_document(language: Language, source: &str) -> Result<String, String> {
    match language {
        Language::Json => format_json(source),
        Language::Xml => Ok(format_markup(source, true)),
        Language::Html => Ok(format_markup(source, false)),
        Language::Css => Ok(format_css(source)),
        Language::Rust
        | Language::C
        | Language::Cpp
        | Language::CSharp
        | Language::Java
        | Language::JavaScript
        | Language::TypeScript
        | Language::Go
        | Language::Php => Ok(reindent_braces(source)),
        _ => Err(format!(
            "Formatting isn't available for {} yet (supported: JSON, XML, HTML, CSS, and brace-based languages).",
            language.label()
        )),
    }
}

fn format_json(source: &str) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(source).map_err(|e| format!("Invalid JSON - nothing changed: {e}"))?;
    let pretty =
        serde_json::to_string_pretty(&value).map_err(|e| format!("Failed to format JSON: {e}"))?;
    Ok(format!("{pretty}\n"))
}

/// Naive forward substring search over a char slice, used instead of repeatedly
/// collecting tail slices into `String`s so formatting stays roughly linear in document
/// size (each search starts where the previous one ended).
fn find_from(chars: &[char], start: usize, needle: &str) -> Option<usize> {
    let needle_chars: Vec<char> = needle.chars().collect();
    let n = needle_chars.len();
    if n == 0 || start >= chars.len() || chars.len() - start < n {
        return None;
    }
    let limit = chars.len() - n;
    let mut i = start;
    while i <= limit {
        if chars[i..i + n] == needle_chars[..] {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Scans forward from just past `<` at `start - 1`, returning the index of the matching
/// unquoted `>` (so `<a href=">">` doesn't end the tag early on the `>` inside the quotes).
fn find_tag_end(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    let mut quote: Option<char> = None;
    while i < chars.len() {
        let c = chars[i];
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => quote = Some(c),
                '>' => return Some(i),
                _ => {}
            },
        }
        i += 1;
    }
    None
}

/// Shared XML/HTML pretty-printer: walks the markup re-indenting each tag/text node by
/// nesting depth. `<script>`/`<style>` bodies are kept verbatim (HTML only) since
/// reformatting embedded JS/CSS correctly would need a real parser for each.
fn format_markup(source: &str, is_xml: bool) -> String {
    const VOID_ELEMENTS: &[&str] = &[
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
        "source", "track", "wbr",
    ];
    const RAW_TEXT_ELEMENTS: &[&str] = &["script", "style"];

    let chars: Vec<char> = source.chars().collect();
    let lower_chars: Vec<char> = source.to_ascii_lowercase().chars().collect();
    let len = chars.len();
    let mut out = String::new();
    let mut indent: usize = 0;
    let mut i = 0usize;

    let push_line = |out: &mut String, indent: usize, content: &str| {
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            out.push_str(&INDENT_UNIT.repeat(indent));
            out.push_str(trimmed);
            out.push('\n');
        }
    };

    while i < len {
        if chars[i] != '<' {
            let mut j = i;
            while j < len && chars[j] != '<' {
                j += 1;
            }
            let text: String = chars[i..j].iter().collect();
            push_line(&mut out, indent, &text);
            i = j;
            continue;
        }

        // Comment: <!-- ... -->
        if chars.get(i + 1) == Some(&'!')
            && chars.get(i + 2) == Some(&'-')
            && chars.get(i + 3) == Some(&'-')
        {
            match find_from(&chars, i + 4, "-->") {
                Some(close) => {
                    let end = close + 3;
                    let text: String = chars[i..end].iter().collect();
                    push_line(&mut out, indent, &text);
                    i = end;
                }
                None => {
                    let text: String = chars[i..].iter().collect();
                    push_line(&mut out, indent, &text);
                    i = len;
                }
            }
            continue;
        }

        // CDATA: <![CDATA[ ... ]]>
        if find_from(&lower_chars, i, "<![cdata[") == Some(i) {
            match find_from(&chars, i + 9, "]]>") {
                Some(close) => {
                    let end = close + 3;
                    let text: String = chars[i..end].iter().collect();
                    push_line(&mut out, indent, &text);
                    i = end;
                }
                None => {
                    let text: String = chars[i..].iter().collect();
                    push_line(&mut out, indent, &text);
                    i = len;
                }
            }
            continue;
        }

        // Doctype / processing instruction: <! ... > or <? ... ?>
        if chars.get(i + 1) == Some(&'!') || chars.get(i + 1) == Some(&'?') {
            match find_tag_end(&chars, i + 1) {
                Some(end) => {
                    let text: String = chars[i..=end].iter().collect();
                    push_line(&mut out, indent, &text);
                    i = end + 1;
                }
                None => {
                    let text: String = chars[i..].iter().collect();
                    push_line(&mut out, indent, &text);
                    i = len;
                }
            }
            continue;
        }

        // Regular opening/closing/self-closing tag.
        let Some(end) = find_tag_end(&chars, i + 1) else {
            let text: String = chars[i..].iter().collect();
            push_line(&mut out, indent, &text);
            break;
        };

        let inner: String = chars[i + 1..end].iter().collect();
        let trimmed_inner = inner.trim();
        let is_closing = trimmed_inner.starts_with('/');
        let is_self_closing = trimmed_inner.ends_with('/');
        let name_source = trimmed_inner
            .trim_start_matches('/')
            .trim_end_matches('/')
            .trim();
        let tag_name_lower = name_source
            .split(|c: char| c.is_whitespace())
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();

        if is_closing {
            indent = indent.saturating_sub(1);
            push_line(&mut out, indent, &format!("</{tag_name_lower}>"));
            i = end + 1;
            continue;
        }

        push_line(&mut out, indent, &format!("<{trimmed_inner}>"));

        let is_void = !is_xml && VOID_ELEMENTS.contains(&tag_name_lower.as_str());
        if is_self_closing || is_void {
            i = end + 1;
            continue;
        }

        if !is_xml && RAW_TEXT_ELEMENTS.contains(&tag_name_lower.as_str()) {
            let close_marker = format!("</{tag_name_lower}");
            match find_from(&lower_chars, end + 1, &close_marker) {
                Some(close_start) => {
                    let raw: String = chars[end + 1..close_start].iter().collect();
                    if !raw.trim().is_empty() {
                        indent += 1;
                        for line in raw.lines() {
                            if !line.trim().is_empty() {
                                push_line(&mut out, indent, line);
                            }
                        }
                        indent -= 1;
                    }
                    i = close_start;
                }
                None => {
                    let raw: String = chars[end + 1..].iter().collect();
                    indent += 1;
                    for line in raw.lines() {
                        if !line.trim().is_empty() {
                            push_line(&mut out, indent, line);
                        }
                    }
                    i = len;
                }
            }
            continue;
        }

        indent += 1;
        i = end + 1;
    }

    let mut result = out.trim_end().to_string();
    result.push('\n');
    result
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Lightweight CSS pretty-printer: splits on `{`, `}`, `;` (quote- and comment-aware) and
/// re-indents one selector/declaration per line, same idea as the markup formatter above.
fn format_css(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let len = chars.len();
    let mut out = String::new();
    let mut indent: usize = 0;
    let mut buf = String::new();
    let mut i = 0usize;

    while i < len {
        let c = chars[i];
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            let normalized = normalize_ws(&buf);
            if !normalized.is_empty() {
                out.push_str(&INDENT_UNIT.repeat(indent));
                out.push_str(&normalized);
                out.push('\n');
            }
            buf.clear();
            let end = find_from(&chars, i + 2, "*/").map(|p| p + 2).unwrap_or(len);
            let comment: String = chars[i..end].iter().collect();
            out.push_str(&INDENT_UNIT.repeat(indent));
            out.push_str(comment.trim());
            out.push('\n');
            i = end;
            continue;
        }

        if c == '"' || c == '\'' {
            let quote = c;
            buf.push(c);
            i += 1;
            while i < len {
                buf.push(chars[i]);
                let is_closing_quote = chars[i] == quote;
                i += 1;
                if is_closing_quote {
                    break;
                }
            }
            continue;
        }

        match c {
            '{' => {
                let selector = normalize_ws(&buf);
                if !selector.is_empty() {
                    out.push_str(&INDENT_UNIT.repeat(indent));
                    out.push_str(&selector);
                    out.push_str(" {\n");
                }
                buf.clear();
                indent += 1;
            }
            '}' => {
                let decl = normalize_ws(&buf);
                if !decl.is_empty() {
                    out.push_str(&INDENT_UNIT.repeat(indent));
                    out.push_str(&decl);
                    out.push_str(";\n");
                }
                buf.clear();
                indent = indent.saturating_sub(1);
                out.push_str(&INDENT_UNIT.repeat(indent));
                out.push_str("}\n");
            }
            ';' => {
                let decl = normalize_ws(&buf);
                if !decl.is_empty() {
                    out.push_str(&INDENT_UNIT.repeat(indent));
                    out.push_str(&decl);
                    out.push_str(";\n");
                }
                buf.clear();
            }
            _ => buf.push(c),
        }
        i += 1;
    }

    let trailing = normalize_ws(&buf);
    if !trailing.is_empty() {
        out.push_str(&INDENT_UNIT.repeat(indent));
        out.push_str(&trailing);
        out.push('\n');
    }

    let mut result = out.trim_end().to_string();
    result.push('\n');
    result
}

/// Best-effort net `{([` vs `})]` count for one line, skipping string/char literals and a
/// trailing `//` line comment. Used by `reindent_braces` to track nesting depth; it does
/// not track multi-line `/* */` comments, so braces mentioned inside one will still count.
fn net_bracket_delta(line: &str) -> i32 {
    let mut delta = 0i32;
    let mut in_string: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(q) = in_string {
            if c == '\\' {
                chars.next();
            } else if c == q {
                in_string = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => in_string = Some(c),
            '/' if chars.peek() == Some(&'/') => break,
            '{' | '(' | '[' => delta += 1,
            '}' | ')' | ']' => delta -= 1,
            _ => {}
        }
    }
    delta
}

/// Reindents (but does not reflow) brace-based source: each existing line's leading
/// whitespace is replaced based on the running `{}/()/[]` nesting depth. This is a
/// reasonable "format document" fallback for C-like languages without a full parser, but
/// it won't split up minified single-line code the way the JSON/XML/HTML/CSS formatters do.
fn reindent_braces(source: &str) -> String {
    const BRACE_INDENT_UNIT: &str = "    ";
    let mut out = String::new();
    let mut depth: i32 = 0;
    for raw_line in source.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            out.push('\n');
            continue;
        }
        let starts_with_closer =
            trimmed.starts_with('}') || trimmed.starts_with(')') || trimmed.starts_with(']');
        let line_depth = if starts_with_closer {
            (depth - 1).max(0)
        } else {
            depth.max(0)
        } as usize;
        out.push_str(&BRACE_INDENT_UNIT.repeat(line_depth));
        out.push_str(trimmed);
        out.push('\n');

        depth += net_bracket_delta(trimmed);
        if depth < 0 {
            depth = 0;
        }
    }
    if out.is_empty() {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_pretty_prints_minified_input() {
        let out = format_document(Language::Json, r#"{"a":1,"b":[1,2,3]}"#).unwrap();
        assert_eq!(
            out,
            "{\n  \"a\": 1,\n  \"b\": [\n    1,\n    2,\n    3\n  ]\n}\n"
        );
    }

    #[test]
    fn json_rejects_invalid_input() {
        assert!(format_document(Language::Json, "{not json}").is_err());
    }

    #[test]
    fn html_indents_nested_tags_and_keeps_script_verbatim() {
        let input = "<html><head><title>Hi</title></head><body><script>var x=1;if(x){y();}</script><div class=\"a\"><p>Hello</p></div><img src=\"x.png\"></body></html>";
        let out = format_document(Language::Html, input).unwrap();
        assert!(out.contains("<html>\n"));
        assert!(out.contains("  <head>\n"));
        assert!(out.contains("    <title>\n"));
        assert!(out.contains("      Hi\n"));
        assert!(out.contains("var x=1;if(x){y();}"));
        assert!(out.contains("<img src=\"x.png\">\n"));
        assert!(!out.contains("</img>"));
        assert!(out.trim_end().ends_with("</html>"));
    }

    #[test]
    fn xml_indents_self_closing_and_nested_elements() {
        let input = "<root><item id=\"1\"/><child><leaf>text</leaf></child></root>";
        let out = format_document(Language::Xml, input).unwrap();
        assert_eq!(
            out,
            "<root>\n  <item id=\"1\"/>\n  <child>\n    <leaf>\n      text\n    </leaf>\n  </child>\n</root>\n"
        );
    }

    #[test]
    fn css_formats_rules_onto_separate_lines() {
        let input = "a{color:red;background:blue}b{margin:0}";
        let out = format_document(Language::Css, input).unwrap();
        assert_eq!(
            out,
            "a {\n  color:red;\n  background:blue;\n}\nb {\n  margin:0;\n}\n"
        );
    }

    #[test]
    fn brace_reindent_fixes_nested_indentation() {
        let input = "fn main() {\nlet x = 1;\nif x == 1 {\nprintln!(\"{}\", x);\n}\n}\n";
        let out = format_document(Language::Rust, input).unwrap();
        assert_eq!(
            out,
            "fn main() {\n    let x = 1;\n    if x == 1 {\n        println!(\"{}\", x);\n    }\n}\n"
        );
    }

    #[test]
    fn unsupported_language_reports_an_error() {
        assert!(format_document(Language::Markdown, "# Title").is_err());
    }
}
