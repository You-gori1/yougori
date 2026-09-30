//! Shared by the setup button and standalone CLI. Only known, unmodified
//! Yougori-generated files may be updated; personal edits are never replaced.
use crate::{GUIDE, SKILL};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MARKER: &str = ".yougori-install.json";
const PREVIOUS_MARKER: &str = ".opendock-install.json";
const LOCATION: &str = "\n## Installed CLI location\n\nThe local executable is `";
// Used only to recognize pre-rename, marker-free managed skills.
const LOCATION_END: &str = "`. Invoke this absolute path if `opendock-cli` is not on PATH.\n";
const LEGACY_SKILL: &str = "61b3759b99b127c3db983dc019107f42237a0aeb74a32e85a1b16fb2bdb186b4";
const LEGACY_GUIDE: &str = "b4abd50bca2d5e4bb482b8c06351ea8a66351d484ee5d08676bb66c31dd9239c";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillStatus {
    pub state: &'static str,
    pub path: PathBuf,
    pub message: String,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    files: BTreeMap<String, String>,
    // A journal allows retrying an interrupted update without treating partially
    // installed, known bytes as user changes. Unknown bytes still fail closed.
    #[serde(default)]
    previous: BTreeMap<String, String>,
}

pub fn default_directory() -> Result<PathBuf, String> {
    let home = PathBuf::from(
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .filter(|v| !v.is_empty())
            .ok_or("Cannot locate this user's home directory")?,
    );
    workspace_skill_directory(&home)
}

fn workspace_skill_directory(home: &Path) -> Result<PathBuf, String> {
    if !home.is_absolute() {
        return Err("The user's home directory must be absolute".into());
    }
    let root = home.join("Yougori").join("Workspace");
    Ok(root.join("skills/yougori"))
}

fn generated(cli: &Path) -> BTreeMap<String, String> {
    let location = cli.display().to_string();
    let delimiter = "`".repeat(
        location
            .split(|c| c != '`')
            .map(str::len)
            .max()
            .unwrap_or(0)
            + 1,
    );
    let suffix = format!(
        "\n## Installed CLI location\n\nThe local executable is {delimiter}{location}{delimiter}. Invoke this absolute path if `yougori` is not on PATH.\n"
    );
    BTreeMap::from([
        ("SKILL.md".into(), format!("{SKILL}{suffix}")),
        ("references/cli.md".into(), GUIDE.into()),
    ])
}
pub fn instructions(cli: &Path) -> String {
    format!("{}\n{}", generated(cli)["SKILL.md"], GUIDE)
}
fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
fn read(path: &Path) -> Result<Option<String>, String> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Read {}: {e}", path.display())),
    };
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 512 * 1024 {
        return Err(format!(
            "Leave {} unchanged: it is not a regular, bounded skill file",
            path.display()
        ));
    }
    let mut text = String::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(512 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() > 512 * 1024 {
        return Err("Skill file is too large".into());
    }
    Ok(Some(text))
}
fn check_directory(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if !m.is_dir() || m.file_type().is_symlink() => Err(format!(
            "{} must be a regular directory; nothing was changed",
            path.display()
        )),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

fn known_files(path: &Path, files: &BTreeMap<String, String>) -> Result<bool, String> {
    let marker = match read(&path.join(MARKER))? {
        Some(value) => Some(value),
        None => read(&path.join(PREVIOUS_MARKER))?,
    };
    if let Some(marker) = marker {
        let manifest: Manifest = serde_json::from_str(&marker)
            .map_err(|_| "Invalid Yougori skill marker; existing files were left unchanged")?;
        if manifest.version != 1
            || manifest.files.len() != 2
            || !["SKILL.md", "references/cli.md"]
                .iter()
                .all(|name| manifest.files.contains_key(*name))
        {
            return Ok(false);
        }
        return Ok(files.iter().all(|(name, data)| {
            let hash = digest(data);
            manifest.files.get(name) == Some(&hash) || manifest.previous.get(name) == Some(&hash)
        }));
    }
    // Recognize the previous shipped, marker-free installer, including its
    // original executable path. This does not adopt arbitrary custom skills.
    Ok(files.len() == 2
        && files
            .get("SKILL.md")
            .and_then(|s| s.rsplit_once(LOCATION))
            .is_some_and(|(body, suffix)| {
                digest(body) == LEGACY_SKILL
                    && suffix.strip_suffix(LOCATION_END).is_some_and(|location| {
                        !location.is_empty()
                            && !location.contains(['`', '\n', '\r'])
                            && Path::new(location).is_absolute()
                    })
            })
        && files
            .get("references/cli.md")
            .is_some_and(|s| digest(s) == LEGACY_GUIDE))
}
fn existing(path: &Path) -> Result<BTreeMap<String, String>, String> {
    check_directory(path)?;
    check_directory(&path.join("references"))?;
    let mut files = BTreeMap::new();
    for name in ["SKILL.md", "references/cli.md"] {
        if let Some(text) = read(&path.join(name))? {
            files.insert(name.into(), text);
        }
    }
    Ok(files)
}
pub fn status_at(path: &Path, cli: &Path) -> Result<SkillStatus, String> {
    let files = existing(path)?;
    let (state, message) = if !path.exists() {
        (
            "missing",
            "Install the Yougori skill in your workspace. No administrator access needed.",
        )
    } else if files == generated(cli) {
        (
            "ready",
            "Yougori skill is ready. Ask your agent to read the SKILL.md file at this location.",
        )
    } else if known_files(path, &files)? {
        (
            "updateAvailable",
            "Update the Yougori-managed skill for this version. Personal edits are preserved.",
        )
    } else {
        ("conflict", "An existing custom or incomplete skill was left unchanged. Move or rename that skill before setting up Yougori access.")
    };
    Ok(SkillStatus {
        state,
        path: path.into(),
        message: message.into(),
    })
}
fn replace_file(path: &Path, text: &str) -> Result<(), String> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().ok_or("Missing skill directory")?)
        .map_err(|e| e.to_string())?;
    file.write_all(text.as_bytes())
        .and_then(|_| file.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    file.persist(path)
        .map_err(|e| format!("Save {}: {e}", path.display()))?;
    Ok(())
}
pub fn install_at(path: &Path, cli: &Path) -> Result<SkillStatus, String> {
    if !cli.is_absolute() || !cli.is_file() {
        return Err("The bundled Yougori CLI is missing; reinstall Yougori".into());
    }
    let status = status_at(path, cli)?;
    if status.state == "conflict" {
        return Err(format!("{} ({})", status.message, path.display()));
    }
    if status.state == "ready" {
        return Ok(status);
    }
    let before = existing(path)?;
    let files = generated(cli);
    std::fs::create_dir_all(path.join("references")).map_err(|e| e.to_string())?;
    check_directory(path)?;
    check_directory(&path.join("references"))?;
    if existing(path)? != before {
        return Err("Skill files changed during setup. Retry after reviewing them.".into());
    }
    let mut manifest = Manifest {
        version: 1,
        files: files.iter().map(|(k, v)| (k.clone(), digest(v))).collect(),
        previous: before.iter().map(|(k, v)| (k.clone(), digest(v))).collect(),
    };
    replace_file(
        &path.join(MARKER),
        &serde_json::to_string(&manifest).map_err(|e| e.to_string())?,
    )?;
    for (name, text) in &files {
        replace_file(&path.join(name), text)?;
    }
    manifest.previous.clear();
    replace_file(
        &path.join(MARKER),
        &serde_json::to_string(&manifest).map_err(|e| e.to_string())?,
    )?;
    status_at(path, cli)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skill_lives_in_the_shared_workspace() {
        let home = tempfile::tempdir().unwrap();
        let path = workspace_skill_directory(home.path()).unwrap();
        assert_eq!(path, home.path().join("Yougori/Workspace/skills/yougori"));
        assert!(!path.exists(), "Looking up the path must not create files");
        assert!(workspace_skill_directory(Path::new("relative")).is_err());
    }
    #[test]
    fn setup_is_idempotent_and_custom_changes_are_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("opendock");
        let cli = std::env::current_exe().unwrap();
        assert_eq!(status_at(&path, &cli).unwrap().state, "missing");
        assert_eq!(install_at(&path, &cli).unwrap().state, "ready");
        assert_eq!(install_at(&path, &cli).unwrap().state, "ready");
        std::fs::write(path.join("SKILL.md"), "Personal skill").unwrap();
        assert_eq!(status_at(&path, &cli).unwrap().state, "conflict");
        assert!(install_at(&path, &cli).is_err());
        assert_eq!(
            std::fs::read_to_string(path.join("SKILL.md")).unwrap(),
            "Personal skill"
        );
    }
    #[test]
    fn managed_skills_follow_a_relocated_cli_but_unknown_directories_stay_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("opendock");
        let cli = std::env::current_exe().unwrap();
        install_at(&path, &cli).unwrap();
        let relocated = dir.path().join("new-cli.exe");
        std::fs::write(&relocated, b"fixture").unwrap();
        assert_eq!(
            status_at(&path, &relocated).unwrap().state,
            "updateAvailable"
        );
        assert_eq!(install_at(&path, &relocated).unwrap().state, "ready");
        let custom = dir.path().join("custom");
        std::fs::create_dir(&custom).unwrap();
        assert!(install_at(&custom, &cli).is_err());
        assert_eq!(std::fs::read_dir(custom).unwrap().count(), 0);
    }

    #[test]
    fn interrupted_managed_updates_are_retryable_but_modified_guides_are_not() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("opendock");
        let cli = std::env::current_exe().unwrap();
        install_at(&path, &cli).unwrap();
        std::fs::remove_file(path.join("references/cli.md")).unwrap();
        assert_eq!(install_at(&path, &cli).unwrap().state, "ready");
        std::fs::write(path.join("references/cli.md"), "Personal instructions").unwrap();
        assert!(install_at(&path, &cli).is_err());
        assert_eq!(
            std::fs::read_to_string(path.join("references/cli.md")).unwrap(),
            "Personal instructions"
        );
    }
    #[test]
    fn guide_contains_the_exact_cli_location_and_host_permission_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let cli = dir.path().join("CLI `with` spaces.exe");
        let guide = instructions(&cli);
        assert!(guide.contains(&format!("``{}``", cli.display())));
        assert!(guide.contains("Host terminal"));
        assert!(guide.contains("host-task authorization"));
        assert!(guide.contains("Raw API and execution"));
    }
}
