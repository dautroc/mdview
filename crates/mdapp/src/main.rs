mod app;
mod menu;
mod navigation;
mod state;
mod watcher;
mod window;

use std::path::PathBuf;

fn main() {
    let mut print_html = false;
    let mut paths: Vec<PathBuf> = Vec::new();

    for arg in std::env::args_os().skip(1) {
        match arg.to_string_lossy().as_ref() {
            "--print-html" => print_html = true,
            "--version" => {
                println!("mdview {}", mdcore::version());
                return;
            }
            "--help" | "-h" => {
                println!("usage: mdview [--print-html] [FILE...]");
                return;
            }
            _ => paths.push(PathBuf::from(arg)),
        }
    }

    if print_html {
        let Some(path) = paths.first() else {
            eprintln!("mdview: --print-html requires a file path");
            std::process::exit(2);
        };
        match mdcore::render_document(path) {
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
