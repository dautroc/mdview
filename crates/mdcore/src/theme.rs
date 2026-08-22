/// Which appearance the page uses. `System` defers to the OS via the
/// stylesheet's `prefers-color-scheme` query; every other variant pins one of
/// syntect's built-in palettes, from which the page chrome is derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    System,
    GitHub,
    SolarizedLight,
    OceanLight,
    SolarizedDark,
    Eighties,
    Mocha,
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
        }
    }

    /// The syntect built-in this theme draws its palette from. `None` for
    /// System, which uses the light/dark pair the media query selects.
    pub fn syntect_name(&self) -> Option<&'static str> {
        match self {
            Theme::System => None,
            Theme::GitHub => Some("InspiredGitHub"),
            Theme::SolarizedLight => Some("Solarized (light)"),
            Theme::OceanLight => Some("base16-ocean.light"),
            Theme::SolarizedDark => Some("Solarized (dark)"),
            Theme::Eighties => Some("base16-eighties.dark"),
            Theme::Mocha => Some("base16-mocha.dark"),
        }
    }

    pub fn is_dark(&self) -> Option<bool> {
        match self {
            Theme::System => None,
            Theme::GitHub | Theme::SolarizedLight | Theme::OceanLight => Some(false),
            Theme::SolarizedDark | Theme::Eighties | Theme::Mocha => Some(true),
        }
    }

    /// Temporary: ⌘T still cycles until Task 3 moves that affordance into
    /// `app.rs`. Advances through `all()` in order, wrapping at the end.
    pub fn next(self) -> Theme {
        let all = Theme::all();
        let i = all.iter().position(|t| *t == self).unwrap_or(0);
        all[(i + 1) % all.len()]
    }
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
    fn the_picker_offers_system_plus_three_light_and_three_dark() {
        let all = Theme::all();
        assert_eq!(all.len(), 7);
        assert_eq!(all.iter().filter(|t| t.is_dark() == Some(false)).count(), 3);
        assert_eq!(all.iter().filter(|t| t.is_dark() == Some(true)).count(), 3);
    }

    #[test]
    fn every_named_theme_resolves_to_a_real_syntect_palette() {
        // A typo in a syntect theme name would otherwise surface as an
        // unstyled page at runtime, with every test still green.
        let set = syntect::highlighting::ThemeSet::load_defaults();
        for theme in Theme::all() {
            if let Some(name) = theme.syntect_name() {
                assert!(set.themes.contains_key(name), "no syntect theme named {name}");
            }
        }
    }
}
