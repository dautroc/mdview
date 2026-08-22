/// Which appearance the page should use. `System` defers to the OS via the
/// stylesheet's `prefers-color-scheme` query; the other two pin it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    System,
    Light,
    Dark,
}

impl Theme {
    /// The string used in persisted defaults and on the bridge wire.
    pub fn as_wire(&self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    /// Parse a persisted or bridge value. Anything unrecognised means
    /// "follow the OS" rather than an error — a bad stored value must not
    /// leave the app unable to pick an appearance.
    pub fn from_wire(value: &str) -> Theme {
        match value {
            "light" => Theme::Light,
            "dark" => Theme::Dark,
            _ => Theme::System,
        }
    }

    /// The order the toggle cycles through.
    pub fn next(self) -> Theme {
        match self {
            Theme::System => Theme::Light,
            Theme::Light => Theme::Dark,
            Theme::Dark => Theme::System,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_round_trips() {
        for theme in [Theme::System, Theme::Light, Theme::Dark] {
            assert_eq!(Theme::from_wire(theme.as_wire()), theme);
        }
    }

    #[test]
    fn unknown_wire_value_falls_back_to_system() {
        // Following the OS is the safe default for a value we cannot read.
        assert_eq!(Theme::from_wire("chartreuse"), Theme::System);
        assert_eq!(Theme::from_wire(""), Theme::System);
    }

    #[test]
    fn next_cycles_system_light_dark() {
        assert_eq!(Theme::System.next(), Theme::Light);
        assert_eq!(Theme::Light.next(), Theme::Dark);
        assert_eq!(Theme::Dark.next(), Theme::System);
    }
}
