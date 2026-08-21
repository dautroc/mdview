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
