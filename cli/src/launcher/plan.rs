//! What the container needs before a project's command can work, read from the PC, where the
//! project already runs. Package folders installed on the PC or used by the command's scripts are
//! installed in the container too; global tools the command relies on are installed globally;
//! steps that cannot work on Linux are found before launch, with replacement commands taken from
//! the project's own scripts. Nothing here guesses: every decision rests on a file on the PC.
use super::{
    package::{self, Manager, Project},
    scripts::{self, Join, Packages, Step},
    sync::{self, ignore_rules::Rules},
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

mod recommend;

#[derive(Debug)]
pub struct RunChoice {
    pub command: String,
    pub detail: String,
    pub recommended: bool,
}

/// One install the container runs before the command, in `folder` ("." is the project).
#[derive(Clone, Debug, PartialEq)]
pub struct Install {
    pub folder: String,
    /// Exists once installed: relative to `folder`, or absolute.
    pub marker: String,
    /// The project's own dependencies must install; other folders and tools are best effort.
    pub required: bool,
    pub label: String,
    pub command: String,
}

pub fn installs(root: &Path, project: &Project, files: &sync::Manifest) -> Vec<Install> {
    if project.is_python() {
        return vec![Install { folder: ".".into(), marker: "/yougori/venv".into(), required: true, label: "Python packages".into(), command: project.install().into() }];
    }
    let mut packages = Packages::new(root);
    let expansion = scripts::expand(&mut packages, ".", &project.command_label());
    let mut installs = vec![Install { folder: ".".into(), marker: "node_modules".into(), required: true, label: "dependencies".into(), command: node_install(project.manager) }];
    for folder in folders(root, &mut packages, files, &expansion) {
        let manager = package::manager(&root.join(&folder), packages.get(&folder).unwrap_or(&Value::Null));
        installs.push(Install { label: format!("{folder}/ dependencies"), marker: "node_modules".into(), required: false, command: node_install(manager), folder });
    }
    installs.extend(tools(root, project, &mut packages, &expansion));
    installs
}

fn node_install(manager: Manager) -> String {
    match manager {
        Manager::Npm => format!("{} && node /yougori/launch/prepare.cjs optional", manager.install()),
        other => other.install().into(),
    }
}

/// The dev.env value: one tab-separated line per install (see dev.sh).
pub fn lines(installs: &[Install]) -> String {
    installs
        .iter()
        .map(|i| format!("{}\t{}\t{}\t{}\t{}", i.folder, i.marker, u8::from(i.required), i.label.replace(']', ")"), i.command))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Installs beyond the project's own dependencies, for the launch summary.
pub fn describe(installs: &[Install]) -> Option<String> {
    let extra: Vec<&str> = installs.iter().skip(1).map(|i| i.label.trim_end_matches(" dependencies")).collect();
    (!extra.is_empty()).then(|| format!("Also installs {}", extra.join(", ")))
}

/// Nested package folders the container installs: those installed on the PC, or used by the
/// project's scripts. Workspace members install with their workspace root, and folders the
/// project's own install scripts install are left to them.
fn folders(root: &Path, packages: &mut Packages, files: &sync::Manifest, expansion: &scripts::Expansion) -> Vec<String> {
    let mut candidates: Vec<String> = files
        .keys()
        .filter_map(|path| path.strip_suffix("/package.json"))
        .filter(|dir| !dir.chars().any(char::is_control) && !dir.split('/').any(|p| matches!(p, "node_modules" | ".git" | ".venv" | "venv")))
        .map(str::to_owned)
        .collect();
    if candidates.is_empty() {
        return candidates;
    }
    candidates.sort_by_key(|dir| (dir.matches('/').count(), dir.clone()));
    let mut used: BTreeSet<String> = expansion.folders.union(&expansion.installs).cloned().collect();
    let mut handled = BTreeSet::new();
    let names: Vec<String> = packages.get(".").and_then(|p| p["scripts"].as_object()).map(|s| s.keys().cloned().collect()).unwrap_or_default();
    for name in names {
        let reached = scripts::expand(packages, ".", &format!("npm run {}", quote(&name)));
        if matches!(name.as_str(), "preinstall" | "install" | "postinstall" | "prepare") {
            handled.extend(reached.installs.iter().cloned());
        }
        used.extend(reached.folders);
        used.extend(reached.installs);
    }
    let mut roots: Vec<(String, Rules)> = members(root, ".", packages).map(|rules| (".".to_owned(), rules)).into_iter().collect();
    let mut chosen = Vec::new();
    for dir in candidates {
        let member = roots.iter().any(|(parent, rules)| inside(parent, &dir).is_some_and(|relative| rules.ignored(relative, true)));
        if member || handled.contains(&dir) || !(used.contains(&dir) || root.join(&dir).join("node_modules").is_dir()) {
            continue;
        }
        if let Some(rules) = members(root, &dir, packages) {
            roots.push((dir.clone(), rules));
        }
        chosen.push(dir);
    }
    chosen
}

fn inside<'a>(parent: &str, dir: &'a str) -> Option<&'a str> {
    if parent == "." { Some(dir) } else { dir.strip_prefix(parent)?.strip_prefix('/') }
}

/// The workspace members of the package in `dir` (npm, yarn and bun `workspaces`, pnpm-workspace.yaml).
fn members(root: &Path, dir: &str, packages: &mut Packages) -> Option<Rules> {
    let mut patterns: Vec<String> = Vec::new();
    if let Some(package) = packages.get(dir) {
        let list = match &package["workspaces"] {
            Value::Array(list) => Some(list),
            Value::Object(map) => map.get("packages").and_then(Value::as_array),
            _ => None,
        };
        patterns.extend(list.into_iter().flatten().filter_map(Value::as_str).map(str::to_owned));
    }
    if let Ok(text) = fs::read_to_string(root.join(dir).join("pnpm-workspace.yaml")) {
        if let Ok(yaml) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text) {
            patterns.extend(yaml.get("packages").and_then(|p| p.as_sequence()).into_iter().flatten().filter_map(|p| p.as_str()).map(str::to_owned));
        }
    }
    if patterns.is_empty() {
        return None;
    }
    let rules: Vec<String> = patterns
        .iter()
        .map(|pattern| {
            let (negate, pattern) = pattern.strip_prefix('!').map_or(("", pattern.as_str()), |rest| ("!", rest));
            format!("{negate}/{}", pattern.trim_start_matches("./").trim_end_matches('/'))
        })
        .collect();
    Some(Rules::parse(&rules.join("\n")))
}

/// Programs the command runs that the image lacks: package managers the scripts call, and tools
/// installed globally on the PC rather than in the project.
fn tools(root: &Path, project: &Project, packages: &mut Packages, expansion: &scripts::Expansion) -> Vec<Install> {
    let mut out = Vec::new();
    let tool = |label: &str, command: &str| Install { folder: ".".into(), marker: format!("/usr/local/bin/{label}"), required: false, label: label.into(), command: command.into() };
    if expansion.runners.contains("pnpm") && project.manager != Manager::Pnpm {
        out.push(tool("pnpm", "corepack enable pnpm || npm install -g pnpm --no-audit --no-fund"));
    }
    if expansion.runners.contains("bun") && project.manager != Manager::Bun {
        out.push(tool("bun", "npm install -g bun --no-audit --no-fund"));
    }
    let mut seen = BTreeSet::new();
    for step in &expansion.steps {
        if !seen.insert(step.name.clone()) || matches!(step.name.as_str(), "node" | "npm" | "npx" | "yarn" | "pnpm" | "pnpx" | "bun" | "bunx") {
            continue;
        }
        if let Verdict::Global { package, version } = verdict(root, packages, None, step) {
            out.push(tool(&step.name, &format!("npm install -g --no-audit --no-fund {}", quote(&format!("{package}@{version}")))));
        }
    }
    out
}

pub enum Verdict {
    /// Runs only on Windows.
    Windows,
    /// Will be there: in the image, installed with the project, or downloaded when run.
    Present,
    /// Installed globally on the PC with npm; the container installs the same package.
    Global { package: String, version: String },
    /// Missing from the container, and why.
    Missing(&'static str),
    /// A file the command runs that the copy leaves behind.
    NotCopied(String),
    Unknown,
}

const WINDOWS: [&str; 26] = [
    "powershell", "powershell.exe", "cmd", "cmd.exe", "start", "taskkill", "tasklist", "setx",
    "xcopy", "robocopy", "del", "rd", "copy", "move", "cls", "explorer", "explorer.exe", "wsl",
    "wsl.exe", "reg", "netsh", "wmic", "icacls", "attrib", "where", "mklink",
];

pub fn windows_only(name: &str) -> bool {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).to_ascii_lowercase();
    WINDOWS.contains(&base.as_str())
        || base.rsplit_once('.').is_some_and(|(_, ext)| matches!(ext, "exe" | "bat" | "cmd" | "ps1" | "vbs"))
}

/// Programs every Yougori project image has (Debian bookworm with build tools).
const IMAGE: [&str; 66] = [
    "node", "npm", "npx", "corepack", "yarn", "yarnpkg", "sh", "bash", "dash", "env", "echo",
    "printf", "cat", "cp", "mv", "rm", "mkdir", "rmdir", "touch", "ls", "ln", "chmod", "chown",
    "test", "[", "true", "false", ":", "sleep", "kill", "pwd", "export", "exit", "set", "unset",
    "source", ".", "eval", "wait", "trap", "read", "grep", "sed", "awk", "find", "xargs", "sort",
    "head", "tail", "tr", "cut", "wc", "tee", "date", "basename", "dirname", "realpath",
    "readlink", "which", "type", "timeout", "git", "curl", "wget", "tar", "make",
];

pub fn missing_tool(name: &str) -> Option<&'static str> {
    Some(match name {
        "docker" | "docker-compose" | "podman" => "Docker isn't available inside the project container.",
        "kubectl" | "helm" => "Kubernetes tools aren't installed in the project container.",
        "java" | "javac" | "mvn" | "gradle" => "Java isn't installed in the project image.",
        "go" => "Go isn't installed in the project image.",
        "cargo" | "rustc" | "rustup" => "Rust isn't installed in the project image.",
        "dotnet" => ".NET isn't installed in the project image.",
        "php" | "composer" => "PHP isn't installed in the project image.",
        "ruby" | "gem" | "bundle" | "rails" => "Ruby isn't installed in the project image.",
        "deno" => "Deno isn't installed in the project image.",
        "pwsh" => "PowerShell isn't installed in the project container.",
        "pip" | "pip3" => "pip isn't installed in the Node.js image. Launch the Python part as its own project (with requirements.txt).",
        "sudo" => "sudo isn't installed; commands in the container already run as root.",
        "brew" => "Homebrew exists only on macOS and Linux desktops, not in the container.",
        "open" | "xdg-open" => "The container has no desktop to open files or links in.",
        "code" => "VS Code isn't available inside the container.",
        "ngrok" | "cloudflared" => "Yougori publishes the app itself; tunnel tools aren't installed in the container.",
        "redis-server" | "mongod" | "mysqld" | "postgres" | "pg_ctl" => "Database servers aren't installed in the project container.",
        _ => return None,
    })
}

/// Packages whose programs are not named after the package.
const BINS: [(&str, &[&str]); 30] = [
    ("typescript", &["tsc", "tsserver"]), ("@angular/cli", &["ng"]), ("@nestjs/cli", &["nest"]),
    ("@vue/cli-service", &["vue-cli-service"]), ("webpack-cli", &["webpack"]), ("@remix-run/dev", &["remix"]),
    ("@react-router/dev", &["react-router"]), ("@sveltejs/kit", &["svelte-kit"]), ("@tailwindcss/cli", &["tailwindcss"]),
    ("firebase-tools", &["firebase"]), ("netlify-cli", &["netlify", "ntl"]), ("npm-run-all", &["run-p", "run-s"]),
    ("npm-run-all2", &["npm-run-all", "run-p", "run-s"]), ("dotenv-cli", &["dotenv"]), ("sequelize-cli", &["sequelize"]),
    ("@playwright/test", &["playwright"]), ("@babel/cli", &["babel"]), ("postcss-cli", &["postcss"]),
    ("storybook", &["sb"]), ("nuxt", &["nuxi"]), ("pm2", &["pm2-runtime", "pm2-dev"]), ("ts-node-dev", &["tsnd"]),
    ("@swc/cli", &["swc"]), ("@docusaurus/core", &["docusaurus"]), ("http-server", &["hs"]), ("copyfiles", &["copyup"]),
    ("less", &["lessc"]), ("concurrently", &["conc"]), ("cross-env", &["cross-env-shell"]), ("gatsby-cli", &["gatsby"]),
];

fn declares(package: &Value, program: &str) -> bool {
    ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"].iter().any(|field| {
        package[field].as_object().is_some_and(|deps| {
            deps.keys().any(|name| {
                name == program
                    || name.rsplit('/').next() == Some(program)
                    || BINS.iter().any(|(package, bins)| package == name && bins.contains(&program))
            })
        })
    })
}

/// Whether the package in `dir`, or one above it, provides `program`: installed on the PC or
/// declared as a dependency (npm puts every enclosing node_modules/.bin on PATH).
fn provided(root: &Path, packages: &mut Packages, dir: &str, program: &str) -> bool {
    let mut current = Some(dir.to_owned());
    while let Some(dir) = current {
        let bin = root.join(&dir).join("node_modules").join(".bin");
        if bin.join(program).is_file() || bin.join(format!("{program}.cmd")).is_file() || packages.get(&dir).is_some_and(|p| declares(p, program)) {
            return true;
        }
        current = (dir != ".").then(|| scripts::join(&dir, "..")).flatten();
    }
    false
}

pub fn verdict(root: &Path, packages: &mut Packages, filter: Option<&sync::Filter>, step: &Step) -> Verdict {
    let name = step.name.as_str();
    if windows_only(name) {
        return Verdict::Windows;
    }
    if step.fetched || name.starts_with('/') {
        return Verdict::Present;
    }
    if name.contains('/') {
        let Some(relative) = scripts::join(&step.dir, name) else { return Verdict::Unknown };
        if relative.split('/').any(|part| part == "node_modules") {
            return Verdict::Present;
        }
        return match filter {
            Some(filter) if root.join(&relative).is_file() && filter.excluded(&relative, false) => Verdict::NotCopied(relative),
            _ => Verdict::Present,
        };
    }
    if provided(root, packages, &step.dir, name) || IMAGE.contains(&name) {
        return Verdict::Present;
    }
    if let Some((package, version)) = global(name) {
        return Verdict::Global { package, version };
    }
    missing_tool(name).map_or(Verdict::Unknown, Verdict::Missing)
}

/// npm packages installed globally on this PC, as (program, package, version).
fn globals() -> &'static [(String, String, String)] {
    static GLOBALS: OnceLock<Vec<(String, String, String)>> = OnceLock::new();
    GLOBALS.get_or_init(|| {
        let mut found = Vec::new();
        for modules in global_folders() {
            let Ok(entries) = fs::read_dir(&modules) else { continue };
            for entry in entries.flatten() {
                let dirs: Vec<PathBuf> = if entry.file_name().to_string_lossy().starts_with('@') {
                    fs::read_dir(entry.path()).map(|e| e.flatten().map(|e| e.path()).collect()).unwrap_or_default()
                } else {
                    vec![entry.path()]
                };
                for dir in dirs {
                    let Ok(bytes) = fs::read(dir.join("package.json")) else { continue };
                    let Ok(package) = serde_json::from_slice::<Value>(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes)) else { continue };
                    let (Some(name), Some(version)) = (package["name"].as_str(), package["version"].as_str()) else { continue };
                    if matches!(name, "npm" | "corepack") {
                        continue;
                    }
                    let bins: Vec<String> = match &package["bin"] {
                        Value::String(_) => vec![name.rsplit('/').next().unwrap_or(name).to_owned()],
                        Value::Object(map) => map.keys().cloned().collect(),
                        _ => Vec::new(),
                    };
                    found.extend(bins.into_iter().map(|bin| (bin, name.to_owned(), version.to_owned())));
                }
            }
        }
        found
    })
}

/// Where npm keeps global packages on this PC: the user prefix, npm's configured prefix (set
/// when started through npm), and the folder of `node` itself (official installers, nvm).
fn global_folders() -> Vec<PathBuf> {
    let modules = |prefix: PathBuf| if cfg!(windows) { prefix.join("node_modules") } else { prefix.join("lib").join("node_modules") };
    let mut folders = Vec::new();
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            folders.push(PathBuf::from(appdata).join("npm").join("node_modules"));
        }
    }
    for name in ["npm_config_prefix", "npm_config_global_prefix"] {
        if let Some(prefix) = std::env::var_os(name) {
            folders.push(modules(PathBuf::from(prefix)));
        }
    }
    let node = if cfg!(windows) { "node.exe" } else { "node" };
    if let Some(dir) = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).find(|dir| dir.join(node).is_file()) {
        folders.push(if cfg!(windows) { dir.join("node_modules") } else { modules(dir.parent().map(Path::to_path_buf).unwrap_or_default()) });
    }
    folders.dedup();
    folders
}

pub fn global(program: &str) -> Option<(String, String)> {
    globals().iter().find(|(bin, _, _)| bin == program).map(|(_, package, version)| (package.clone(), version.clone()))
}

/// The folder whose node_modules/.bin has `program` on the PC: the project or a package inside it.
pub fn installed_bin(root: &Path, program: &str) -> Option<String> {
    fn search(root: &Path, dir: &str, program: &str, depth: usize) -> Option<String> {
        let bin = root.join(dir).join("node_modules").join(".bin");
        if bin.join(program).is_file() || bin.join(format!("{program}.cmd")).is_file() {
            return Some(dir.to_owned());
        }
        if depth == 0 {
            return None;
        }
        let mut entries: Vec<String> = fs::read_dir(root.join(dir)).ok()?.flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .filter(|name| !matches!(name.as_str(), "node_modules" | ".git"))
            .collect();
        entries.sort();
        entries.into_iter().find_map(|name| {
            let child = if dir == "." { name } else { format!("{dir}/{name}") };
            search(root, &child, program, depth - 1)
        })
    }
    search(root, ".", program, 3)
}

/// The Node.js major version on this PC.
pub fn pc_node_major() -> Option<u32> {
    static MAJOR: OnceLock<Option<u32>> = OnceLock::new();
    *MAJOR.get_or_init(|| {
        // Started through npm: "npm/10.9.2 node/v22.12.0 win32 x64 workspaces/false".
        let agent = std::env::var("npm_config_user_agent").ok();
        let version = agent
            .as_deref()
            .and_then(|a| a.split_whitespace().find_map(|w| w.strip_prefix("node/")).map(str::to_owned))
            .or_else(|| {
                let output = std::process::Command::new("node").arg("--version").stdin(std::process::Stdio::null()).output().ok()?;
                output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
            })?;
        version.trim_start_matches('v').split('.').next()?.parse().ok()
    })
}

#[derive(Debug, PartialEq)]
pub struct Finding {
    /// The command cannot work as it is.
    pub blocking: bool,
    pub text: String,
}

#[derive(Default)]
pub struct Check {
    pub findings: Vec<Finding>,
    /// Commands from the project's own scripts without the blocking findings, best first.
    pub alternatives: Vec<String>,
}

/// Reads the run command before anything starts.
pub fn check(root: &Path, project: &Project, env_files: bool) -> Check {
    let command = project.command_label();
    if project.is_python() || command.is_empty() {
        return Check::default();
    }
    let filter = sync::Filter::load(root, env_files).ok();
    let mut packages = Packages::new(root);
    let expansion = scripts::expand(&mut packages, ".", &command);
    let findings = findings(root, &mut packages, filter.as_ref(), &expansion);
    let alternatives = if findings.iter().any(|f| f.blocking) {
        alternatives(root, &mut packages, filter.as_ref(), &command, &expansion)
    } else {
        Vec::new()
    };
    Check { findings, alternatives }
}

/// Run choices for ordinary setup, including Linux replacements for Windows launch scripts.
/// Build/install/test scripts alone are not app servers and do not belong in this menu.
pub fn run_choices(root: &Path, project: &Project) -> Vec<RunChoice> {
    let default = project.command_label();
    let mut out = Vec::new();
    if !default.is_empty() {
        let detail = if project.is_python() { "Detected command" } else { project.command() };
        out.push((default, detail.to_owned()));
    }
    if project.is_python() { return recommend::rank(root, project, out); }
    let mut packages = Packages::new(root);
    let names: Vec<String> = project.package["scripts"].as_object().into_iter().flat_map(|s| s.keys())
        .filter(|name| matches!(name.split(':').next().unwrap_or(""), "dev" | "start" | "serve" | "prod" | "production" | "preview"))
        .cloned().collect();
    for name in names {
        if !project.package["scripts"][&name].as_str().is_some_and(|s| !s.trim().is_empty()) { continue; }
        let command = run_script(root, &mut packages, ".", &name);
        let expansion = scripts::expand(&mut packages, ".", &command);
        let candidates = if findings(root, &mut packages, None, &expansion).iter().any(|f| f.blocking) {
            alternatives(root, &mut packages, None, &command, &expansion).into_iter()
                .map(|c| (c, format!("Linux alternative to {} run {name}", project.manager.name()))).collect()
        } else {
            vec![(command, project.package["scripts"][&name].as_str().unwrap_or("").to_owned())]
        };
        for (command, detail) in candidates {
            if !out.iter().any(|(existing, _)| existing == &command) {
                out.push((command, detail));
            }
        }
    }
    recommend::rank(root, project, out)
}

fn findings(root: &Path, packages: &mut Packages, filter: Option<&sync::Filter>, expansion: &scripts::Expansion) -> Vec<Finding> {
    let mut out: Vec<Finding> = Vec::new();
    let mut add = |blocking: bool, text: String| {
        if !out.iter().any(|f| f.text == text) {
            out.push(Finding { blocking, text });
        }
    };
    for missing in &expansion.missing {
        let manifest = if missing.dir == "." { "package.json".to_owned() } else { format!("{}/package.json", missing.dir) };
        add(true, format!("{manifest} has no script named {} (run by {}).", missing.name, missing.origin));
    }
    for step in &expansion.steps {
        let origin = step.origin();
        if step.optional {
            continue;
        }
        if step.name.eq_ignore_ascii_case("set") && step.args.first().is_some_and(|a| a.contains('=')) {
            add(false, format!("`set {}` is Windows cmd syntax ({origin}); on Linux the variable stays unset. cross-env sets it on every system.", step.args[0]));
            continue;
        }
        if let Some(variable) = step.args.iter().find_map(|a| windows_variable(a)) {
            add(false, format!("`%{variable}%` is Windows cmd syntax ({origin}); Linux passes it on literally. Use ${variable} instead."));
        }
        match verdict(root, packages, filter, step) {
            Verdict::Windows => add(true, format!("`{}` only runs on Windows ({origin}), and the container runs Linux.", step.name)),
            Verdict::Missing(why) => add(false, format!("`{}` ({origin}): {why}", step.name)),
            Verdict::NotCopied(path) => add(false, format!("{path} ({origin}) isn't copied to the container: {}.", reason(root, &path, filter))),
            _ => {}
        }
    }
    out
}

fn reason(root: &Path, path: &str, filter: Option<&sync::Filter>) -> &'static str {
    match sync::excluded_by(root, path, false, filter.is_some_and(sync::Filter::copies_env_files)) {
        Some(sync::Excluded::Always) => "temporary sync files are not project files",
        Some(sync::Excluded::Env) => ".env files stay on this PC unless you choose to copy them",
        None => "the file is included in the project copy",
    }
}

/// `%NAME%` (upper case, two characters or more), which only cmd.exe expands.
fn windows_variable(arg: &str) -> Option<&str> {
    let mut rest = arg;
    while let Some(start) = rest.find('%') {
        let after = &rest[start + 1..];
        let end = after.find('%')?;
        let name = &after[..end];
        if name.len() >= 2 && name.starts_with(|c: char| c.is_ascii_uppercase()) && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
            return Some(name);
        }
        rest = &after[end..];
    }
    None
}

/// Commands built from the project's scripts that avoid what blocks `command`: the steps before
/// the blocked one (usually a build) followed by a script that serves the result, or a dev script.
fn alternatives(root: &Path, packages: &mut Packages, filter: Option<&sync::Filter>, command: &str, expansion: &scripts::Expansion) -> Vec<String> {
    let blocked = expansion.steps.iter().find(|step| !step.optional && matches!(verdict(root, packages, filter, step), Verdict::Windows));
    let prefix = blocked.map(|step| prefix(packages, step, command)).unwrap_or_default();
    let built: BTreeSet<String> = if prefix.is_empty() { BTreeSet::new() } else { scripts::expand(packages, ".", &prefix).folders };
    let mut folders: Vec<String> = built.iter().filter(|d| *d != ".").cloned().collect();
    folders.push(".".into());
    folders.extend(expansion.folders.iter().filter(|d| !folders.contains(d)).cloned().collect::<Vec<_>>());
    let mut candidates = Vec::new();
    for (order, dir) in folders.iter().enumerate() {
        let Some(names) = packages.get(dir).and_then(|p| p["scripts"].as_object()).map(|s| s.keys().cloned().collect::<BTreeSet<_>>()) else { continue };
        for (rank, name) in [(0, "start"), (1, "serve"), (2, "prod"), (2, "production"), (2, "start:prod"), (3, "dev"), (4, "preview")] {
            if !names.contains(name) {
                continue;
            }
            let run = run_script(root, packages, dir, name);
            // Dev servers build on the fly; the others serve what the build step produced.
            let (full, order) = if name == "dev" {
                (run, if dir == "." { 0 } else { order + 1 })
            } else if prefix.is_empty() {
                (run, order)
            } else {
                (format!("{prefix} && {run}"), order)
            };
            candidates.push((rank, order, full));
        }
    }
    candidates.sort();
    let mut out: Vec<String> = Vec::new();
    // Two commands that end up running the same programs are one choice.
    let mut runs = BTreeSet::new();
    for (_, _, candidate) in candidates {
        if candidate == command || out.contains(&candidate) {
            continue;
        }
        let expansion = scripts::expand(packages, ".", &candidate);
        let programs: Vec<(String, String, Vec<String>)> = expansion.steps.iter().map(|s| (s.dir.clone(), s.name.clone(), s.args.clone())).collect();
        if !findings(root, packages, filter, &expansion).iter().any(|f| f.blocking) && runs.insert(programs) {
            out.push(candidate);
        }
        if out.len() == 3 {
            break;
        }
    }
    out
}

/// The commands before `step` in its script that must succeed first (joined by `&&`), when that
/// script belongs to the project folder itself.
fn prefix(packages: &mut Packages, step: &Step, command: &str) -> String {
    let body = match &step.owner {
        None => Some(command.to_owned()),
        Some((dir, name)) if dir == "." => packages.script(dir, name),
        Some(_) => None,
    };
    let Some(body) = body else { return String::new() };
    let parts = scripts::split(&body);
    let before = &parts[..step.part.min(parts.len())];
    let chained = parts.get(step.part).is_some_and(|p| p.join == Join::And) && before.iter().skip(1).all(|p| p.join == Join::And);
    if before.is_empty() || !chained {
        return String::new();
    }
    before.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join(" && ")
}

/// How to run script `name` of the package in `dir` from the project folder.
fn run_script(root: &Path, packages: &mut Packages, dir: &str, name: &str) -> String {
    let manager = package::manager(&root.join(dir), packages.get(dir).unwrap_or(&Value::Null));
    let name = quote(name);
    if dir == "." {
        return format!("{} run {name}", manager.name());
    }
    let dir = quote(dir);
    match manager {
        Manager::Npm => format!("npm --prefix {dir} run {name}"),
        Manager::Pnpm => format!("pnpm --dir {dir} run {name}"),
        Manager::Yarn => format!("yarn --cwd {dir} run {name}"),
        Manager::Bun => format!("bun --cwd {dir} run {name}"),
    }
}

fn quote(word: &str) -> String {
    if !word.is_empty() && word.chars().all(|c| c.is_ascii_alphanumeric() || "_-./:@+=".contains(c)) {
        word.into()
    } else {
        super::project::shell(word)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let path = root.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        root
    }

    fn manifest(root: &Path) -> sync::Manifest {
        sync::scan(root, &sync::Filter::load(root, false).unwrap()).unwrap()
    }

    const CRM: &str = r#"{"scripts":{
        "dev":"npm run dev:client","dev:client":"npm --prefix client run dev",
        "production":"npm run build && powershell -NoProfile -ExecutionPolicy Bypass -File scripts/restart-production.ps1",
        "start":"npm run production","build":"npm --prefix server run build && npm --prefix client run build",
        "install:all":"npm install && npm --prefix server install && npm --prefix client install"},
        "devDependencies":{"concurrently":"^9"}}"#;

    fn crm() -> tempfile::TempDir {
        project(&[
            ("package.json", CRM),
            ("package-lock.json", "{}"),
            ("server/package.json", r#"{"scripts":{"build":"tsc -p tsconfig.json","start":"node dist/index.js","dev":"tsx watch src/index.ts"},"devDependencies":{"typescript":"^5","tsx":"^4"}}"#),
            ("server/package-lock.json", "{}"),
            ("client/package.json", r#"{"scripts":{"dev":"vite","build":"tsc --noEmit && vite build","preview":"vite preview"},"devDependencies":{"typescript":"^5","vite":"^6"}}"#),
            ("extension/package.json", r#"{"name":"not-used"}"#),
            ("scripts/restart-production.ps1", "Write-Host hi"),
        ])
    }

    #[test]
    fn nested_packages_the_project_uses_are_installed_with_it() {
        let root = crm();
        let mut project = Project::load(root.path()).unwrap();
        project.custom_command = Some("npm run production".into());
        let installs = installs(root.path(), &project, &manifest(root.path()));
        let folders: Vec<_> = installs.iter().map(|i| (i.folder.as_str(), i.required)).collect();
        assert_eq!(folders, [(".", true), ("client", false), ("server", false)], "extension/ is not used by any script");
        assert!(installs[1].command.contains("npm ci") && installs[1].command.ends_with("prepare.cjs optional"));
        assert_eq!(describe(&installs).unwrap(), "Also installs client/, server/");
        let line = lines(&installs);
        assert_eq!(line.lines().nth(2).unwrap().split('\t').take(4).collect::<Vec<_>>(), ["server", "node_modules", "0", "server/ dependencies"]);
        // Copied dependency manifests are not independent project packages.
        fs::create_dir_all(root.path().join("node_modules/vendor/node_modules")).unwrap();
        fs::write(root.path().join("node_modules/vendor/package.json"), "{}").unwrap();
        let copied = super::installs(root.path(), &project, &manifest(root.path()));
        assert!(!copied.iter().any(|i| i.folder.starts_with("node_modules/")));
        // A folder installed on the PC counts even when no script names it.
        fs::create_dir_all(root.path().join("extension/node_modules")).unwrap();
        let installs = super::installs(root.path(), &project, &manifest(root.path()));
        assert!(installs.iter().any(|i| i.folder == "extension"));
    }

    #[test]
    fn workspace_members_and_folders_the_project_installs_itself_are_skipped() {
        let root = project(&[
            ("package.json", r#"{"workspaces":["packages/*","!packages/legacy"],"scripts":{"postinstall":"cd tools && npm install","dev":"npm --prefix packages/web run dev && npm --prefix packages/legacy run dev && npm --prefix tools run x"}}"#),
            ("packages/web/package.json", r#"{"scripts":{"dev":"vite"}}"#),
            ("packages/legacy/package.json", r#"{"scripts":{"dev":"vite"}}"#),
            ("tools/package.json", r#"{"scripts":{"x":"node x.js"}}"#),
        ]);
        let installs = installs(root.path(), &Project::load(root.path()).unwrap(), &manifest(root.path()));
        let folders: Vec<_> = installs.iter().map(|i| i.folder.as_str()).collect();
        assert_eq!(folders, [".", "packages/legacy"]);
        let pnpm = project(&[
            ("package.json", r#"{"scripts":{"dev":"npm --prefix apps/site run dev"}}"#),
            ("pnpm-workspace.yaml", "packages:\n  - 'apps/*'\n"),
            ("apps/site/package.json", r#"{"scripts":{"dev":"vite"}}"#),
        ]);
        let installs = super::installs(pnpm.path(), &Project::load(pnpm.path()).unwrap(), &manifest(pnpm.path()));
        assert_eq!(installs.len(), 1);
    }

    #[test]
    fn package_managers_the_scripts_call_are_installed_as_tools() {
        let root = project(&[("package.json", r#"{"scripts":{"dev":"pnpm --dir web dev & bun run api/index.ts"}}"#), ("web/package.json", r#"{"scripts":{"dev":"vite"}}"#)]);
        let installs = installs(root.path(), &Project::load(root.path()).unwrap(), &manifest(root.path()));
        let tools: Vec<_> = installs.iter().filter(|i| i.marker.starts_with('/')).map(|i| i.label.as_str()).collect();
        assert_eq!(tools, ["pnpm", "bun"]);
        assert!(installs.iter().any(|i| i.folder == "web"));
    }

    #[test]
    fn ordinary_run_menu_includes_full_dev_and_linux_production_without_a_custom_command() {
        let root = crm();
        let mut project = Project::load(root.path()).unwrap();
        project.package["scripts"]["dev:full"] = serde_json::json!("concurrently -n server,client \"npm:dev:server\" \"npm:dev:client\"");
        project.package["scripts"]["dev:server"] = serde_json::json!("npm --prefix server run dev");
        fs::write(root.path().join("package.json"), project.package.to_string()).unwrap();
        assert!(check(root.path(), &project, false).alternatives.is_empty(), "the default dev command has no blocking finding");
        let choices = run_choices(root.path(), &project);
        assert_eq!(choices[0].command, "npm run build && npm --prefix server run start");
        assert!(choices[0].recommended);
        assert!(choices.iter().any(|c| c.command == "npm run dev:full" && c.detail.contains("client, server")));
        let commands: Vec<_> = choices.iter().map(|c| c.command.as_str()).collect();
        assert!(commands.contains(&"npm run build && npm --prefix server run start"));
        assert!(commands.contains(&"npm run dev:server"));
        assert!(!commands.contains(&"npm run dev:client"), "duplicate of dev");
        for hidden in ["npm run production", "npm run start", "npm run build", "npm run install:all"] {
            assert!(!commands.contains(&hidden), "{hidden}");
        }
        assert_eq!(commands.len(), commands.iter().collect::<BTreeSet<_>>().len());
    }

    #[test]
    fn run_menu_supports_projects_without_a_default_and_keeps_the_package_manager() {
        let root = project(&[("package.json", r#"{"packageManager":"pnpm@10.0.0","scripts":{"preview":"vite preview","build":"vite build"}}"#)]);
        let project = Project::load(root.path()).unwrap();
        assert!(project.command_label().is_empty());
        let choices = run_choices(root.path(), &project);
        assert_eq!(choices[0].command, "pnpm run build && pnpm run preview");
        assert!(choices[0].recommended);
    }

    #[test]
    fn windows_steps_block_with_replacements_from_the_projects_scripts() {
        let root = crm();
        let mut project = Project::load(root.path()).unwrap();
        project.custom_command = Some("npm run production".into());
        let check = check(root.path(), &project, false);
        let blocking: Vec<_> = check.findings.iter().filter(|f| f.blocking).map(|f| f.text.as_str()).collect();
        assert_eq!(blocking, ["`powershell` only runs on Windows (scripts.production), and the container runs Linux."]);
        assert_eq!(check.alternatives, [
            "npm run build && npm --prefix server run start",
            "npm run dev",
            "npm --prefix server run dev",
        ]);
        project.custom_command = Some(check.alternatives[0].clone());
        let fixed = super::check(root.path(), &project, false);
        assert!(fixed.findings.is_empty(), "{:?}", fixed.findings);
        project.custom_command = Some("npm run prod".into());
        let missing = super::check(root.path(), &project, false);
        assert_eq!(missing.findings[0].text, "package.json has no script named prod (run by your command).");
        assert!(missing.findings[0].blocking);
        assert_eq!(missing.alternatives[0], "npm run dev");
    }

    #[test]
    fn warnings_cover_windows_syntax_and_missing_tools_but_allow_copied_build_output() {
        let root = project(&[
            ("package.json", r#"{"scripts":{"start":"node dist/server.js","win":"set NODE_ENV=production&& node app.js --port %PORT%","db":"docker compose up || echo skip","up":"docker compose up"}}"#),
            ("dist/server.js", ""),
        ]);
        let mut project = Project::load(root.path()).unwrap();
        let text = |project: &Project| check(root.path(), project, false).findings.into_iter().map(|f| (f.blocking, f.text)).collect::<Vec<_>>();
        assert!(text(&project).is_empty());
        project.custom_command = Some("npm run win".into());
        let found = text(&project);
        assert!(found[0].1.starts_with("`set NODE_ENV=production` is Windows cmd syntax"));
        assert!(found[1].1.starts_with("`%PORT%` is Windows cmd syntax"));
        project.custom_command = Some("npm run db".into());
        assert!(text(&project).is_empty(), "a fallback after || is expected to fail");
        project.custom_command = Some("npm run up".into());
        assert_eq!(text(&project), [(false, "`docker` (scripts.up): Docker isn't available inside the project container.".into())]);
        assert_eq!(windows_variable("date +%Y%m%d"), None);
        assert!(windows_only("scripts/setup.BAT") && windows_only("powershell.exe") && !windows_only("node"));
    }

    #[test]
    fn programs_are_found_in_node_modules_declared_dependencies_or_enclosing_packages() {
        let root = project(&[
            ("package.json", r#"{"devDependencies":{"typescript":"^5"}}"#),
            ("web/package.json", r#"{}"#),
            ("web/node_modules/.bin/vite.cmd", ""),
        ]);
        let mut packages = Packages::new(root.path());
        let step = |dir: &str, name: &str| Step { dir: dir.into(), name: name.into(), args: Vec::new(), fetched: false, optional: false, owner: None, part: 0 };
        assert!(matches!(verdict(root.path(), &mut packages, None, &step("web", "tsc")), Verdict::Present), "declared by the enclosing package");
        assert!(matches!(verdict(root.path(), &mut packages, None, &step("web", "vite")), Verdict::Present));
        assert!(matches!(verdict(root.path(), &mut packages, None, &step(".", "made-up-tool")), Verdict::Unknown));
        assert_eq!(installed_bin(root.path(), "vite").as_deref(), Some("web"));
    }
}
