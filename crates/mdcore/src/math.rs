use crate::escape::escape_html;

/// Wrap a TeX expression in markup the bundled KaTeX init script looks for.
///
/// The body is HTML-escaped (so `<` and `&` cannot break out) but otherwise
/// preserved exactly — in particular backslashes are untouched, which is what
/// keeps `\frac{a}{b}` renderable.
pub fn math_event_html(tex: &str, display: bool) -> String {
    if display {
        format!("<div class=\"math-display\">{}</div>", escape_html(tex))
    } else {
        format!("<span class=\"math-inline\">{}</span>", escape_html(tex))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_math_becomes_a_span() {
        let html = math_event_html("x^2", false);
        assert_eq!(html, "<span class=\"math-inline\">x^2</span>");
    }

    #[test]
    fn display_math_becomes_a_div() {
        let html = math_event_html("\\int_0^1 x", true);
        assert_eq!(html, "<div class=\"math-display\">\\int_0^1 x</div>");
    }

    #[test]
    fn backslashes_and_braces_survive_untouched() {
        let html = math_event_html("\\frac{a}{b}", false);
        assert!(html.contains("\\frac{a}{b}"), "got: {html}");
    }

    #[test]
    fn html_metacharacters_are_escaped() {
        let html = math_event_html("a < b & c", false);
        assert!(html.contains("a &lt; b &amp; c"), "got: {html}");
    }
}
