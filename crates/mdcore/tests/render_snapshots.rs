use std::path::Path;

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("missing fixture {}: {e}", path.display()))
}

#[test]
fn basic_commonmark() {
    insta::assert_snapshot!(mdcore::render::render_body(&fixture("basic.md")));
}

#[test]
fn gfm_tables_tasks_strikethrough_footnotes() {
    insta::assert_snapshot!(mdcore::render::render_body(&fixture("gfm.md")));
}

#[test]
fn code_blocks_and_mermaid() {
    insta::assert_snapshot!(mdcore::render::render_body(&fixture("code.md")));
}

#[test]
fn inline_and_display_math() {
    insta::assert_snapshot!(mdcore::render::render_body(&fixture("math.md")));
}

#[test]
fn relative_and_remote_image_paths_are_preserved() {
    let html = mdcore::render::render_body(&fixture("images.md"));
    // Paths must pass through untouched; the web view's baseURL resolves them.
    assert!(html.contains("src=\"./img/diagram.png\""), "got: {html}");
    assert!(html.contains("src=\"../shared/logo.png\""), "got: {html}");
    insta::assert_snapshot!(html);
}

/// Frontmatter is stripped, and the rule further down the document -- which
/// is the same three characters -- is not.
#[test]
fn frontmatter_is_stripped_and_later_rules_are_kept() {
    let html = mdcore::render::render_body(&fixture("frontmatter.md"));
    assert!(!html.contains("tags:"), "metadata leaked into the body: {html}");
    assert!(html.starts_with("<h1>"), "got: {html}");
    insta::assert_snapshot!(html);
}

#[test]
fn empty_file_renders_an_empty_body() {
    assert_eq!(mdcore::render::render_body(&fixture("empty.md")), "");
}
