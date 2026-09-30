//! What a command really runs, read without running it: shell operators, package-manager commands
//! and the project's scripts, followed into every package folder they reach.
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// The operator before a command: `a && b` gives `b` the join `And`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Join {
    First,
    And,
    Or,
    Then,
    Pipe,
    Background,
}

#[derive(Debug, PartialEq)]
pub struct Part {
    pub text: String,
    pub join: Join,
}

/// Splits a shell line at `&&`, `||`, `;`, `|`, `&` and newlines outside quotes and `$(...)`.
/// Grouping parentheses are dropped; redirections such as `2>&1` stay inside their command.
pub fn split(line: &str) -> Vec<Part> {
    let chars: Vec<char> = line.chars().collect();
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut join = Join::First;
    let mut quote: Option<char> = None;
    let mut nested = 0usize;
    let mut i = 0;
    let flush = |current: &mut String, join: Join, parts: &mut Vec<Part>| {
        let text = current.trim();
        if !text.is_empty() {
            parts.push(Part { text: text.into(), join });
        }
        current.clear();
    };
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if let Some(q) = quote {
            current.push(c);
            if c == '\\' && q != '\'' {
                if let Some(n) = next {
                    current.push(n);
                    i += 1;
                }
            } else if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            '\\' => {
                current.push(c);
                if let Some(n) = next {
                    current.push(n);
                    i += 1;
                }
            }
            '\'' | '"' | '`' => {
                quote = Some(c);
                current.push(c);
            }
            '(' if nested > 0 || current.ends_with('$') => {
                nested += 1;
                current.push(c);
            }
            ')' if nested > 0 => {
                nested -= 1;
                current.push(c);
            }
            '(' | ')' => current.push(' '),
            '&' if current.ends_with(['>', '<']) || next == Some('>') => current.push(c),
            '|' if current.ends_with('>') => current.push(c),
            '&' | '|' | ';' | '\n' if nested == 0 => {
                let operator = match (c, next) {
                    ('&', Some('&')) => {
                        i += 1;
                        Join::And
                    }
                    ('|', Some('|')) => {
                        i += 1;
                        Join::Or
                    }
                    ('|', Some('&')) => {
                        i += 1;
                        Join::Pipe
                    }
                    ('|', _) => Join::Pipe,
                    ('&', _) => Join::Background,
                    _ => Join::Then,
                };
                flush(&mut current, join, &mut parts);
                join = operator;
            }
            _ => current.push(c),
        }
        i += 1;
    }
    flush(&mut current, join, &mut parts);
    parts
}

/// What one simple command does, as far as launching a project is concerned.
#[derive(Debug, PartialEq)]
pub enum Command {
    /// A package script. `dir` is relative to the current folder; `bin` falls back to a program
    /// of that name (yarn, pnpm and bun shorthands); `hooks` runs pre/post scripts (npm, yarn).
    Script { dir: Option<String>, name: String, bin: bool, hooks: bool, if_present: bool },
    /// npm-run-all style script names, which may contain `*`.
    Scripts { patterns: Vec<String> },
    Install { dir: Option<String> },
    Cd(String),
    /// A program. `fetched` when npx-like tools download it if it is missing.
    Program { name: String, args: Vec<String>, fetched: bool },
    /// Complete command lines run by concurrently, `sh -c` and similar.
    Lines(Vec<String>),
    /// run-script-os: the calling script's platform variant.
    Os,
    Other,
}

fn assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && !name.starts_with(|c: char| c.is_ascii_digit())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// The words after environment assignments and wrappers that only prepare the command they run.
fn unwrapped(words: &[String]) -> Result<Vec<&str>, Command> {
    let mut words: Vec<&str> = words.iter().map(String::as_str).collect();
    loop {
        while words.first().is_some_and(|w| assignment(w)) {
            words.remove(0);
        }
        match words.first().copied() {
            None => return Err(Command::Other),
            Some("cross-env" | "nohup" | "time" | "exec" | "command" | "nice") => {
                words.remove(0);
            }
            Some("env") => {
                words.remove(0);
                while words.first().is_some_and(|w| w.starts_with('-')) {
                    if words.remove(0) == "-u" && !words.is_empty() {
                        words.remove(0);
                    }
                }
            }
            Some("dotenv" | "dotenvx" | "env-cmd") => match words.iter().position(|w| *w == "--") {
                Some(end) => {
                    words.drain(..=end);
                }
                None => return Err(Command::Other),
            },
            Some("cross-env-shell") => {
                let rest: Vec<&str> = words[1..].iter().copied().filter(|w| !assignment(w)).collect();
                return Err(Command::Lines(vec![rest.join(" ")]));
            }
            Some(_) => return Ok(words),
        }
    }
}

pub fn command(words: &[String]) -> Command {
    let words = match unwrapped(words) {
        Ok(words) => words,
        Err(command) => return command,
    };
    let (program, rest) = (words[0], &words[1..]);
    match program {
        "npm" => npm(rest),
        "npx" | "pnpx" | "bunx" => fetched(rest),
        "yarn" | "pnpm" | "bun" => runner(program, rest),
        "concurrently" | "conc" => concurrently(rest),
        "npm-run-all" | "npm-run-all2" | "run-p" | "run-s" => Command::Scripts {
            patterns: rest
                .iter()
                .enumerate()
                .filter(|(i, w)| !w.starts_with('-') && !(*i > 0 && matches!(rest[i - 1], "--max-parallel" | "--npm-path")))
                .filter_map(|(_, w)| w.split_whitespace().next().map(str::to_owned))
                .collect(),
        },
        "run-script-os" => Command::Os,
        "turbo" | "nx" | "lerna" => Command::Other,
        // Shell keywords: what they guard is conditional.
        "if" | "then" | "else" | "elif" | "fi" | "for" | "do" | "done" | "while" | "until" | "case" | "esac" | "{" | "}" | "!" | "[[" | "function" => Command::Other,
        "cd" | "pushd" => rest
            .iter()
            .find(|w| !w.starts_with('-'))
            .map_or(Command::Other, |dir| Command::Cd((*dir).into())),
        "sh" | "bash" | "dash" | "zsh" if rest.contains(&"-c") => {
            let at = rest.iter().position(|w| *w == "-c").unwrap_or(0);
            rest.get(at + 1).map_or(Command::Other, |line| Command::Lines(vec![(*line).into()]))
        }
        _ => Command::Program { name: program.into(), args: rest.iter().map(|w| (*w).into()).collect(), fetched: false },
    }
}

const INSTALL: [&str; 17] = [
    "install", "i", "in", "ins", "inst", "insta", "instal", "isnt", "isnta", "isntal", "isntall",
    "add", "ci", "clean-install", "ic", "install-clean", "isntall-clean",
];

fn npm(words: &[&str]) -> Command {
    let (mut dir, mut positional, mut if_present) = (None, Vec::new(), false);
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        match word {
            "--" => break,
            "--prefix" | "-C" => {
                dir = words.get(i + 1).map(|d| (*d).to_owned());
                i += 1;
            }
            // Workspace selection runs in members the root install already covers.
            "--workspace" | "-w" | "--workspaces" | "-ws" => return Command::Other,
            "--loglevel" | "--userconfig" | "--cache" | "--registry" => i += 1,
            "--if-present" => if_present = true,
            _ if word.starts_with("--prefix=") => dir = Some(word["--prefix=".len()..].into()),
            _ if word.starts_with('-') => {}
            _ => positional.push(word),
        }
        i += 1;
    }
    let script = |name: &str| Command::Script { dir: dir.clone(), name: name.into(), bin: false, hooks: true, if_present };
    match positional.first().copied() {
        Some("run" | "run-script" | "rum" | "urn") => positional.get(1).map_or(Command::Other, |name| script(name)),
        Some(name @ ("start" | "stop" | "restart")) => script(name),
        Some("test" | "t" | "tst") => script("test"),
        Some(sub) if INSTALL.contains(&sub) => Command::Install { dir },
        Some("exec" | "x") => fetched(&words[words.iter().position(|w| matches!(*w, "exec" | "x")).unwrap_or(0) + 1..]),
        _ => Command::Other,
    }
}

fn fetched(words: &[&str]) -> Command {
    let mut i = 0;
    while i < words.len() {
        match words[i] {
            "-p" | "--package" | "-c" | "--call" => i += 2,
            word if word.starts_with('-') => i += 1,
            name => {
                return Command::Program {
                    name: name.into(),
                    args: words[i + 1..].iter().map(|w| (*w).into()).collect(),
                    fetched: true,
                }
            }
        }
    }
    Command::Other
}

/// yarn, pnpm and bun: `TOOL NAME` runs script NAME, or else a program called NAME.
fn runner(tool: &str, words: &[&str]) -> Command {
    let (mut dir, mut positional) = (None, Vec::new());
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        match word {
            "--cwd" | "-C" | "--dir" => {
                dir = words.get(i + 1).map(|d| (*d).to_owned());
                i += 1;
            }
            _ if positional.is_empty() && matches!(word, "--filter" | "-F" | "-r" | "--recursive" | "-w" | "--workspace-root" | "workspace" | "workspaces") => return Command::Other,
            _ if word.starts_with("--cwd=") || word.starts_with("--dir=") => dir = word.split_once('=').map(|(_, d)| d.to_owned()),
            _ if word.starts_with('-') => {}
            _ => positional.push(word),
        }
        i += 1;
    }
    let hooks = tool == "yarn";
    let script = |name: &str, bin: bool| {
        if tool == "bun" && (name.contains('/') || name.rsplit_once('.').is_some_and(|(_, ext)| matches!(ext, "js" | "mjs" | "cjs" | "ts" | "mts" | "cts" | "tsx" | "jsx"))) {
            return Command::Program { name: "bun".into(), args: vec![name.into()], fetched: false };
        }
        Command::Script { dir: dir.clone(), name: name.into(), bin, hooks, if_present: false }
    };
    match positional.first().copied() {
        None if tool == "yarn" => Command::Install { dir },
        None => Command::Other,
        Some("install" | "i" | "add") => Command::Install { dir },
        Some("run") => positional.get(1).map_or(Command::Other, |name| script(name, true)),
        Some("exec") => positional.get(1).map_or(Command::Other, |name| Command::Program { name: (*name).into(), args: Vec::new(), fetched: false }),
        Some("dlx" | "x") => positional.get(1).map_or(Command::Other, |name| Command::Program { name: (*name).into(), args: Vec::new(), fetched: true }),
        Some("node") if tool == "yarn" => Command::Program { name: "node".into(), args: Vec::new(), fetched: false },
        Some(name @ ("start" | "test")) => script(name, false),
        Some("remove" | "rm" | "upgrade" | "up" | "update" | "link" | "unlink" | "why" | "info" | "outdated" | "dedupe" | "prune" | "publish" | "pack" | "create" | "init" | "config" | "store" | "cache") => Command::Other,
        Some(name) => script(name, true),
    }
}

fn concurrently(words: &[&str]) -> Command {
    const VALUE: [&str; 20] = [
        "-n", "--names", "-c", "--prefix-colors", "-p", "--prefix", "-s", "--success", "-t",
        "--timestamp-format", "-l", "--prefix-length", "-m", "--max-processes", "--restart-tries",
        "--restart-after", "--default-input-target", "--hide", "--teardown", "--name-separator",
    ];
    let mut lines = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        if VALUE.contains(&word) {
            i += 1;
        } else if !word.starts_with('-') {
            // `npm:dev` is concurrently's shorthand for `npm run dev`.
            let shorthand = ["npm", "yarn", "pnpm", "bun"]
                .into_iter()
                .find_map(|tool| word.strip_prefix(tool).and_then(|r| r.strip_prefix(':')).map(|script| format!("{tool} run {script}")));
            lines.push(shorthand.unwrap_or_else(|| word.into()));
        }
        i += 1;
    }
    Command::Lines(lines)
}

/// A folder relative to the project, "." for the project itself. None when it leaves the project
/// or cannot be known (variables, home folders).
pub fn join(base: &str, path: &str) -> Option<String> {
    let path = path.trim_matches(|c| c == '"' || c == '\'');
    if path.is_empty() || path.contains('$') || path.starts_with('~') || path.contains('%') {
        return None;
    }
    let (mut parts, path): (Vec<&str>, &str) = match path.strip_prefix("/workspace") {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => (Vec::new(), rest),
        _ if path.starts_with('/') => return None,
        _ => (base.split('/').filter(|p| !p.is_empty() && *p != ".").collect(), path),
    };
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(if parts.is_empty() { ".".into() } else { parts.join("/") })
}

/// A program the command runs, and where.
#[derive(Clone, Debug)]
pub struct Step {
    pub dir: String,
    pub name: String,
    pub args: Vec<String>,
    pub fetched: bool,
    /// After `||`, or in a script run with --if-present: its failure is expected.
    pub optional: bool,
    /// The script whose text contains the step (None: the command itself), and which of that
    /// text's top-level commands it is part of.
    pub owner: Option<(String, String)>,
    pub part: usize,
}

impl Step {
    pub fn origin(&self) -> String {
        match &self.owner {
            None => "your command".into(),
            Some((dir, name)) if dir == "." => format!("scripts.{name}"),
            Some((dir, name)) => format!("{dir}/package.json scripts.{name}"),
        }
    }
}

#[derive(Debug)]
pub struct MissingScript {
    pub dir: String,
    pub name: String,
    pub origin: String,
}

#[derive(Default)]
pub struct Expansion {
    pub steps: Vec<Step>,
    /// Step ranges separated by a sequential shell operator or script runner.
    pub sequences: Vec<(std::ops::Range<usize>, std::ops::Range<usize>)>,
    pub missing: Vec<MissingScript>,
    /// Package folders whose scripts the command runs.
    pub folders: BTreeSet<String>,
    /// Package folders the command installs itself.
    pub installs: BTreeSet<String>,
    /// Package managers other than npm that the command calls (yarn, pnpm, bun).
    pub runners: BTreeSet<String>,
}

/// Package manifests read from the PC, by folder.
pub struct Packages<'a> {
    root: &'a Path,
    cache: BTreeMap<String, Option<Value>>,
}

impl<'a> Packages<'a> {
    pub fn new(root: &'a Path) -> Self {
        Self { root, cache: BTreeMap::new() }
    }
    pub fn get(&mut self, dir: &str) -> Option<&Value> {
        let root = self.root;
        self.cache
            .entry(dir.into())
            .or_insert_with(|| {
                let bytes = std::fs::read(root.join(dir).join("package.json")).ok()?;
                serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes)).ok()
            })
            .as_ref()
    }
    pub fn script(&mut self, dir: &str, name: &str) -> Option<String> {
        self.get(dir)?["scripts"][name].as_str().map(str::to_owned)
    }
    pub fn root(&self) -> &Path {
        self.root
    }
}

struct Walk<'p, 'a> {
    packages: &'p mut Packages<'a>,
    out: Expansion,
    stack: Vec<(String, String)>,
}

/// Every program `line` runs when started in `dir`, following scripts across package folders.
pub fn expand(packages: &mut Packages, dir: &str, line: &str) -> Expansion {
    let mut walk = Walk { packages, out: Expansion::default(), stack: Vec::new() };
    walk.line(line, dir, false);
    walk.out
}

impl Walk<'_, '_> {
    fn line(&mut self, line: &str, dir: &str, optional: bool) {
        if self.stack.len() > 24 || self.out.steps.len() > 400 {
            return;
        }
        let mut cwd = dir.to_owned();
        let parts = split(line);
        let line_start = self.out.steps.len();
        for (index, part) in parts.iter().enumerate() {
            let step_start = self.out.steps.len();
            // Either side of `||` may fail without failing the command.
            let optional = optional || part.join == Join::Or || parts.get(index + 1).is_some_and(|next| next.join == Join::Or);
            let Ok(words) = shell_words::split(&part.text) else { continue };
            if let Some(runner) = unwrapped(&words).ok().and_then(|w| w.first().copied()) {
                match runner {
                    "yarn" | "yarnpkg" => self.out.runners.insert("yarn".into()),
                    "pnpm" | "pnpx" => self.out.runners.insert("pnpm".into()),
                    "bun" | "bunx" => self.out.runners.insert("bun".into()),
                    _ => false,
                };
            }
            match command(&words) {
                Command::Cd(target) => {
                    if let Some(next) = join(&cwd, &target) {
                        cwd = next;
                    }
                }
                Command::Script { dir, name, bin, hooks, if_present } => {
                    if let Some(target) = dir.map_or(Some(cwd.clone()), |d| join(&cwd, &d)) {
                        self.script(&target, &name, bin, hooks, optional || if_present, index, &words);
                    }
                }
                Command::Scripts { patterns } => {
                    let runner_start = self.out.steps.len();
                    let sequential = !words.iter().any(|w| matches!(w.as_str(), "run-p" | "--parallel" | "-p"));
                    for pattern in patterns {
                        let names = if pattern.contains('*') {
                            self.packages.get(&cwd).and_then(|p| p["scripts"].as_object())
                                .map(|s| s.keys().filter(|n| wildcard(&pattern, n)).cloned().collect::<Vec<_>>()).unwrap_or_default()
                        } else { vec![pattern] };
                        for name in names {
                            let start = self.out.steps.len();
                            self.script(&cwd, &name, false, true, optional, index, &words);
                            if sequential && start > runner_start {
                                self.out.sequences.push((runner_start..start, start..self.out.steps.len()));
                            }
                        }
                    }
                }
                Command::Install { dir } => {
                    if let Some(target) = dir.map_or(Some(cwd.clone()), |d| join(&cwd, &d)) {
                        self.out.installs.insert(target);
                    }
                }
                Command::Program { name, args, fetched } => self.out.steps.push(Step {
                    dir: cwd.clone(), name, args, fetched, optional, owner: self.stack.last().cloned(), part: index,
                }),
                Command::Lines(lines) => {
                    for line in lines {
                        self.line(&line, &cwd, optional);
                    }
                }
                Command::Os => {
                    if let Some((dir, name)) = self.stack.last().cloned() {
                        if let Some(variant) = [":linux", ":nix", ":default"].iter().map(|s| format!("{name}{s}")).find(|v| self.packages.script(&dir, v).is_some()) {
                            self.script(&dir, &variant, false, false, optional, index, &words);
                        }
                    }
                }
                Command::Other => {}
            }
            if matches!(part.join, Join::And | Join::Then) && step_start > line_start {
                self.out.sequences.push((line_start..step_start, step_start..self.out.steps.len()));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn script(&mut self, dir: &str, name: &str, bin: bool, hooks: bool, optional: bool, part: usize, words: &[String]) {
        if name.contains('*') {
            let names: Vec<String> = self.packages.get(dir).and_then(|p| p["scripts"].as_object()).map(|scripts| scripts.keys().filter(|n| wildcard(name, n)).cloned().collect()).unwrap_or_default();
            for name in names {
                self.script(dir, &name, false, hooks, optional, part, words);
            }
            return;
        }
        let Some(body) = self.packages.script(dir, name) else {
            if name == "start" && self.packages.root().join(dir).join("server.js").is_file() {
                // npm start without a start script runs server.js.
                self.out.steps.push(Step { dir: dir.into(), name: "node".into(), args: vec!["server.js".into()], fetched: false, optional, owner: self.stack.last().cloned(), part });
            } else if bin {
                let args = words.iter().skip_while(|w| *w != name).skip(1).cloned().collect();
                self.out.steps.push(Step { dir: dir.into(), name: name.into(), args, fetched: false, optional, owner: self.stack.last().cloned(), part });
            } else if !optional {
                self.missing(dir, name);
            }
            return;
        };
        let key = (dir.to_owned(), name.to_owned());
        if self.stack.contains(&key) {
            return;
        }
        self.out.folders.insert(dir.into());
        if hooks {
            self.hook(dir, &format!("pre{name}"), optional);
        }
        self.stack.push(key);
        self.line(&body, dir, optional);
        self.stack.pop();
        if hooks {
            self.hook(dir, &format!("post{name}"), optional);
        }
    }

    /// npm and yarn run `preNAME` and `postNAME` around script NAME.
    fn hook(&mut self, dir: &str, name: &str, optional: bool) {
        let Some(body) = self.packages.script(dir, name) else { return };
        let key = (dir.to_owned(), name.to_owned());
        if !self.stack.contains(&key) {
            self.stack.push(key);
            self.line(&body, dir, optional);
            self.stack.pop();
        }
    }

    fn missing(&mut self, dir: &str, name: &str) {
        let origin = self.stack.last().map_or("your command".into(), |(d, n)| if d == "." { format!("scripts.{n}") } else { format!("{d}/package.json scripts.{n}") });
        self.out.missing.push(MissingScript { dir: dir.into(), name: name.into(), origin });
    }
}

/// `*` matches any run of characters, as npm-run-all and concurrently use it for script names.
fn wildcard(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, tail)) => name.strip_prefix(head).is_some_and(|rest| (0..=rest.len()).any(|i| rest.is_char_boundary(i) && wildcard(tail, &rest[i..]))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn splits_at_shell_operators_but_not_inside_quotes_substitutions_or_redirections() {
        let parts = split("npm run build && powershell -File 'a && b.ps1' 2>&1 || echo $(date; true) ; (cd x && y) &>log | tee z");
        let texts: Vec<_> = parts.iter().map(|p| (p.text.as_str(), p.join)).collect();
        assert_eq!(texts, [
            ("npm run build", Join::First),
            ("powershell -File 'a && b.ps1' 2>&1", Join::And),
            ("echo $(date; true)", Join::Or),
            ("cd x", Join::Then),
            ("y  &>log", Join::And),
            ("tee z", Join::Pipe),
        ]);
    }

    #[test]
    fn reads_package_manager_commands_wrappers_and_folders() {
        let words = |s: &str| shell_words::split(s).unwrap();
        assert_eq!(command(&words("npm --prefix server run build")), Command::Script { dir: Some("server".into()), name: "build".into(), bin: false, hooks: true, if_present: false });
        assert_eq!(command(&words("npm run lint --if-present --prefix=web")), Command::Script { dir: Some("web".into()), name: "lint".into(), bin: false, hooks: true, if_present: true });
        assert_eq!(command(&words("cross-env NODE_ENV=production PORT=1 npm start")), Command::Script { dir: None, name: "start".into(), bin: false, hooks: true, if_present: false });
        assert_eq!(command(&words("npm ci --prefix client")), Command::Install { dir: Some("client".into()) });
        assert_eq!(command(&words("yarn --cwd api dev")), Command::Script { dir: Some("api".into()), name: "dev".into(), bin: true, hooks: true, if_present: false });
        assert_eq!(command(&words("pnpm -C web install")), Command::Install { dir: Some("web".into()) });
        assert_eq!(command(&words("yarn")), Command::Install { dir: None });
        assert!(matches!(command(&words("npx -y serve dist")), Command::Program { name, fetched: true, .. } if name == "serve"));
        assert!(matches!(command(&words("bun src/index.ts")), Command::Program { name, .. } if name == "bun"));
        assert_eq!(command(&words("concurrently -n a,b \"npm:dev:*\" 'vite --port 1'")), Command::Lines(vec!["npm run dev:*".into(), "vite --port 1".into()]));
        assert_eq!(command(&words("dotenv -e .env -- node app.js")), Command::Program { name: "node".into(), args: vec!["app.js".into()], fetched: false });
        assert_eq!(command(&words("run-p --max-parallel 2 watch:* 'serve -- --x'")), Command::Scripts { patterns: vec!["watch:*".into(), "serve".into()] });
        assert_eq!(command(&words("npm run -w web dev")), Command::Other);
        assert_eq!(join("server", "../client/./src"), Some("client/src".into()));
        assert_eq!(join(".", "/workspace/api"), Some("api".into()));
        assert_eq!(join(".", ".."), None);
        assert_eq!(join(".", "$HOME"), None);
    }

    #[test]
    fn follows_scripts_hooks_and_folders_to_the_programs_they_run() {
        let root = tempfile::tempdir().unwrap();
        let write = |path: &str, text: &str| {
            let path = root.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        write("package.json", r#"{"scripts":{
            "production":"npm run build && powershell -NoProfile -File scripts/restart.ps1",
            "prebuild":"echo pre",
            "build":"npm --prefix server run build && npm --prefix client run build",
            "dev":"concurrently \"npm:dev:*\"","dev:api":"cd server && npm run dev","dev:web":"npm run nope || true",
            "loop":"npm run loop","os":"run-script-os","os:win32":"del x","os:linux":"rm x",
            "setup":"npm install && cd client && npm ci"}}"#);
        write("server/package.json", r#"{"scripts":{"build":"tsc -p .","dev":"tsx watch src"}}"#);
        write("client/package.json", r#"{"scripts":{"build":"tsc --noEmit && vite build"}}"#);
        let mut packages = Packages::new(root.path());
        let production = expand(&mut packages, ".", "npm run production");
        let steps: Vec<_> = production.steps.iter().map(|s| (s.dir.as_str(), s.name.as_str(), s.origin(), s.part)).collect();
        assert_eq!(steps, [
            (".", "echo", "scripts.prebuild".into(), 0),
            ("server", "tsc", "server/package.json scripts.build".into(), 0),
            ("client", "tsc", "client/package.json scripts.build".into(), 0),
            ("client", "vite", "client/package.json scripts.build".into(), 1),
            (".", "powershell", "scripts.production".into(), 1),
        ]);
        assert_eq!(production.folders, BTreeSet::from([".".into(), "server".into(), "client".into()]));
        let dev = expand(&mut packages, ".", "npm run dev");
        assert!(dev.steps.iter().any(|s| s.dir == "server" && s.name == "tsx"));
        assert!(dev.missing.is_empty(), "a script after || is optional: {:?}", dev.missing);
        assert!(expand(&mut packages, ".", "npm run loop").steps.is_empty());
        assert_eq!(expand(&mut packages, ".", "npm run os").steps[0].name, "rm");
        let missing = expand(&mut packages, ".", "npm run prod");
        assert_eq!((missing.missing[0].name.as_str(), missing.missing[0].origin.as_str()), ("prod", "your command"));
        assert_eq!(expand(&mut packages, ".", "npm run setup").installs, BTreeSet::from([".".into(), "client".into()]));
        assert!(wildcard("dev:*", "dev:api") && !wildcard("dev:*", "build"));
    }
}
