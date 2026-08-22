//! Page chrome derived from a syntax theme's own palette.
//!
//! Hand-writing a chrome palette beside each syntax palette means two things
//! that can disagree, per theme. This project has already shipped two bugs of
//! exactly that shape. Computing one from the other makes the class impossible.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub fn hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// Blend `a` toward `b` by `t`, clamped to 0..=1.
pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Rgb { r: lerp(a.r, b.r), g: lerp(a.g, b.g), b: lerp(a.b, b.b) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromeTokens {
    pub bg: Rgb,
    pub fg: Rgb,
    pub muted: Rgb,
    pub border: Rgb,
    pub code_bg: Rgb,
    pub link: Rgb,
    pub banner_bg: Rgb,
    pub banner_fg: Rgb,
    pub banner_border: Rgb,
}

/// Derive every chrome token from a theme's background and foreground.
pub fn tokens(bg: Rgb, fg: Rgb, dark: bool) -> ChromeTokens {
    // A dark theme's code surface reads better slightly lighter than the page;
    // a light theme's slightly darker. Both are a small step toward the text.
    let code_bg = mix(bg, fg, if dark { 0.07 } else { 0.05 });
    let accent = if dark {
        Rgb { r: 0x6c, g: 0xb6, b: 0xff }
    } else {
        Rgb { r: 0x09, g: 0x69, b: 0xda }
    };
    let warn = if dark {
        Rgb { r: 0xf2, g: 0xcc, b: 0x60 }
    } else {
        Rgb { r: 0x9a, g: 0x6d, b: 0x03 }
    };
    ChromeTokens {
        bg,
        fg,
        muted: mix(fg, bg, 0.40),
        border: mix(bg, fg, 0.22),
        code_bg,
        link: accent,
        banner_bg: mix(bg, warn, if dark { 0.18 } else { 0.22 }),
        banner_fg: if dark { warn } else { warn },
        banner_border: mix(bg, warn, 0.45),
    }
}

impl ChromeTokens {
    pub fn to_css_vars(&self) -> String {
        format!(
            "--bg:{};--fg:{};--muted:{};--border:{};--code-bg:{};--link:{};\
--banner-bg:{};--banner-fg:{};--banner-border:{};",
            self.bg.hex(),
            self.fg.hex(),
            self.muted.hex(),
            self.border.hex(),
            self.code_bg.hex(),
            self.link.hex(),
            self.banner_bg.hex(),
            self.banner_fg.hex(),
            self.banner_border.hex(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIGHT_BG: Rgb = Rgb { r: 0xff, g: 0xff, b: 0xff };
    const LIGHT_FG: Rgb = Rgb { r: 0x32, g: 0x32, b: 0x32 };
    const DARK_BG: Rgb = Rgb { r: 0x3b, g: 0x32, b: 0x28 };
    const DARK_FG: Rgb = Rgb { r: 0xd0, g: 0xc8, b: 0xc6 };

    #[test]
    fn mix_endpoints_are_the_inputs() {
        assert_eq!(mix(LIGHT_BG, LIGHT_FG, 0.0), LIGHT_BG);
        assert_eq!(mix(LIGHT_BG, LIGHT_FG, 1.0), LIGHT_FG);
    }

    #[test]
    fn mix_is_clamped() {
        assert_eq!(mix(LIGHT_BG, LIGHT_FG, -1.0), LIGHT_BG);
        assert_eq!(mix(LIGHT_BG, LIGHT_FG, 2.0), LIGHT_FG);
    }

    #[test]
    fn code_background_is_distinguishable_from_the_page() {
        // A code block must read as a distinct surface on every theme, or
        // fenced code vanishes into the page.
        for (bg, fg, dark) in [(LIGHT_BG, LIGHT_FG, false), (DARK_BG, DARK_FG, true)] {
            let t = tokens(bg, fg, dark);
            assert_ne!(t.code_bg, t.bg, "code surface must differ from page");
        }
    }

    #[test]
    fn muted_sits_between_page_and_text() {
        // Muted text must be dimmer than body text but still legible, i.e.
        // strictly between the two endpoints on every channel.
        for (bg, fg, dark) in [(LIGHT_BG, LIGHT_FG, false), (DARK_BG, DARK_FG, true)] {
            let t = tokens(bg, fg, dark);
            let between = |c: u8, a: u8, b: u8| (c >= a.min(b)) && (c <= a.max(b));
            assert!(between(t.muted.r, bg.r, fg.r), "muted.r outside bg..fg");
            assert!(between(t.muted.g, bg.g, fg.g), "muted.g outside bg..fg");
            assert!(between(t.muted.b, bg.b, fg.b), "muted.b outside bg..fg");
            assert_ne!(t.muted, fg, "muted must actually be dimmer than body text");
        }
    }

    #[test]
    fn every_token_is_emitted_as_a_css_variable() {
        let css = tokens(DARK_BG, DARK_FG, true).to_css_vars();
        for name in [
            "--bg", "--fg", "--muted", "--border", "--code-bg", "--link",
            "--banner-bg", "--banner-fg", "--banner-border",
        ] {
            assert!(css.contains(name), "missing {name}");
        }
    }

    #[test]
    fn tokens_differ_between_a_light_and_a_dark_source() {
        assert_ne!(
            tokens(LIGHT_BG, LIGHT_FG, false).to_css_vars(),
            tokens(DARK_BG, DARK_FG, true).to_css_vars()
        );
    }
}
