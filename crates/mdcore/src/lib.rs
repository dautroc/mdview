#![forbid(unsafe_code)]

/// The crate version, used by `mdapp` in the About panel.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_reported() {
        assert_eq!(super::version(), "0.1.0");
    }
}
