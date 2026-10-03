//! Real CLI processes with an isolated home; never use personal preferences.
use serde_json::Value;
use std::process::{Command, Output};

fn run(home: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_yougori"))
        .args(args)
        .current_dir(home)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .output()
        .unwrap()
}

#[test]
fn unreadable_preferences_are_not_replaced_by_a_new_template() {
    for action in [vec!["prefs", "remember", "compute", "local"], vec!["prefs", "forget", "compute"]] {
        let home = tempfile::tempdir().unwrap();
        let folder = home.path().join(".yougori");
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("PREFERENCES.md");
        let original = b"personal bytes\n\xff\xfe\n";
        std::fs::write(&path, original).unwrap();
        let output = run(home.path(), &action);
        assert!(!output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["ok"], false);
        assert!(response["error"].as_str().unwrap().contains("Cannot read preferences"));
        assert_eq!(std::fs::read(path).unwrap(), original);
    }
}

#[test]
fn missing_preferences_can_be_created_without_losing_personal_text_on_later_writes() {
    let home = tempfile::tempdir().unwrap();
    let first = run(home.path(), &["prefs", "remember", "compute", "local"]);
    assert!(first.status.success(), "{}", String::from_utf8_lossy(&first.stdout));
    let path = home.path().join(".yougori/PREFERENCES.md");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("- compute: local"));
    std::fs::write(&path, format!("Personal café 日本\n\n{text}")).unwrap();
    let second = run(home.path(), &["prefs", "remember", "compute", "local"]);
    assert!(second.status.success(), "{}", String::from_utf8_lossy(&second.stdout));
    let forgotten = run(home.path(), &["prefs", "forget", "compute"]);
    assert!(forgotten.status.success(), "{}", String::from_utf8_lossy(&forgotten.stdout));
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.starts_with("Personal café 日本\n"));
    assert!(!text.contains("- compute:"));
}

#[test]
fn a_directory_at_the_preferences_path_is_an_error_not_a_missing_file() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(".yougori/PREFERENCES.md");
    std::fs::create_dir_all(&path).unwrap();
    let output = run(home.path(), &["prefs", "remember", "compute", "local"]);
    assert!(!output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(response["error"].as_str().unwrap().contains("Cannot read preferences"));
    assert!(path.is_dir());
}
