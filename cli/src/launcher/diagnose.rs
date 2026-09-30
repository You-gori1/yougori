//! Explains a failed install or start from its output: which program, package or file is missing,
//! why the container lacks it while the PC has it, and what to do. Every explanation rests on a
//! line in the output and on the project folder; nothing is inferred from an exit code alone.
use super::{package, plan, scripts, sync};
use serde_json::Value;
use std::path::Path;

pub struct Context<'a> {
    pub root: &'a Path,
    pub env_files: bool,
    /// The Node.js major version in the container, for Node projects.
    pub node: Option<u32>,
    /// How to change the launch settings, e.g. `npm run yougori-change`.
    pub change: &'a str,
}

/// Terminal output as plain lines: colours removed, and of each line only what a terminal shows
/// last (progress spinners rewrite a line with `\r`).
fn plain(output: &str) -> Vec<String> {
    let mut text = String::with_capacity(output.len());
    let mut chars = output.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while chars.next().is_some_and(|c| !('@'..='~').contains(&c)) {}
            }
            continue;
        }
        text.push(c);
    }
    text.split('\n')
        .map(|line| line.trim_end_matches('\r').rsplit('\r').find(|s| !s.trim().is_empty()).unwrap_or("").trim().to_owned())
        .collect()
}

pub fn explain(output: &str, context: &Context) -> Vec<String> {
    let lines = plain(output);
    let mut found: Vec<String> = Vec::new();
    let mut add = |text: String| {
        if !found.contains(&text) {
            found.push(text);
        }
    };
    // CRLF before anything else: the shell then misreads every other name in the script.
    if output.contains("$'\\r'") || output.contains("\r': No such file") || output.contains("\r: not found") || output.contains("\r: No such file") {
        add("A script has Windows line endings (CRLF), which Linux shells can't run. Save it with LF line endings, or add `*.sh text eol=lf` to .gitattributes and check it out again.".into());
    }
    for (index, line) in lines.iter().enumerate() {
        if let Some(name) = missing_command(line) {
            if !name.contains('\r') && !name.contains("\\r") {
                add(command(name, context));
            }
        } else if let Some(name) = line.strip_suffix(": Permission denied").and_then(|l| l.rsplit(": ").next()).filter(|_| line.starts_with("sh:") || line.starts_with("bash:")) {
            add(format!("`{name}` isn't executable in the container. Run it through its interpreter, for example `sh {name}`."));
        } else if let Some(module) = module_name(line) {
            let importer = line.split_once(" imported from ").map(|(_, from)| from.trim().to_owned()).or_else(|| {
                lines.iter().skip(index + 1).take(8).find_map(|l| l.strip_prefix("- /").map(|p| format!("/{p}")))
            });
            if let Some(text) = self::module(module, importer.as_deref(), context) {
                add(text);
            }
        } else if let Some(path) = quoted_after(line, "ENOENT: no such file or directory, ").or_else(|| line.split_once("ENOENT: no such file or directory, ").and_then(|(_, rest)| rest.split_once(' ')).and_then(|(_, p)| p.strip_prefix('\'')).and_then(|p| p.split('\'').next())).or_else(|| quoted_after(line, "No such file or directory: ")).or_else(|| quoted_after(line, "can't open file ")) {
            if let Some(text) = file(path, context) {
                add(text);
            }
        } else if let Some(name) = missing_script(line) {
            let mut scripts: Vec<String> = package::Project::load(context.root).ok().and_then(|p| p.package["scripts"].as_object().map(|s| s.keys().filter(|k| !k.starts_with("yougori")).cloned().collect())).unwrap_or_default();
            scripts.truncate(8);
            let available = if scripts.is_empty() { String::new() } else { format!(" (it has {})", scripts.join(", ")) };
            add(format!("package.json has no script named {name}{available}. Choose another command with {}.", context.change));
        } else if let Some(module) = quoted_after(line, "ModuleNotFoundError: No module named ") {
            let listed = if context.root.join("requirements.txt").is_file() { "Add it to requirements.txt." } else { "List it in a requirements.txt file." };
            add(format!("The Python package `{module}` isn't installed in the container. {listed}"));
        } else if let Some(port) = line.split("EADDRINUSE").nth(1).and_then(|rest| rest.rsplit(':').next()).and_then(|p| p.trim().parse::<u16>().ok()) {
            add(format!("Port {port} is already taken inside the container, usually by the previous server that is still stopping. Press r to restart."));
        }
    }
    let joined = lines.join("\n");
    let native = ["invalid ELF header", "NODE_MODULE_VERSION", "was compiled against a different Node.js version", "Could not locate the bindings file", "gyp ERR!", "node-pre-gyp ERR!", "prebuild-install warn install No prebuilt binaries"];
    if let Some(line) = lines.iter().find(|l| native.iter().any(|n| l.contains(n))) {
        let package = line.split("/node_modules/").nth(1).map(|rest| {
            let mut parts = rest.split('/');
            let first = parts.next().unwrap_or_default();
            if first.starts_with('@') { format!("{first}/{}", parts.next().unwrap_or_default()) } else { first.to_owned() }
        }).filter(|p| !p.is_empty()).map_or("A native package".to_owned(), |p| format!("The native package {p}"));
        let node = context.node.map_or(String::new(), |n| format!(" for Node {n}"));
        add(format!("{package} failed to build or load{node} on Linux.{}", node_hint(context)));
    }
    if ["EBADENGINE", "Unsupported engine", "The engine \"node\" is incompatible"].iter().any(|n| joined.contains(n)) {
        let node = context.node.map_or("the container's".to_owned(), |n| format!("the container's Node {n}"));
        add(format!("A package asks for a different Node.js version than {node}.{}", node_hint(context)));
    }
    if joined.contains("JavaScript heap out of memory") || joined.contains("Reached heap limit") {
        add(format!("Node ran out of heap memory. Give the container more RAM with {}, or raise the limit with NODE_OPTIONS=--max-old-space-size=4096 in the command.", context.change));
    }
    if !context.env_files && sync::has_env_files(context.root) {
        let lower = joined.to_ascii_lowercase();
        if ["environment variable", "process.env", "env var", "missing required", "is not set", "must be set", "must be defined", "api key", "api_key", "database_url", ".env"].iter().any(|n| lower.contains(n)) {
            add(format!(".env files stayed on this PC (your launch setting). If the app reads them, run {} and choose to copy them.", context.change));
        }
    }
    found.truncate(4);
    found
}

/// For a command that ended with exit 0 before the app ever answered.
pub fn finished(command: &str) -> String {
    let lower = command.to_ascii_lowercase();
    let why = if lower.contains("pm2 start") {
        "pm2 start runs the app in the background and returns; `pm2-runtime start …` keeps it in the foreground."
    } else if lower.contains("forever start") {
        "forever start runs the app in the background and returns; run the server directly instead."
    } else if lower.trim_end().ends_with('&') || lower.contains("nohup ") {
        "It starts the server in the background (& or nohup) and returns; run it in the foreground."
    } else {
        "The launch command must keep running and serve the app, like `vite` or `node server.js`; build or setup commands end when they are done."
    };
    format!("The command finished without starting a server. {why}")
}

/// `sh: 1: tsc: not found`, `bash: tsc: command not found`, `env: 'node': No such file or directory`.
fn missing_command(line: &str) -> Option<&str> {
    if let Some(rest) = line.strip_prefix("env: ").or_else(|| line.strip_prefix("/usr/bin/env: ")) {
        return rest.strip_suffix(": No such file or directory").map(|name| name.trim_matches(|c| c == '\'' || c == '‘' || c == '’'));
    }
    let body = line.strip_suffix(": not found").or_else(|| line.strip_suffix(": command not found"))?;
    let name = body.rsplit(": ").next()?.trim();
    (!name.is_empty() && !name.contains(' ')).then_some(name)
}

fn command(name: &str, context: &Context) -> String {
    if plan::windows_only(name) {
        return format!("`{name}` is a Windows program, and the container runs Linux. Choose a command that works on Linux with {}.", context.change);
    }
    if let Some(dir) = plan::installed_bin(context.root, name) {
        let folder = if dir == "." { "the project".to_owned() } else { format!("{dir}/") };
        return match sync::excluded_by(context.root, &format!("{dir}/package.json"), false, context.env_files).filter(|_| dir != ".") {
            Some(_) => format!("`{name}` comes from {dir}/node_modules on your PC, but {folder} isn't copied to the container (it's ignored)."),
            None => format!("`{name}` is installed in {folder} on your PC, but {folder}'s dependencies are missing in the container; check their install output above."),
        };
    }
    if let Some((package, version)) = plan::global(name) {
        return format!("`{name}` is installed globally on your PC ({package} {version}), not in this project. Add {package} to devDependencies so the project installs it everywhere.");
    }
    match plan::missing_tool(name) {
        Some(why) => format!("`{name}`: {why}"),
        None => format!("`{name}` isn't installed in the container, and no package.json in this project provides it."),
    }
}

/// The text between quotes after `prefix`: `Cannot find module 'x'` gives `x`.
fn quoted_after<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = &line[line.find(prefix)? + prefix.len()..];
    let quote = rest.chars().next().filter(|c| matches!(c, '\'' | '"' | '‘'))?;
    let rest = &rest[quote.len_utf8()..];
    let end = rest.find(|c| c == quote || (quote == '‘' && c == '’'))?;
    Some(&rest[..end])
}

/// `Cannot find module 'x'`, `Cannot find package 'x' imported from …`, and Rollup's unquoted
/// `Cannot find module @rollup/rollup-linux-x64-gnu.`
fn module_name(line: &str) -> Option<&str> {
    for prefix in ["Cannot find module ", "Cannot find package "] {
        if let Some(name) = quoted_after(line, prefix) {
            return Some(name);
        }
        if let Some((_, rest)) = line.split_once(prefix) {
            let token = rest.split_whitespace().next()?.trim_end_matches(['.', ',']);
            if !token.is_empty() && !token.starts_with(['\'', '"', '‘']) {
                return Some(token);
            }
        }
    }
    None
}

/// `npm error Missing script: "x"`, pnpm, yarn and bun equivalents.
fn missing_script(line: &str) -> Option<&str> {
    let name = if let Some(rest) = line.split_once("Missing script:").map(|(_, r)| r) {
        rest
    } else if let Some(rest) = line.split_once("error Command \"").map(|(_, r)| r) {
        rest.strip_suffix("\" not found.")?
    } else if let Some(rest) = line.split_once("Couldn't find a script named ").map(|(_, r)| r) {
        rest
    } else if let Some(rest) = line.split_once("Script not found ").map(|(_, r)| r) {
        rest
    } else {
        return None;
    };
    let name = name.trim().trim_end_matches('.').trim_matches(|c| c == '"' || c == '\'' || c == '`');
    (!name.is_empty() && !name.contains(' ')).then_some(name)
}

fn module(module: &str, importer: Option<&str>, context: &Context) -> Option<String> {
    if module.starts_with('/') || module.starts_with('.') {
        let path = match (module.starts_with('.'), importer) {
            (true, Some(importer)) => {
                let base = workspace_relative(importer)?;
                let dir = base.rsplit_once('/').map_or(".", |(d, _)| d);
                scripts::join(dir, module)?
            }
            (true, None) => return None,
            (false, _) => workspace_relative(module)?,
        };
        return file(&path, context);
    }
    let lower = module.to_ascii_lowercase();
    if lower.contains("linux") && ["x64", "arm64"].iter().any(|a| lower.contains(a)) {
        return Some(format!("`{module}`, the Linux build of a native package, is missing: package-lock.json was written on another system and npm skipped it. Deleting package-lock.json lets npm resolve it again (Yougori reinstalls it on the next start)."));
    }
    if let Some(builtin) = module.strip_prefix("node:") {
        return Some(format!("`node:{builtin}` isn't available in the container's Node.js version.{}", node_hint(context)));
    }
    let mut parts = module.split('/');
    let first = parts.next()?;
    let name = if first.starts_with('@') { format!("{first}/{}", parts.next()?) } else { first.to_owned() };
    let folder = importer.and_then(workspace_relative).map_or(".".to_owned(), |path| package_folder(context.root, &path));
    let manifest = if folder == "." { "package.json".to_owned() } else { format!("{folder}/package.json") };
    let listed = std::fs::read(context.root.join(&manifest)).ok().and_then(|b| serde_json::from_slice::<Value>(b.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&b)).ok()).is_some_and(|package| {
        ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"].iter().any(|f| package[f].get(&name).is_some())
    });
    Some(if listed {
        format!("`{name}` is listed in {manifest} but isn't installed in the container; check the install output above.")
    } else {
        format!("`{name}` isn't a dependency in {manifest}. It works on your PC only because it's installed somewhere else there (a parent folder or globally). Add it to {manifest}.")
    })
}

/// A path inside the container's /workspace, relative to the project.
fn workspace_relative(path: &str) -> Option<String> {
    let path = path.split([':', ' ']).next()?;
    let rest = path.strip_prefix("/workspace/")?;
    scripts::join(".", rest).filter(|p| p != ".")
}

/// The nearest folder with a package.json that contains `path`.
fn package_folder(root: &Path, path: &str) -> String {
    let mut dir = path.rsplit_once('/').map_or(".".to_owned(), |(d, _)| d.to_owned());
    while dir != "." {
        if root.join(&dir).join("package.json").is_file() {
            return dir;
        }
        dir = dir.rsplit_once('/').map_or(".".to_owned(), |(d, _)| d.to_owned());
    }
    dir
}

fn file(path: &str, context: &Context) -> Option<String> {
    let relative = if path.starts_with('/') { workspace_relative(path)? } else { scripts::join(".", path)? };
    let name = relative.rsplit('/').next().unwrap_or(&relative);
    if (name == ".env" || name.starts_with(".env.")) && !context.env_files {
        return Some(format!("{relative} stays on this PC (your launch setting keeps .env files here). If the app needs it, run {} and choose to copy .env files.", context.change));
    }
    let on_pc = context.root.join(&relative).exists();
    if sync::build_output(&relative) && !on_pc {
        return Some(format!("{relative} doesn't exist yet: the build step creates it. Build before starting, for example npm run build && …"));
    }
    if !on_pc {
        return None;
    }
    Some(match sync::excluded_by(context.root, &relative, false, context.env_files)? {
        sync::Excluded::Always => format!("{relative} is a temporary sync file."),
        sync::Excluded::Env => format!("{relative} stays on this PC (your launch setting keeps .env files here). Run {} to copy them.", context.change),
    })
}

fn node_hint(context: &Context) -> String {
    match (plan::pc_node_major(), context.node) {
        (Some(pc), Some(node)) if pc != node && package::NODE_MAJORS.contains(&pc) => {
            format!(" Your PC runs Node {pc}: add a .nvmrc file containing {pc} so the container uses the same version.")
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn context(root: &Path) -> Context<'_> {
        Context { root, env_files: false, node: Some(24), change: "npm run yougori-change" }
    }

    fn folder(files: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let path = root.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        root
    }

    #[test]
    fn missing_programs_name_the_folder_or_platform_they_belong_to() {
        let root = folder(&[("package.json", "{}"), ("server/package.json", "{}"), ("server/node_modules/.bin/tsc", "")]);
        let context = context(root.path());
        let output = "\x1b[1m> tsc -p tsconfig.json\x1b[0m\r\nsh: 1: tsc: not found\r\nsh: 1: powershell: not found\r\n";
        assert_eq!(explain(output, &context), [
            "`tsc` is installed in server/ on your PC, but server/'s dependencies are missing in the container; check their install output above.",
            "`powershell` is a Windows program, and the container runs Linux. Choose a command that works on Linux with npm run yougori-change.",
        ]);
        assert_eq!(explain("bash: line 1: docker: command not found\n", &context), ["`docker`: Docker isn't available inside the project container."]);
        assert_eq!(explain("bash: line 2: $'\\r': command not found\n", &context).len(), 1);
    }

    #[test]
    fn missing_modules_and_files_explain_why_the_copy_lacks_them() {
        let root = folder(&[
            ("package.json", r#"{"scripts":{"dev":"vite"}}"#),
            ("server/package.json", r#"{"dependencies":{"express":"4"}}"#),
            ("server/dist/index.js", ""),
            ("server/data/app.db", ""),
            (".gitignore", "data/\n"),
        ]);
        let context = context(root.path());
        let cjs = "Error: Cannot find module 'cors'\nRequire stack:\n- /workspace/server/src/app.js\n";
        assert_eq!(explain(cjs, &context), ["`cors` isn't a dependency in server/package.json. It works on your PC only because it's installed somewhere else there (a parent folder or globally). Add it to server/package.json."]);
        let esm = "Error [ERR_MODULE_NOT_FOUND]: Cannot find package 'express' imported from /workspace/server/src/app.js";
        assert!(explain(esm, &context)[0].starts_with("`express` is listed in server/package.json"));
        let built = "Error: Cannot find module '/workspace/server/dist/index.js'";
        assert!(explain(built, &context).is_empty());
        let data = "Error: ENOENT: no such file or directory, open '/workspace/server/data/app.db'";
        assert!(explain(data, &context).is_empty());
        let platform = "Error: Cannot find module @rollup/rollup-linux-x64-gnu. npm has a bug related to optional dependencies";
        assert!(explain(platform, &context)[0].starts_with("`@rollup/rollup-linux-x64-gnu`, the Linux build of a native package"));
        let quoted = "Error: Cannot find module '@esbuild/linux-arm64'";
        assert!(explain(quoted, &context)[0].contains("the Linux build of a native package"));
        let script = "npm error Missing script: \"prod\"";
        assert_eq!(explain(script, &context), ["package.json has no script named prod (it has dev). Choose another command with npm run yougori-change."]);
    }

    #[test]
    fn runtime_failures_explain_memory_ports_versions_and_env_files() {
        let root = folder(&[("package.json", "{}"), (".env", "KEY=1")]);
        let context = context(root.path());
        assert!(explain("FATAL ERROR: Reached heap limit Allocation failed - JavaScript heap out of memory", &context)[0].starts_with("Node ran out of heap memory"));
        assert_eq!(explain("Error: listen EADDRINUSE: address already in use :::3000", &context), ["Port 3000 is already taken inside the container, usually by the previous server that is still stopping. Press r to restart."]);
        assert!(explain("gyp ERR! cwd /workspace/node_modules/better-sqlite3", &context)[0].starts_with("The native package better-sqlite3 failed to build or load for Node 24 on Linux."));
        assert_eq!(explain("Error: Missing required environment variable DATABASE_URL", &context), [".env files stayed on this PC (your launch setting). If the app reads them, run npm run yougori-change and choose to copy them."]);
        assert!(explain("ModuleNotFoundError: No module named 'flask'", &context)[0].contains("`flask`"));
        assert!(finished("pm2 start server.js").contains("pm2-runtime"));
        assert!(finished("npm run build").contains("must keep running"));
    }
}
