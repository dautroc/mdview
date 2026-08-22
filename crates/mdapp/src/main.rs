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
        match arg.to_string_lossy().as_ref() {
            "--print-html" => print_html = true,
            "--theme" => want_theme = true,
            other if want_theme => {
                theme = mdcore::Theme::from_wire(other);
                want_theme = false;
            }
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
