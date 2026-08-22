mod app;
mod bridge;
mod defaults;
mod menu;
mod navigation;
mod state;
mod watcher;
mod window;

use std::path::PathBuf;

fn main() {
    let mut print_html = false;
    let mut theme = mdcore::Theme::System;
    let mut want_theme = false;
    let mut paths: Vec<PathBuf> = Vec::new();

    for arg in std::env::args_os().skip(1) {
        let arg = arg.to_string_lossy().into_owned();

        if want_theme {
            want_theme = false;
            // A value that itself starts with "--" is another flag, not a
            // theme name: `--theme --print-html` must not swallow
            // `--print-html` as an (unrecognised, System-defaulting) theme
            // value. Treat the missing value as System — already the
            // default — and let this token fall through to be parsed
            // normally below instead of being consumed here.
            if !arg.starts_with("--") {
                theme = mdcore::Theme::from_wire(&arg);
                continue;
            }
        }

        match arg.as_str() {
            "--print-html" => print_html = true,
            "--theme" => want_theme = true,
            "--version" => {
                println!("mdview {}", mdcore::version());
                return;
            }
            "--help" | "-h" => {
                println!("usage: mdview [--print-html] [--theme THEME] [FILE...]");
                return;
            }
            other => paths.push(PathBuf::from(other)),
        }
    }

    if print_html {
        let Some(path) = paths.first() else {
            eprintln!("mdview: --print-html requires a file path");
            std::process::exit(2);
        };
        match mdcore::render_document(path, theme) {
            Ok(doc) => print!("{}", doc.html),
            Err(err) => {
                eprintln!("mdview: {err}");
                std::process::exit(1);
            }
        }
        return;
    }

    app::run(paths);
}

#[cfg(test)]
mod bundle_version_tests {
    /// The bundle carries its own version, independent of Cargo's. A release
    /// that ships them out of step reports one version in Finder's Get Info
    /// and another in `--version`, and there is nothing at runtime to catch
    /// it, so pin them here.
    #[test]
    fn the_bundle_version_matches_the_crate_version() {
        let plist = include_str!("../../../bundle/Info.plist");
        let key = "<key>CFBundleShortVersionString</key><string>";
        let start = plist.find(key).expect("no CFBundleShortVersionString") + key.len();
        let end = start + plist[start..].find('<').expect("unterminated version");
        assert_eq!(
            &plist[start..end],
            env!("CARGO_PKG_VERSION"),
            "bundle/Info.plist and Cargo.toml disagree about the version"
        );
    }
}
