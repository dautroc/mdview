/// Escape text for insertion into HTML element content.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            other => out.push(other),
        }
    }
    out
}

/// Escape text for insertion into a double-quoted HTML attribute.
pub fn escape_attr(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

/// Encode a string as a JavaScript double-quoted literal, safe to embed inside
/// a `<script>` element or an `evaluateJavaScript` payload.
pub fn js_string_literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // `</script` inside a literal would close the enclosing element.
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            // Valid JS source terminators that Rust does not treat as newlines.
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            other if (other as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", other as u32))
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_html_metacharacters() {
        assert_eq!(escape_html("<a> & 'x' \"y\""), "&lt;a&gt; &amp; 'x' \"y\"");
    }

    #[test]
    fn escapes_quotes_in_attributes() {
        assert_eq!(escape_attr("a\"b<c"), "a&quot;b&lt;c");
    }

    #[test]
    fn js_literal_quotes_and_escapes() {
        assert_eq!(js_string_literal("a\"b"), "\"a\\\"b\"");
        assert_eq!(js_string_literal("a\\b"), "\"a\\\\b\"");
        assert_eq!(js_string_literal("a\nb"), "\"a\\nb\"");
    }

    #[test]
    fn js_literal_breaks_up_closing_script_tags() {
        // A body containing </script> would otherwise terminate the injected
        // script element early.
        let out = js_string_literal("</script>");
        assert!(!out.contains("</script>"), "got: {out}");
    }

    #[test]
    fn js_literal_escapes_line_separator() {
        // U+2028 is a newline to a JS parser but not to Rust.
        let out = js_string_literal("a\u{2028}b");
        assert!(out.contains("\\u2028"), "got: {out}");
    }
}
