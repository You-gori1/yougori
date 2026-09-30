//! Yougori's own project files (`yougori init`) and the preferences file agents read (`yougori prefs`).
//! Preferences only pre-fill suggestions: agents still ask before every decision.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const IGNORE_TEMPLATE: &str = "# Files Yougori leaves behind when this folder is copied into an environment.
# Same syntax as .gitignore. Delete a line to include those files again.
.git/
node_modules/
.venv/
venv/
__pycache__/
dist/
build/
target/
.next/
*.log
# Secrets stay on this PC unless you choose otherwise.
.env
.env.*
*.pem
*.key
*.p12
";

pub const PREFERENCES_TEMPLATE: &str = "# Yougori preferences

Suggestions for agents. Always confirm with the user before acting: preferences pre-fill the
question, they never replace it. Always ask, every time, before anything billable, public,
destructive (delete, reset, restore), involving credentials, PC folder access, or Full control
sharing. Never write passwords, tokens or API keys here.

Update with `yougori prefs remember KEY VALUE` after the user confirms a choice they want remembered.

## Remembered
";

fn yaml_template(name: &str) -> String {
    format!("# Yougori project. Start it with `yougori up`, stop it with `yougori down`.
project: {name}
environments:
  app:
    image: docker.io/library/ubuntu:24.04
    cpu: 2
    memory: 4GB
    internet: true
    working_dir: /workspace
    volumes:
      - {{source: {name}-workspace, target: /workspace}}
    # Share this folder with the environment (grants PC access; add permissions: {{pc: true}}):
    #   - {{source: ./, target: /workspace, bind: true, read_only: true}}
    # ports: [\"3000:3000\"]
")
}

fn project_name(folder: &Path) -> String {
    let raw = folder.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let name = raw.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '-' }).collect::<String>();
    let name = name.trim_matches(|c: char| !c.is_ascii_alphanumeric()).chars().take(40).collect::<String>();
    if name.is_empty() { "my-project".into() } else { name }
}

fn inside_git(folder: &Path) -> bool {
    folder.ancestors().any(|dir| dir.join(".git").exists())
}

/// Creates missing project files; never overwrites anything.
pub fn init(folder: &Path, name: Option<&str>) -> Result<Value, String> {
    let name = name.map(str::to_owned).unwrap_or_else(|| project_name(folder));
    if !crate::workload::identifier(&name) || name.len() > 40 {
        return Err("Project name must use 1–40 letters, digits, dots, dashes or underscores".into());
    }
    let mut created = Vec::new();
    let mut kept = Vec::new();
    let mut write = |relative: &str, content: &str| -> Result<(), String> {
        let path = folder.join(relative);
        if path.exists() {
            kept.push(relative.to_owned());
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, content).map_err(|e| format!("Cannot write {relative}: {e}"))?;
        created.push(relative.to_owned());
        Ok(())
    };
    let compose = ["compose.yaml", "compose.yml", "docker-compose.yml", "docker-compose.yaml"].into_iter().find(|f| folder.join(f).exists());
    if compose.is_none() {
        write("yougori.yaml", &yaml_template(&name))?;
    }
    write(".yougoriignore", IGNORE_TEMPLATE)?;
    write(".yougori/PREFERENCES.md", PREFERENCES_TEMPLATE)?;
    if inside_git(folder) {
        let path = folder.join(".gitignore");
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        let missing = [".yougori/local.yaml", ".yougori/state.json"].into_iter().filter(|line| !current.lines().any(|l| l.trim() == *line)).collect::<Vec<_>>();
        if !missing.is_empty() {
            let separator = if current.is_empty() || current.ends_with('\n') { "" } else { "\n" };
            std::fs::write(&path, format!("{current}{separator}# Yougori: machine-specific files\n{}\n", missing.join("\n"))).map_err(|e| e.to_string())?;
            created.push(".gitignore (Yougori entries)".into());
        }
    }
    let mut next = vec!["Edit yougori.yaml, then run `yougori up`.".to_owned()];
    if let Some(compose) = compose {
        next = vec![format!("Found {compose}: run `yougori import {compose}` to create yougori.yaml from it.")];
    }
    next.push("Review .yougoriignore before copying this folder into an environment.".into());
    Ok(json!({"folder": folder, "project": name, "created": created, "kept": kept, "next": next}))
}

/// `~/.yougori/PREFERENCES.md` and the nearest project's `.yougori/PREFERENCES.md`.
pub fn preference_files(cwd: &Path) -> (Option<PathBuf>, Option<PathBuf>) {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from);
    let user = home.map(|h| h.join(".yougori").join("PREFERENCES.md"));
    let project = cwd.ancestors().find(|dir| dir.join(".yougori").is_dir() || dir.join("yougori.yaml").is_file())
        .map(|dir| dir.join(".yougori").join("PREFERENCES.md"));
    (user, project)
}

fn looks_secret(key: &str, value: &str) -> bool {
    let key = key.to_ascii_lowercase().replace(['_', '-', ' '], "");
    let sensitive = ["token", "password", "passwd", "secret", "apikey", "credential", "privatekey"].iter().any(|word| key.contains(word));
    let compact = value.trim();
    let random = compact.len() >= 24 && !compact.contains(' ') && compact.chars().any(|c| c.is_ascii_digit()) && compact.chars().any(|c| c.is_ascii_alphabetic());
    sensitive || random
}

/// Adds or updates `- KEY: VALUE (confirmed N times, last DATE)` under `## Remembered`.
pub fn remember(text: &str, key: &str, value: &str, today: &str) -> Result<String, String> {
    let key = key.trim();
    let value = value.trim();
    if key.is_empty() || key.len() > 80 || key.contains([':', '\n']) || value.is_empty() || value.len() > 300 || value.contains('\n') {
        return Err("Use a short key without ':' and a one-line value".into());
    }
    if looks_secret(key, value) {
        return Err("That looks like a secret. Preferences never store passwords, tokens or keys.".into());
    }
    let mut text = if text.contains("## Remembered") { text.to_owned() } else { format!("{}\n{}", text.trim_end(), "\n## Remembered\n").trim_start().to_owned() };
    let prefix = format!("- {key}: ");
    let mut lines = text.lines().map(str::to_owned).collect::<Vec<_>>();
    let count = |line: &str| line.rsplit_once("(confirmed ").and_then(|(_, rest)| rest.split_whitespace().next()?.parse::<u32>().ok()).unwrap_or(1);
    let existing = lines.iter().position(|line| line.starts_with(&prefix));
    let entry = |times: u32| format!("{prefix}{value} (confirmed {times} {}, last {today})", if times == 1 { "time" } else { "times" });
    match existing {
        Some(index) => {
            let same = lines[index][prefix.len()..].split(" (confirmed ").next() == Some(value);
            lines[index] = entry(if same { count(&lines[index]) + 1 } else { 1 });
        }
        None => {
            let section = lines.iter().position(|l| l.trim() == "## Remembered").unwrap_or(lines.len());
            let end = lines[section + 1..].iter().position(|l| l.starts_with("## ")).map_or(lines.len(), |i| section + 1 + i);
            lines.insert(end, entry(1));
        }
    }
    text = lines.join("\n");
    text.push('\n');
    Ok(text)
}

pub fn forget(text: &str, key: &str) -> Option<String> {
    let prefix = format!("- {}: ", key.trim());
    let kept = text.lines().filter(|line| !line.starts_with(&prefix)).collect::<Vec<_>>();
    (kept.len() != text.lines().count()).then(|| kept.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn init_creates_valid_files_once_and_never_overwrites() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("My Shop!");
        std::fs::create_dir_all(folder.join(".git")).unwrap();
        std::fs::write(folder.join(".gitignore"), "node_modules/").unwrap();
        let first = init(&folder, None).unwrap();
        assert_eq!(first["project"], "my-shop");
        assert_eq!(first["created"].as_array().unwrap().len(), 4);
        let yaml = std::fs::read_to_string(folder.join("yougori.yaml")).unwrap();
        crate::manifest::parse(&yaml).expect("the generated yougori.yaml is valid");
        let gitignore = std::fs::read_to_string(folder.join(".gitignore")).unwrap();
        assert!(gitignore.starts_with("node_modules/\n") && gitignore.contains(".yougori/local.yaml"));
        std::fs::write(folder.join("yougori.yaml"), "mine").unwrap();
        let second = init(&folder, None).unwrap();
        assert!(second["created"].as_array().unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(folder.join("yougori.yaml")).unwrap(), "mine");
        assert_eq!(std::fs::read_to_string(folder.join(".gitignore")).unwrap(), gitignore);
    }
    #[test]
    fn compose_projects_are_pointed_to_import() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("compose.yaml"), "services: {}").unwrap();
        let result = init(root.path(), Some("shop")).unwrap();
        assert!(!root.path().join("yougori.yaml").exists());
        assert!(result["next"][0].as_str().unwrap().contains("yougori import compose.yaml"));
    }
    #[test]
    fn preferences_count_confirmations_and_refuse_secrets() {
        let text = remember(PREFERENCES_TEMPLATE, "Public access", "Quick link", "2026-09-26").unwrap();
        assert!(text.contains("- Public access: Quick link (confirmed 1 time, last 2026-09-26)"));
        let text = remember(&text, "Public access", "Quick link", "2026-09-27").unwrap();
        assert!(text.contains("- Public access: Quick link (confirmed 2 times, last 2026-09-27)"));
        let text = remember(&text, "Public access", "crm.example.com", "2026-09-28").unwrap();
        assert!(text.contains("- Public access: crm.example.com (confirmed 1 time, last 2026-09-28)"));
        let text = remember(&text, "Environment kind", "container", "2026-09-28").unwrap();
        assert_eq!(text.matches("- ").count() - PREFERENCES_TEMPLATE.matches("- ").count(), 2);
        assert!(remember(&text, "API token", "abc", "d").is_err());
        assert!(remember(&text, "Domain", "cfut_8f3kLq92mZpX0aR7vN4sT1yB6", "d").is_err());
        let text = forget(&text, "Public access").unwrap();
        assert!(!text.contains("Public access") && text.contains("Environment kind"));
        assert!(forget(&text, "missing").is_none());
        assert!(remember("", "Kind", "gpu", "d").unwrap().contains("## Remembered\n- Kind: gpu"));
    }
}
