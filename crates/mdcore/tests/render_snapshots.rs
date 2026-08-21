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
