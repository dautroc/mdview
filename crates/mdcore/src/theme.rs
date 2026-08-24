/// Which appearance the page uses. `System` defers to the OS via the
/// stylesheet's `prefers-color-scheme` query; every other variant pins a
/// palette -- one of syntect's built-ins, or one MDView bundles itself -- from
/// which the page chrome is derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    System,
    GitHub,
    SolarizedLight,
    OceanLight,
    SolarizedDark,
    Eighties,
    Mocha,
    MonokaiPro,
}

impl Theme {
    pub fn all() -> &'static [Theme] {
        &[
            Theme::System,
            Theme::GitHub,
            Theme::SolarizedLight,
            Theme::OceanLight,
            Theme::SolarizedDark,
            Theme::Eighties,
            Theme::Mocha,
            Theme::MonokaiPro,
        ]
    }

    pub fn as_wire(&self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::GitHub => "github",
            Theme::SolarizedLight => "solarized-light",
            Theme::OceanLight => "ocean-light",
            Theme::SolarizedDark => "solarized-dark",
            Theme::Eighties => "eighties",
            Theme::Mocha => "mocha",
            Theme::MonokaiPro => "monokai-pro",
        }
    }

    /// Anything unrecognised means "follow the OS" rather than an error — a
    /// stale stored value must not leave the app unable to pick an appearance.
    pub fn from_wire(value: &str) -> Theme {
        Theme::all()
            .iter()
            .copied()
            .find(|t| t.as_wire() == value)
            .unwrap_or(Theme::System)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Theme::System => "System",
            Theme::GitHub => "GitHub",
            Theme::SolarizedLight => "Solarized Light",
            Theme::OceanLight => "Ocean Light",
            Theme::SolarizedDark => "Solarized Dark",
            Theme::Eighties => "Eighties",
            Theme::Mocha => "Mocha",
            Theme::MonokaiPro => "Monokai Pro",
        }
    }

    /// The syntect palette this theme draws from -- a built-in, or one of
    /// MDView's bundled tmThemes. `None` for System, which uses the
    /// light/dark pair the media query selects.
    pub fn syntect_name(&self) -> Option<&'static str> {
        match self {
            Theme::System => None,
            Theme::GitHub => Some("InspiredGitHub"),
            Theme::SolarizedLight => Some("Solarized (light)"),
            Theme::OceanLight => Some("base16-ocean.light"),
            Theme::SolarizedDark => Some("Solarized (dark)"),
            Theme::Eighties => Some("base16-eighties.dark"),
            Theme::Mocha => Some("base16-mocha.dark"),
            // Not a syntect built-in; resolved from the bundled tmTheme.
            Theme::MonokaiPro => Some("Monokai Pro"),
        }
    }

    pub fn is_dark(&self) -> Option<bool> {
        match self {
            Theme::System => None,
            Theme::GitHub | Theme::SolarizedLight | Theme::OceanLight => Some(false),
            Theme::SolarizedDark | Theme::Eighties | Theme::Mocha | Theme::MonokaiPro => {
                Some(true)
            }
        }
    }
}

/// What `page.css` paints for `System` in each OS appearance. Mirrored here so
/// the window chrome can match a page that has no theme of its own; the test
/// below reads the stylesheet and fails if the two drift apart.
pub const SYSTEM_LIGHT_BG: crate::chrome::Rgb = crate::chrome::Rgb { r: 0xff, g: 0xff, b: 0xff };
pub const SYSTEM_DARK_BG: crate::chrome::Rgb = crate::chrome::Rgb { r: 0x0d, g: 0x11, b: 0x17 };

/// The page background a theme paints, so the window chrome around the page
/// can be given the same colour instead of staying system grey above a dark
/// document. `None` for `System`, which defers to the OS appearance.
pub fn background(theme: Theme) -> Option<crate::chrome::Rgb> {
    let name = theme.syntect_name()?;
    crate::highlight::palette_for(name).map(|(_, bg, _)| bg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_round_trips_on_the_wire() {
        for theme in Theme::all() {
            assert_eq!(Theme::from_wire(theme.as_wire()), *theme);
        }
    }

    #[test]
    fn unknown_wire_value_falls_back_to_system() {
        assert_eq!(Theme::from_wire("tokyo-night"), Theme::System);
        assert_eq!(Theme::from_wire(""), Theme::System);
    }

    #[test]
    fn system_alone_has_no_syntect_theme_and_no_fixed_darkness() {
        assert_eq!(Theme::System.syntect_name(), None);
        assert_eq!(Theme::System.is_dark(), None);
        for theme in Theme::all().iter().filter(|t| **t != Theme::System) {
            assert!(theme.syntect_name().is_some(), "{:?} needs a palette", theme);
            assert!(theme.is_dark().is_some(), "{:?} needs a darkness", theme);
        }
    }

    #[test]
    fn the_picker_offers_system_plus_three_light_and_four_dark() {
        let all = Theme::all();
        assert_eq!(all.len(), 8);
        assert_eq!(all.iter().filter(|t| t.is_dark() == Some(false)).count(), 3);
        assert_eq!(all.iter().filter(|t| t.is_dark() == Some(true)).count(), 4);
    }

    #[test]
    fn every_named_theme_resolves_to_a_real_palette() {
        // A typo in a palette name would otherwise surface as an unstyled
        // page at runtime, with every test still green. Resolve through the
        // renderer's own path, not `ThemeSet::load_defaults` directly -- not
        // every palette is a syntect built-in.
        for theme in Theme::all() {
            if let Some(name) = theme.syntect_name() {
                assert!(
                    crate::highlight::palette_for(name).is_some(),
                    "no palette named {name}"
                );
            }
        }
    }
}

#[cfg(test)]
mod background_tests {
    use super::*;

    #[test]
    fn every_named_theme_reports_a_background_and_system_does_not() {
        assert_eq!(background(Theme::System), None, "System defers to the OS");
        for theme in Theme::all() {
            if *theme == Theme::System {
                continue;
            }
            let bg = background(*theme).expect("named themes must report a background");
            // The window colour has to agree with the page, or the titlebar
            // reads as a seam rather than an edge.
            assert_eq!(
                theme.is_dark(),
                Some(crate::chrome::luminance(bg) < 0.5),
                "{} background disagrees with its declared darkness",
                theme.as_wire()
            );
        }
    }
}

#[cfg(test)]
mod system_background_tests {
    use super::*;

    /// Pull the `--bg` declared in the first `:root` block at or after `from`.
    fn bg_after(css: &str, from: usize) -> String {
        let root = css[from..].find(":root").expect("no :root block") + from;
        let decl = css[root..].find("--bg:").expect("no --bg") + root + "--bg:".len();
        let end = css[decl..].find(';').expect("unterminated --bg") + decl;
        css[decl..end].trim().to_string()
    }

    #[test]
    fn the_system_window_colours_match_what_the_stylesheet_paints() {
        let css = crate::assets::PAGE_CSS;
        let dark_at = css
            .find("prefers-color-scheme: dark")
            .expect("no dark media query");
        assert_eq!(bg_after(css, 0), SYSTEM_LIGHT_BG.hex());
        assert_eq!(bg_after(css, dark_at), SYSTEM_DARK_BG.hex());
    }
}
