//! Real standalone process tests. These must never dispatch a mutation or use
//! the developer's personal skills directory, even if a desktop engine is open.
use serde_json::Value;
use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_yougori-cli"))
        .args(args)
        .output()
        .unwrap()
}
fn response(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn ssh_ascii_output_does_not_change_wire_json_or_user_text() {
    let help = Command::new(env!("CARGO_BIN_EXE_yougori"))
        .arg("help").env("YOUGORI_ASCII", "1").output().unwrap();
    assert!(help.status.success());
    assert!(help.stdout.is_ascii(), "{}", String::from_utf8_lossy(&help.stdout));
    let directory = tempfile::tempdir().unwrap();
    let skill = directory.path().join("café→日本");
    let args = ["skills", "install", "--path", skill.to_str().unwrap()];
    let normal = cli(&args);
    let ascii = Command::new(env!("CARGO_BIN_EXE_yougori"))
        .args(args)
        .env("YOUGORI_ASCII", "1").output().unwrap();
    assert_eq!(ascii.status.success(), normal.status.success());
    assert!(ascii.status.success(), "{}", String::from_utf8_lossy(&ascii.stdout));
    assert_eq!(ascii.stdout, normal.stdout);
    assert!(String::from_utf8_lossy(&ascii.stdout).contains("café→日本"));
}

#[test]
fn launcher_never_prompts_on_pipes_and_model_alias_preserves_resources() {
    for args in [vec!["launch"],vec!["launch", "--cloud"],vec!["run"],vec!["cli"],vec!["terminal", "unused-environment"],vec!["terminal", "unused-environment", "--project"]] {
        let output=cli(&args);
        assert!(!output.status.success());
        assert!(response(&output)["error"].as_str().unwrap().contains("interactive terminal"));
    }
    let output=cli(&["run","hf.co/TinyLlama/TinyLlama-1.1B-Chat-v1.0","--api","--cpu","3","--memory","6GB","--storage","30GB","--dry-run"]);
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stdout));
    let result=&response(&output)["result"];
    assert_eq!(result["gpu"],"nvidia");
    assert_eq!(result["resources"],serde_json::json!({"cpu":3.0,"memoryGb":6.0,"storageGb":30.0}));
    let invalid=cli(&["model","run","hf.co/a/b","--cpu","NaN","--dry-run"]);
    assert!(!invalid.status.success());
}

#[test]
fn neocloud_model_selection_is_explicit_for_scripts_and_dry_run_stays_offline() {
    let output = cli(&["model", "run", "hf.co/HuggingFaceTB/SmolLM2-135M", "--neocloud", "--dry-run"]);
    assert!(output.status.success());
    assert_eq!(response(&output)["result"]["neocloud"], true);
    let output = cli(&["model", "run", "hf.co/a/b", "--neocloud", "--environment", "my-pod", "--port", "8123", "--dry-run"]);
    assert!(output.status.success());
    assert_eq!(response(&output)["result"]["environment"], "my-pod");
    assert_eq!(response(&output)["result"]["api"], true);
    let missing = cli(&["model", "run", "hf.co/a/b", "--neocloud"]);
    assert!(!missing.status.success());
    assert!(response(&missing)["error"].as_str().unwrap().contains("--environment"));
    let invalid = cli(&["model", "run", "hf.co/a/b", "--neocloud", "--memory", "4", "--dry-run"]);
    assert!(!invalid.status.success());
    let help = cli(&["model", "run", "hf.co/a/b", "--neocloud", "--help"]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--neocloud"));
}

#[test]
fn bare_entry_opens_cli_and_help_stays_explicit() {
    let empty = tempfile::tempdir().unwrap();
    let bare = Command::new(env!("CARGO_BIN_EXE_yougori-cli")).current_dir(empty.path()).output().unwrap();
    assert!(!bare.status.success());
    assert_eq!(response(&bare), response(&cli(&["cli"])));
    assert!(response(&bare)["error"].as_str().unwrap().contains("yougori help"));
    for args in [["help"], ["--help"], ["-h"]] {
        let output = cli(&args);
        assert!(output.status.success());
        let help = String::from_utf8_lossy(&output.stdout);
        assert!(help.contains("yougori cli"));
        assert!(help.contains("yougori run"));
    }
    let guided = cli(&["cli", "--help"]);
    assert!(guided.status.success());
    assert!(String::from_utf8_lossy(&guided.stdout).contains("arrow keys"));
    let invalid = cli(&["cli", "unexpected"]);
    assert!(!invalid.status.success());
    assert!(response(&invalid)["error"].as_str().unwrap().contains("Usage: yougori cli"));
}

#[test]
fn bare_project_entry_keeps_the_menu_without_mutating_on_pipes() {
    for (name, content) in [("package.json", "{\"scripts\":{\"dev\":\"vite\"}}"), ("main.py", "print('hello')")] {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join(name), content).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_yougori"))
            .current_dir(folder.path()).output().unwrap();
        assert!(!output.status.success());
        assert_eq!(response(&output), response(&cli(&["cli"])));
        assert_eq!(std::fs::read_to_string(folder.path().join(name)).unwrap(), content);
        assert!(!folder.path().join("yougori").exists());
    }
}

#[test]
fn one_gib_container_is_validated_before_starting_the_engine() {
    for extra in [vec![],vec!["--gpu","nvidia"]] {
        let mut args=vec!["run","-d","--storage","1GB"];
        args.extend(extra);
        args.extend(["--dry-run","alpine:3.24"]);
        let output=cli(&args);
        assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stdout));
        assert_eq!(response(&output)["result"]["request"]["storageGb"],1.0);
    }
    let output=cli(&["run","-d","--storage","0GB","--dry-run","alpine:3.24"]);
    assert!(!output.status.success());
}

#[test]
fn offline_help_and_catalog_expose_all_features() {
    let help = cli(&["env", "create", "--help"]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("Yougori CLI"));
    assert!(!String::from_utf8_lossy(&help.stdout).contains("OpenDock"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("--startup image"));
    let version = cli(&["--version"]);
    assert!(version.status.success());
    assert!(String::from_utf8_lossy(&version.stdout).starts_with("yougori "));
    assert_eq!(yougori_cli::SKILL.lines().nth(1), Some("name: yougori"));
    let schema = cli(&["schema"]);
    assert!(schema.status.success());
    let parsed = response(&schema);
    assert_eq!(
        parsed["methods"].as_array().unwrap().len(),
        yougori_cli::catalog::methods().len()
    );
    for method in [
        "create_environment",
        "attach_host_folder",
        "publish_environment_service",
        "verify_environment_cuda",
    ] {
        assert!(parsed["methods"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["name"] == method));
    }
}

#[test]
fn missing_confirmation_fails_before_connecting() {
    for command in [
        vec!["env", "delete", "test-id-never-dispatched"],
        vec!["rm", "test-id-never-dispatched"],
        vec!["volume", "rm", "test-volume-never-dispatched"],
        vec!["model", "usage", "test-id-never-dispatched", "--reset"],
    ] {
        let output = cli(&command);
        assert!(!output.status.success(), "{command:?}");
        let parsed = response(&output);
        assert_eq!(parsed["ok"], false, "{command:?}");
        assert!(parsed["error"].as_str().unwrap().contains("--yes"), "{command:?}: {parsed}");
    }
}

#[test]
fn file_and_stdin_json_accept_bom_and_reject_oversized_input() {
    use std::io::Write;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("request.json");
    let bytes = b"\xef\xbb\xbf{\"environmentId\":\"test-id-never-dispatched\"}";
    std::fs::write(&path, bytes).unwrap();
    let output = cli(&[
        "call",
        "delete_environment",
        "--file",
        path.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert!(response(&output)["error"]
        .as_str()
        .unwrap()
        .contains("--yes"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_yougori-cli"))
        .args(["call", "delete_environment", "--file", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(response(&output)["error"]
        .as_str()
        .unwrap()
        .contains("--yes"));
    std::fs::write(&path, vec![b' '; yougori_cli::wire::MAX_REQUEST + 1]).unwrap();
    let output = cli(&[
        "call",
        "delete_environment",
        "--file",
        path.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert!(response(&output)["error"]
        .as_str()
        .unwrap()
        .contains("exceeds"));
}

#[test]
fn skill_install_is_idempotent_and_never_overwrites_personal_edits() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("opendock");
    let output = cli(&["skills", "install", "--path", path.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let skill = std::fs::read_to_string(path.join("SKILL.md")).unwrap();
    assert!(skill.starts_with(yougori_cli::SKILL));
    assert!(skill.contains(env!("CARGO_BIN_EXE_yougori-cli")));
    assert_eq!(
        std::fs::read_to_string(path.join("references/cli.md")).unwrap(),
        yougori_cli::GUIDE
    );
    let again = cli(&["skills", "install", "--path", path.to_str().unwrap()]);
    assert!(again.status.success());
    assert_eq!(
        std::fs::read_to_string(path.join("SKILL.md")).unwrap(),
        skill
    );
    let edited = format!("{skill}\nPersonal project instructions\n");
    std::fs::write(path.join("SKILL.md"), &edited).unwrap();
    let conflict = cli(&["skills", "install", "--path", path.to_str().unwrap()]);
    assert!(!conflict.status.success());
    assert_eq!(std::fs::read_to_string(path.join("SKILL.md")).unwrap(), edited);
}
