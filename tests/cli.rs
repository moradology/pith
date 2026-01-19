use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

fn write_file(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

#[test]
fn cli_codemap_respects_pithignore_hidden_and_minified() {
    let dir = tempdir().unwrap();

    write_file(&dir.path().join("a.rs"), "pub fn a() -> i32 { 1 }\n");
    write_file(
        &dir.path().join("ignored.rs"),
        "pub fn ignored() -> i32 { 2 }\n",
    );
    write_file(
        &dir.path().join(".hidden.rs"),
        "pub fn hidden() -> i32 { 3 }\n",
    );
    write_file(
        &dir.path().join("bundle.min.js"),
        "export function minified() { return 1 }\n",
    );

    write_file(&dir.path().join(".pithignore"), "ignored.rs\n");

    let output = Command::new(env!("CARGO_BIN_EXE_pith"))
        .args(["codemap", dir.path().to_str().unwrap(), "--json"])
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let codemaps = v.get("codemaps").and_then(|c| c.as_array()).unwrap();

    let paths: Vec<String> = codemaps
        .iter()
        .map(|c| c.get("path").unwrap().as_str().unwrap().to_string())
        .collect();

    assert!(paths.iter().any(|p| p.ends_with("a.rs")));
    assert!(!paths.iter().any(|p| p.ends_with("ignored.rs")));
    assert!(!paths.iter().any(|p| p.ends_with(".hidden.rs")));
    assert!(!paths.iter().any(|p| p.ends_with("bundle.min.js")));
}

#[test]
fn cli_tokens_respects_pithignore_hidden_and_minified() {
    let dir = tempdir().unwrap();

    write_file(&dir.path().join("a.rs"), "pub fn a() {}\n");
    write_file(&dir.path().join("ignored.rs"), "pub fn ignored() {}\n");
    write_file(&dir.path().join(".hidden.rs"), "pub fn hidden() {}\n");
    write_file(
        &dir.path().join("bundle.min.js"),
        "export function x() {}\n",
    );

    write_file(&dir.path().join(".pithignore"), "ignored.rs\n");

    let output = Command::new(env!("CARGO_BIN_EXE_pith"))
        .args([
            "tokens",
            dir.path().to_str().unwrap(),
            "--json",
            "--per-file",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let files = v.get("files").and_then(|f| f.as_object()).unwrap();

    assert!(files.contains_key("a.rs"));
    assert!(!files.contains_key("ignored.rs"));
    assert!(!files.contains_key(".hidden.rs"));
    assert!(!files.contains_key("bundle.min.js"));
}

#[test]
fn cli_json_error_output_is_valid_json_even_with_quotes_in_path() {
    let dir = tempdir().unwrap();

    let bad_path = dir.path().join("does-not-exist-\"quoted\"");

    let output = Command::new(env!("CARGO_BIN_EXE_pith"))
        .args(["codemap", bad_path.to_str().unwrap(), "--json"])
        .output()
        .unwrap();

    assert!(!output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    let _: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
}
