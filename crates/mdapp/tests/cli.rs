use std::path::PathBuf;
use std::process::Command;

fn fixture_file(name: &str, contents: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mdview-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn print_html_emits_a_complete_document() {
    let path = fixture_file("doc.md", "# Title\n\nSome *text*.\n");

    let output = Command::new(env!("CARGO_BIN_EXE_mdview"))
        .arg("--print-html")
        .arg(&path)
        .output()
        .expect("failed to run mdview");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let html = String::from_utf8(output.stdout).unwrap();
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("<h1>Title</h1>"));
    assert!(html.contains("<title>doc.md</title>"));
}

#[test]
fn print_html_on_a_missing_file_exits_one() {
    let output = Command::new(env!("CARGO_BIN_EXE_mdview"))
        .arg("--print-html")
        .arg("/nonexistent/nope.md")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("nope.md"));
}

#[test]
fn print_html_without_a_path_exits_two() {
    let output = Command::new(env!("CARGO_BIN_EXE_mdview"))
        .arg("--print-html")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
}

/// mdcore emits several tokens — CSS classes, element ids, a global JS
/// function name — that mdapp's own Rust code and the bundled `init.js`
/// depend on by name, with nothing else in the test suite checking that the
/// two sides still agree. This exercises the whole pipeline through the
/// actual binary (the seam that spans the crate boundary) with a document
/// that hits every one of those tokens, so a rename on either side that
/// breaks the contract fails a test instead of failing silently at runtime.
#[test]
fn rendered_html_carries_every_token_mdapp_js_depends_on() {
    let path = fixture_file(
        "contract.md",
        "# Contract\n\
         \n\
         Inline math $x^2$ and a display block:\n\
         \n\
         $$\n\
         E = mc^2\n\
         $$\n\
         \n\
         ```mermaid\n\
         graph TD;\n\
         A --> B;\n\
         ```\n\
         \n\
         ```rust\n\
         fn main() {}\n\
         ```\n\
         \n\
         | a | b |\n\
         |---|---|\n\
         | 1 | 2 |\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_mdview"))
        .arg("--print-html")
        .arg(&path)
        .output()
        .expect("failed to run mdview");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let html = String::from_utf8(output.stdout).unwrap();

    // The six tokens the mdcore/mdapp contract spans. The CSS-selector-style
    // spelling (".math-inline", "pre.mermaid") is how the bundled `init.js`
    // actually references them (`querySelectorAll`, `mermaid.run`'s
    // `querySelector`); the id attributes are how `window.rs` on the mdapp
    // side looks them up (`getElementById`).
    assert!(html.contains(".math-inline"), "missing .math-inline token: {html}");
    assert!(html.contains(".math-display"), "missing .math-display token: {html}");
    assert!(html.contains("pre.mermaid"), "missing pre.mermaid token: {html}");
    assert!(html.contains("id=\"mdview-content\""), "missing #mdview-content: {html}");
    assert!(html.contains("id=\"mdview-banners\""), "missing #mdview-banners: {html}");
    assert!(html.contains("window.mdviewRenderAll"), "missing window.mdviewRenderAll: {html}");

    // Self-contained: no external stylesheet, no externally-sourced script.
    assert_eq!(html.matches("<link").count(), 0, "no external stylesheets: {html}");
    assert_eq!(html.matches("src=\"http").count(), 0, "no external scripts: {html}");
}
