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

/// WCAG relative luminance.
pub fn luminance(c: Rgb) -> f32 {
    fn linearize(channel: u8) -> f32 {
        let c = channel as f32 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * linearize(c.r) + 0.7152 * linearize(c.g) + 0.0722 * linearize(c.b)
}

/// WCAG contrast ratio, always >= 1.0.
pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    let la = luminance(a);
    let lb = luminance(b);
    let (lighter, darker) = if la >= lb { (la, lb) } else { (lb, la) };
    (lighter + 0.05) / (darker + 0.05)
}

/// Blend `from` toward `to` by the smallest amount that reaches `target`
/// contrast against `against`, or return the full blend if unreachable.
/// A fixed blend fraction cannot serve palettes whose own fg/bg contrast
/// spans 4.13:1 to 12.82:1 — this targets the property that matters instead.
pub fn mix_to_contrast(from: Rgb, to: Rgb, against: Rgb, target: f32) -> Rgb {
    let mut best = to;
    for step in 0..=20 {
        let candidate = mix(from, to, step as f32 / 20.0);
        if contrast(candidate, against) >= target {
            best = candidate;
            break;
        }
    }
    best
}

/// The colours a syntax palette states for Markdown's own constructs.
///
/// Every field is optional because most palettes describe code and say
/// nothing about prose: `InspiredGitHub` styles headings with `font-weight`
/// and no colour at all. `None` means "this palette has nothing to say here",
/// and `tokens` falls back to the page foreground rather than inventing a hue.
/// `highlight::markup_palette_for` is what fills this in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MarkupPalette {
    pub heading: Option<Rgb>,
    pub link: Option<Rgb>,
    /// `markup.raw` — inline code, and nothing else: fenced blocks are
    /// coloured token by token by syntect's own sheet.
    pub raw: Option<Rgb>,
    pub quote: Option<Rgb>,
    pub bold: Option<Rgb>,
    pub italic: Option<Rgb>,
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
    pub diff_add_bg: Rgb,
    pub diff_add_fg: Rgb,
    /// Strong enough to read as a hue in a 3px rule. The `_bg` tints are laid
    /// under text and cannot be: at that width they are invisible.
    pub diff_add_border: Rgb,
    pub diff_del_bg: Rgb,
    pub diff_del_fg: Rgb,
    pub diff_del_border: Rgb,
    pub diff_hunk_bg: Rgb,
    pub diff_hunk_fg: Rgb,
    pub find_hit_bg: Rgb,
    pub find_hit_fg: Rgb,
    pub find_current_bg: Rgb,
    pub find_current_fg: Rgb,
    pub comment_bg: Rgb,
    pub comment_fg: Rgb,
    /// The document's own colours, as opposed to the chrome around it. Each
    /// is the palette's hue lifted toward `fg` until it clears 4.5:1 against
    /// the surface it is actually laid on -- `code_bg` for the two that sit
    /// on the tinted surface, `bg` for the rest.
    pub heading: Rgb,
    pub strong: Rgb,
    pub em: Rgb,
    pub code_fg: Rgb,
    pub quote_fg: Rgb,
    pub table_head_fg: Rgb,
}

/// Derive every chrome token from a theme's background and foreground, and
/// the document's own colours from what that theme says about Markdown.
pub fn tokens(bg: Rgb, fg: Rgb, dark: bool, markup: MarkupPalette) -> ChromeTokens {
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
    let add = if dark {
        Rgb { r: 0x3f, g: 0xb9, b: 0x50 }
    } else {
        Rgb { r: 0x1a, g: 0x7f, b: 0x37 }
    };
    let delete = if dark {
        Rgb { r: 0xf8, g: 0x51, b: 0x49 }
    } else {
        Rgb { r: 0xcf, g: 0x22, b: 0x2e }
    };
    let hunk = if dark {
        Rgb { r: 0x6c, g: 0xb6, b: 0xff }
    } else {
        Rgb { r: 0x09, g: 0x69, b: 0xda }
    };
    // Find highlights are two surfaces, not one: every match is tinted with
    // the warn hue, and the match the user is standing on takes the accent, so
    // "which one am I on" is a hue change rather than a brightness change that
    // a busy page can swallow.
    // Comments take a third hue for the same reason find takes two: a page
    // can hold a find match and a comment anchor at once, and telling them
    // apart by tint alone is the only cue available inside running text.
    let comment = if dark {
        Rgb { r: 0xd2, g: 0xa8, b: 0xff }
    } else {
        Rgb { r: 0x82, g: 0x50, b: 0xdf }
    };
    let comment_bg = mix(bg, comment, if dark { 0.22 } else { 0.28 });
    let find_hit_bg = mix(bg, warn, if dark { 0.20 } else { 0.30 });
    let find_current_bg = mix(bg, accent, if dark { 0.24 } else { 0.34 });
    let diff_add_bg = mix(bg, add, 0.18);
    let diff_del_bg = mix(bg, delete, 0.18);
    let diff_hunk_bg = mix(bg, hunk, 0.12);
    // A palette's own hue is chosen to sit on the palette's own background,
    // which is not always this page's: base16-ocean.light heads its sections
    // in #8fa1b3, which is 2.4:1 on its #eff1f5 page. Lift each hue toward
    // the text colour by the smallest step that reaches the target -- against
    // the surface the text actually lands on, which for inline code and table
    // headings is `code_bg` and not `bg`.
    //
    // Where the target is unreachable, `mix_to_contrast` hands back the full
    // blend -- which is `fg`, and no accent at all. Some palettes cannot ever
    // reach 4.5 for an accent: Solarized Light runs its own teal at 2.9:1
    // against its page and its own body text at 4.13:1, so a flat demand
    // turns that whole theme's document monochrome, which is the thing this
    // derivation exists to prevent. Settle for the 3:1 floor instead of
    // giving up the hue.
    let readable = |hue: Option<Rgb>, on: Rgb, target: f32| match hue {
        Some(hue) => {
            let lifted = mix_to_contrast(hue, fg, on, target);
            if lifted == fg && target > 3.0 {
                mix_to_contrast(hue, fg, on, 3.0)
            } else {
                lifted
            }
        }
        // Nothing to say: the page foreground, which is already readable.
        None => fg,
    };
    // Headings are large text, where 3:1 is the WCAG floor. Holding them to
    // the body-text 4.5 costs real hue for nothing: Solarized Dark heads its
    // sections in #b58900, which lands at 4.46:1 -- four hundredths short,
    // and enough to walk the yellow all the way to grey.
    let heading = readable(markup.heading, bg, 3.0);
    ChromeTokens {
        bg,
        fg,
        // Fixed blend fractions cannot serve palettes whose own fg/bg
        // contrast ranges from 4.13:1 (Solarized Light) to 12.82:1 (GitHub):
        // measured results included muted at 2.16:1 on Solarized Light and
        // border at 1.29-1.74:1 everywhere. Target the contrast ratio itself.
        muted: mix_to_contrast(bg, fg, bg, 4.5),
        border: mix_to_contrast(bg, fg, bg, 3.0),
        code_bg,
        // The palette's own link hue, not the chrome accent: a document whose
        // links are GitHub blue on every theme is the one place the page
        // visibly disagrees with the code inside it.
        link: readable(markup.link, bg, 4.5),
        banner_bg: mix(bg, warn, if dark { 0.18 } else { 0.22 }),
        banner_fg: warn,
        banner_border: mix(bg, warn, 0.45),
        diff_add_bg,
        diff_add_fg: mix_to_contrast(diff_add_bg, fg, diff_add_bg, 4.5),
        diff_add_border: mix(bg, add, 0.55),
        diff_del_bg,
        diff_del_fg: mix_to_contrast(diff_del_bg, fg, diff_del_bg, 4.5),
        diff_del_border: mix(bg, delete, 0.55),
        diff_hunk_bg,
        diff_hunk_fg: mix_to_contrast(diff_hunk_bg, fg, diff_hunk_bg, 4.5),
        find_hit_bg,
        find_hit_fg: mix_to_contrast(find_hit_bg, fg, find_hit_bg, 4.5),
        find_current_bg,
        find_current_fg: mix_to_contrast(find_current_bg, fg, find_current_bg, 4.5),
        comment_bg,
        comment_fg: mix_to_contrast(comment_bg, fg, comment_bg, 4.5),
        heading,
        strong: readable(markup.bold, bg, 4.5),
        em: readable(markup.italic, bg, 4.5),
        code_fg: readable(markup.raw, code_bg, 4.5),
        quote_fg: readable(markup.quote, bg, 4.5),
        // A table's head is the heading hue on the code surface -- the same
        // role, and `th` already carries that background. Body size, though,
        // so it is held to the body target rather than the heading's.
        table_head_fg: readable(markup.heading, code_bg, 4.5),
    }
}

impl ChromeTokens {
    pub fn to_css_vars(&self) -> String {
        format!(
            "--bg:{};--fg:{};--muted:{};--border:{};--code-bg:{};--link:{};\
--banner-bg:{};--banner-fg:{};--banner-border:{};\
--diff-add-bg:{};--diff-add-fg:{};--diff-add-border:{};\
--diff-del-bg:{};--diff-del-fg:{};--diff-del-border:{};\
--diff-hunk-bg:{};--diff-hunk-fg:{};\
--find-hit-bg:{};--find-hit-fg:{};--find-current-bg:{};--find-current-fg:{};\
--comment-bg:{};--comment-fg:{};\
--heading:{};--strong:{};--em:{};--code-fg:{};--quote-fg:{};--table-head-fg:{};",
            self.bg.hex(),
            self.fg.hex(),
            self.muted.hex(),
            self.border.hex(),
            self.code_bg.hex(),
            self.link.hex(),
            self.banner_bg.hex(),
            self.banner_fg.hex(),
            self.banner_border.hex(),
            self.diff_add_bg.hex(),
            self.diff_add_fg.hex(),
            self.diff_add_border.hex(),
            self.diff_del_bg.hex(),
            self.diff_del_fg.hex(),
            self.diff_del_border.hex(),
            self.diff_hunk_bg.hex(),
            self.diff_hunk_fg.hex(),
            self.find_hit_bg.hex(),
            self.find_hit_fg.hex(),
            self.find_current_bg.hex(),
            self.find_current_fg.hex(),
            self.comment_bg.hex(),
            self.comment_fg.hex(),
            self.heading.hex(),
            self.strong.hex(),
            self.em.hex(),
            self.code_fg.hex(),
            self.quote_fg.hex(),
            self.table_head_fg.hex(),
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
            let t = tokens(bg, fg, dark, MarkupPalette::default());
            assert_ne!(t.code_bg, t.bg, "code surface must differ from page");
        }
    }

    #[test]
    fn muted_sits_between_page_and_text() {
        // Muted text must be dimmer than body text but still legible, i.e.
        // strictly between the two endpoints on every channel.
        for (bg, fg, dark) in [(LIGHT_BG, LIGHT_FG, false), (DARK_BG, DARK_FG, true)] {
            let t = tokens(bg, fg, dark, MarkupPalette::default());
            let between = |c: u8, a: u8, b: u8| (c >= a.min(b)) && (c <= a.max(b));
            assert!(between(t.muted.r, bg.r, fg.r), "muted.r outside bg..fg");
            assert!(between(t.muted.g, bg.g, fg.g), "muted.g outside bg..fg");
            assert!(between(t.muted.b, bg.b, fg.b), "muted.b outside bg..fg");
            assert_ne!(t.muted, fg, "muted must actually be dimmer than body text");
        }
    }

    #[test]
    fn every_token_is_emitted_as_a_css_variable() {
        let css = tokens(DARK_BG, DARK_FG, true, MarkupPalette::default()).to_css_vars();
        for name in [
            "--bg", "--fg", "--muted", "--border", "--code-bg", "--link",
            "--banner-bg", "--banner-fg", "--banner-border",
            "--diff-add-bg", "--diff-add-fg", "--diff-add-border",
            "--diff-del-bg", "--diff-del-fg", "--diff-del-border",
            "--diff-hunk-bg", "--diff-hunk-fg",
            "--find-hit-bg", "--find-hit-fg",
            "--find-current-bg", "--find-current-fg",
            "--comment-bg", "--comment-fg",
            "--heading", "--strong", "--em", "--code-fg", "--quote-fg", "--table-head-fg",
        ] {
            assert!(css.contains(name), "missing {name}");
        }
    }

    #[test]
    fn tokens_differ_between_a_light_and_a_dark_source() {
        assert_ne!(
            tokens(LIGHT_BG, LIGHT_FG, false, MarkupPalette::default()).to_css_vars(),
            tokens(DARK_BG, DARK_FG, true, MarkupPalette::default()).to_css_vars()
        );
    }

    #[test]
    fn diff_foregrounds_are_readable_on_their_backgrounds() {
        for (bg, fg, dark) in [(LIGHT_BG, LIGHT_FG, false), (DARK_BG, DARK_FG, true)] {
            let t = tokens(bg, fg, dark, MarkupPalette::default());
            assert!(contrast(t.diff_add_fg, t.diff_add_bg) >= 4.5);
            assert!(contrast(t.diff_del_fg, t.diff_del_bg) >= 4.5);
            assert!(contrast(t.diff_hunk_fg, t.diff_hunk_bg) >= 4.5);
        }
    }

    #[test]
    fn find_highlights_are_readable_and_distinct_from_each_other() {
        for (bg, fg, dark) in [(LIGHT_BG, LIGHT_FG, false), (DARK_BG, DARK_FG, true)] {
            let t = tokens(bg, fg, dark, MarkupPalette::default());
            assert!(contrast(t.find_hit_fg, t.find_hit_bg) >= 4.5);
            assert!(contrast(t.find_current_fg, t.find_current_bg) >= 4.5);
            // The current match has to be tellable from the other matches, or
            // stepping through them shows no movement.
            assert_ne!(t.find_hit_bg, t.find_current_bg);
            // Both have to be tellable from the page itself.
            assert_ne!(t.find_hit_bg, t.bg);
            assert_ne!(t.find_current_bg, t.bg);
        }
    }
}

#[cfg(test)]
mod contrast_tests {
    use super::*;

    #[test]
    fn luminance_endpoints() {
        let black = Rgb { r: 0, g: 0, b: 0 };
        let white = Rgb { r: 255, g: 255, b: 255 };
        assert!(luminance(black) < 0.001);
        assert!((luminance(white) - 1.0).abs() < 0.001);
    }

    #[test]
    fn contrast_of_a_colour_with_itself_is_one() {
        let c = Rgb { r: 0x40, g: 0x80, b: 0xc0 };
        assert!((contrast(c, c) - 1.0).abs() < 0.001);
    }

    #[test]
    fn contrast_is_symmetric_and_at_least_one() {
        let black = Rgb { r: 0, g: 0, b: 0 };
        let white = Rgb { r: 255, g: 255, b: 255 };
        assert_eq!(contrast(black, white), contrast(white, black));
        assert!(contrast(black, white) >= 1.0);
        assert!((contrast(black, white) - 21.0).abs() < 0.01);
    }

    #[test]
    fn mix_to_contrast_reaches_the_target_when_reachable() {
        let bg = Rgb { r: 0xff, g: 0xff, b: 0xff };
        let fg = Rgb { r: 0x00, g: 0x00, b: 0x00 };
        let muted = mix_to_contrast(bg, fg, bg, 4.5);
        assert!(contrast(muted, bg) >= 4.5);
        // Must be a genuine blend, not the full jump straight to fg.
        assert_ne!(muted, fg);
    }

    #[test]
    fn mix_to_contrast_falls_back_to_the_full_blend_when_unreachable() {
        // fg/bg here can never reach a 10:1 contrast, so the smallest amount
        // that reaches it does not exist -- the fallback must be `to`.
        let bg = Rgb { r: 0xdd, g: 0xdd, b: 0xdd };
        let fg = Rgb { r: 0x99, g: 0x99, b: 0x99 };
        assert!(contrast(fg, bg) < 10.0);
        assert_eq!(mix_to_contrast(bg, fg, bg, 10.0), fg);
    }

    /// v0.16 painted every theme's links the same GitHub blue, because
    /// `link` was assigned the chrome accent rather than anything the palette
    /// said. This is the test that fails if that assignment comes back.
    #[test]
    fn links_come_from_the_palette_and_not_the_chrome_accent() {
        use crate::theme::Theme;

        let accents = [
            Rgb { r: 0x6c, g: 0xb6, b: 0xff },
            Rgb { r: 0x09, g: 0x69, b: 0xda },
        ];
        let mut differ = 0;
        for theme in Theme::all().iter().filter(|t| **t != Theme::System) {
            let name = theme.syntect_name().expect("named theme has a palette");
            let (_, bg, fg) = crate::highlight::palette_for(name).expect("palette");
            let markup = crate::highlight::markup_palette_for(name).expect("markup palette");
            let t = tokens(bg, fg, theme.is_dark().expect("darkness"), markup);
            if !accents.contains(&t.link) {
                differ += 1;
            }
        }
        assert_eq!(
            differ,
            Theme::all().len() - 1,
            "some theme still paints its links the hard-coded accent"
        );
    }

    /// These assertions are the point of fix 8: the old fixed blend fractions
    /// measured at 2.16:1-2.62:1 for `--muted` (below even the 3:1 large-text
    /// floor) and 1.29:1-1.74:1 for `--border` (below the 3:1 non-text floor)
    /// on real palettes. Run them over every named theme's actual palette,
    /// not synthetic colours.
    #[test]
    fn muted_and_border_meet_contrast_targets_on_every_real_palette() {
        use crate::theme::Theme;

        let mut checked = 0;
        for theme in Theme::all().iter().filter(|t| **t != Theme::System) {
            let syntect_name = theme.syntect_name().expect("named theme has a palette");
            let (_, bg, fg) = crate::highlight::palette_for(syntect_name)
                .unwrap_or_else(|| panic!("no bundled syntect palette for {}", theme.label()));
            let is_dark = theme.is_dark().expect("named theme has a darkness");
            let markup = crate::highlight::markup_palette_for(syntect_name)
                .unwrap_or_else(|| panic!("no markup palette for {}", theme.label()));
            let t = tokens(bg, fg, is_dark, markup);
            checked += 1;

            // fg/bg pass straight through unchanged.
            assert_eq!(t.fg, fg, "{}: fg must be unchanged", theme.label());
            assert_eq!(t.bg, bg, "{}: bg must be unchanged", theme.label());
            assert_eq!(
                contrast(t.fg, t.bg),
                contrast(fg, bg),
                "{}: fg/bg contrast must be unchanged",
                theme.label()
            );

            assert_ne!(t.code_bg, t.bg, "{}: code surface must differ from the page", theme.label());

            // The document's own colours, each measured against the surface
            // it is actually laid on. 3:1 is the floor `readable` settles for
            // when a palette's hue cannot reach the body target at all --
            // Solarized Light's accents are all below its own text contrast.
            for (name, colour, on) in [
                ("heading", t.heading, t.bg),
                ("link", t.link, t.bg),
                ("strong", t.strong, t.bg),
                ("em", t.em, t.bg),
                ("quote-fg", t.quote_fg, t.bg),
                ("code-fg", t.code_fg, t.code_bg),
                ("table-head-fg", t.table_head_fg, t.code_bg),
            ] {
                let ratio = contrast(colour, on);
                assert!(
                    ratio >= 3.0,
                    "{}: {name} contrast {ratio:.2} below the 3:1 floor",
                    theme.label()
                );
            }

            // The point of the whole derivation: a document that is not the
            // same page on every theme. Heading, link and inline code all
            // collapsing to fg means the probe found nothing at all.
            assert!(
                t.heading != t.fg || t.link != t.fg || t.code_fg != t.fg,
                "{}: the document has no colour of its own",
                theme.label()
            );

            let border_contrast = contrast(t.border, t.bg);
            assert!(
                border_contrast >= 3.0,
                "{}: border contrast {border_contrast:.2} below the 3:1 non-text floor",
                theme.label()
            );

            let muted_contrast = contrast(t.muted, t.bg);
            if contrast(fg, bg) >= 4.5 {
                assert!(
                    muted_contrast >= 4.5,
                    "{}: muted contrast {muted_contrast:.2} below the 4.5:1 text floor",
                    theme.label()
                );
            } else {
                // Solarized Light's own fg/bg contrast is only 4.13:1, below
                // the 4.5 target -- 4.5 is mathematically unreachable by
                // blending toward fg. The documented fallback is the full
                // blend (muted == fg), the best any derived colour can do.
                assert_eq!(
                    t.muted, t.fg,
                    "{}: 4.5 is unreachable on this palette, muted should fall back to fg",
                    theme.label()
                );
            }
        }
        assert_eq!(
            checked,
            Theme::all().len() - 1,
            "expected to check every named theme"
        );
    }
}
