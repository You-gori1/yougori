//! Conservative, file-based ranking. Script names alone never prove an app is running.
use super::*;

#[derive(Clone, Copy, PartialEq)]
enum Role { Build, Watch, Dev, Server, Preview, Unknown }

fn role(step: &Step) -> Role {
    let args = &step.args;
    let first = args.first().map(String::as_str).unwrap_or("");
    if args.iter().any(|a| matches!(a.as_str(), "--help" | "--version" | "--test")) { return Role::Unknown; }
    if matches!(step.name.as_str(), "tsc" | "webpack" | "rollup" | "esbuild") || first == "build" {
        if args.iter().any(|a| matches!(a.as_str(), "--watch" | "-w")) { return Role::Watch; }
    }
    if step.name == "tsc" && args.iter().any(|a| a == "--noEmit") { return Role::Unknown; }
    match step.name.as_str() {
        "webpack" if first == "serve" => Role::Dev,
        "tsc" | "webpack" | "rollup" | "esbuild" => Role::Build,
        "vite" | "next" | "nuxt" | "nuxi" | "astro" | "react-scripts" => match first {
            "build" | "generate" => Role::Build,
            "preview" => Role::Preview,
            "dev" | "serve" => Role::Dev,
            "start" if step.name == "react-scripts" => Role::Dev,
            "start" => Role::Server,
            "" if step.name == "vite" => Role::Dev,
            _ if step.name == "vite" && first.starts_with('-') => Role::Dev,
            _ => Role::Unknown,
        },
        "serve" | "http-server" => Role::Preview,
        "node" | "tsx" | "ts-node" | "nodemon" | "bun" => {
            let target = args.iter().find(|a| a.ends_with(".js") || a.ends_with(".mjs") || a.ends_with(".cjs") || a.ends_with(".ts"));
            // A utility script is not evidence of a listening app.
            let app = target.is_some_and(|p| {
                let basename = p.rsplit('/').next().unwrap_or(p).split('.').next().unwrap_or("");
                matches!(basename, "server" | "index" | "app" | "main") && !p.starts_with("scripts/")
            });
            if !app { Role::Unknown }
            else if matches!(step.name.as_str(), "tsx" | "ts-node" | "nodemon") || args.iter().any(|a| a == "watch" || a.starts_with("--watch")) { Role::Dev }
            else { Role::Server }
        },
        "python" | "python3" if first == "manage.py" && args.iter().any(|a| a == "runserver") => Role::Dev,
        "uvicorn" | "gunicorn" => Role::Server,
        _ => Role::Unknown,
    }
}

fn built_server(step: &Step) -> bool {
    matches!(step.name.as_str(), "next" | "nuxt" | "nuxi" | "astro" | "vite")
        || step.args.iter().any(|a| {
            let path = a.trim_start_matches("./");
            matches!(path, "dist" | "build") || path.starts_with("dist/") || path.starts_with("build/")
        })
}

fn component(step: &Step) -> String {
    let kind = match step.name.as_str() {
        "vite" | "react-scripts" | "serve" | "http-server" | "webpack" | "rollup" => "web",
        "next" | "nuxt" | "nuxi" | "astro" => "app",
        _ => "server",
    };
    format!("{}:{kind}", step.dir)
}

struct Evidence {
    score: (bool, usize, bool, bool),
    reason: String,
    signature: Vec<(String, String, Vec<String>)>,
    needs_build: bool,
    serves: BTreeSet<String>,
    covers: BTreeSet<String>,
}

fn evidence(root: &Path, packages: &mut Packages, command: &str) -> Evidence {
    let expansion = scripts::expand(packages, ".", command);
    let blocked = findings(root, packages, None, &expansion).iter().any(|f| f.blocking);
    let mut builds = BTreeSet::new();
    let mut serves = BTreeSet::new();
    let mut folders = BTreeSet::new();
    let mut needs_build = false;
    let mut dev = false;
    let mut preview = false;
    for step in expansion.steps.iter().filter(|s| !s.optional) {
        match role(step) {
            Role::Build => { builds.insert(component(step)); },
            Role::Dev | Role::Server | Role::Preview => {
                let kind = role(step);
                dev |= kind == Role::Dev;
                preview |= kind == Role::Preview;
                needs_build |= kind != Role::Dev && built_server(step) && !builds.contains(&component(step));
                serves.insert(component(step));
                folders.insert(if step.dir == "." { "project root".into() } else { step.dir.clone() });
            },
            Role::Watch | Role::Unknown => {},
        }
    }
    // A foreground server before && prevents the next server from starting.
    let serial = expansion.sequences.iter().any(|(before, after)| {
        expansion.steps[before.clone()].iter().any(|s| matches!(role(s), Role::Watch | Role::Dev | Role::Server | Role::Preview))
            && expansion.steps[after.clone()].iter().any(|s| matches!(role(s), Role::Dev | Role::Server | Role::Preview))
    });
    let eligible = !blocked && !serves.is_empty() && !needs_build && !serial;
    let covers: BTreeSet<_> = builds.union(&serves).cloned().collect();
    let coverage = covers.len();
    let reason = if serial { "A long-running command prevents a later server from starting".into() }
        else if needs_build { "Needs a build before starting".into() }
        else if serves.len() > 1 { format!("Starts services in {}", folders.iter().cloned().collect::<Vec<_>>().join(", ")) }
        else if !builds.is_empty() && !serves.is_empty() { "Builds the app, then starts its server".into() }
        else if dev { "Starts the development server".into() }
        else if preview { "Previews built files".into() }
        else if !serves.is_empty() { "Starts the application server".into() }
        else { String::new() };
    Evidence {
        // Coverage comes first. Prefer prepared production over development for equal coverage;
        // previews rank below actual application servers. Ties retain project/default order.
        score: (eligible, coverage, !preview, !dev), reason, needs_build, serves, covers,
        signature: expansion.steps.iter().map(|s| (s.dir.clone(), s.name.clone(), s.args.clone())).collect(),
    }
}

pub(super) fn rank(root: &Path, project: &Project, mut candidates: Vec<(String, String)>) -> Vec<RunChoice> {
    let mut packages = Packages::new(root);
    // A bare production start often needs generated output (Next, Nuxt, TypeScript, etc.).
    // Offer the project's own build followed by start; never invent a build command.
    if project.package["scripts"]["build"].as_str().is_some_and(|s| !s.trim().is_empty()) {
        for (command, _) in candidates.clone() {
            if evidence(root, &mut packages, &command).needs_build {
                let combined = format!("{} && {command}", run_script(root, &mut packages, ".", "build"));
                if !candidates.iter().any(|(c, _)| c == &combined) && !evidence(root, &mut packages, &combined).needs_build {
                    candidates.push((combined, "Build and start from your project's scripts".into()));
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    let mut ranked = Vec::new();
    for (command, detail) in candidates {
        let ev = evidence(root, &mut packages, &command);
        // Aliases for the same programs should not crowd out meaningful choices.
        if !ev.signature.is_empty() && !seen.insert((ev.score.0, ev.signature.clone())) { continue; }
        let detail = if ev.reason.is_empty() { detail } else { ev.reason.clone() };
        ranked.push((RunChoice { command, detail, recommended: false }, ev));
    }
    let services: BTreeSet<_> = ranked.iter().flat_map(|(_, ev)| ev.serves.iter().cloned()).collect();
    for (choice, ev) in &mut ranked {
        if ev.score.0 && !services.is_subset(&ev.covers) {
            ev.score.0 = false;
            choice.detail = format!("{}; other project services are separate", choice.detail);
        }
    }
    ranked.sort_by(|a, b| {
        if !a.1.score.0 && !b.1.score.0 { std::cmp::Ordering::Equal }
        else { b.1.score.cmp(&a.1.score) }
    });
    if let Some((choice, ev)) = ranked.first_mut() {
        choice.recommended = ev.score.0;
    }
    ranked.into_iter().map(|(choice, _)| choice).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choices(package: Value) -> Vec<RunChoice> {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("package.json"), package.to_string()).unwrap();
        run_choices(root.path(), &Project::load(root.path()).unwrap())
    }

    fn recommended(choices: &[RunChoice]) -> Option<&str> {
        let found: Vec<_> = choices.iter().filter(|c| c.recommended).collect();
        assert!(found.len() <= 1);
        found.first().map(|c| c.command.as_str())
    }

    #[test]
    fn standalone_frontends_prefer_dev_over_a_static_preview() {
        for (dev, build, preview) in [("vite", "vite build", "vite preview"), ("react-scripts start", "react-scripts build", "serve build")] {
            let menu = choices(serde_json::json!({"scripts":{"dev":dev,"build":build,"preview":preview}}));
            assert_eq!(recommended(&menu), Some("npm run dev"));
        }
    }

    #[test]
    fn framework_production_starts_include_the_existing_build_for_each_manager() {
        for manager in ["npm", "pnpm", "yarn", "bun"] {
            for framework in ["next", "nuxt"] {
                let menu = choices(serde_json::json!({"packageManager":format!("{manager}@1.0.0"),"scripts":{
                    "dev":format!("{framework} dev"),"build":format!("{framework} build"),"start":format!("{framework} start")
                }}));
                assert_eq!(recommended(&menu), Some(format!("{manager} run build && {manager} run start").as_str()));
            }
        }
    }

    #[test]
    fn backend_source_runs_directly_but_typescript_output_needs_a_build() {
        let menu = choices(serde_json::json!({"scripts":{"dev":"nodemon server.js","start":"node server.js"}}));
        assert_eq!(recommended(&menu), Some("npm run start"));
        let menu = choices(serde_json::json!({"scripts":{"dev":"tsx watch src/index.ts","build":"tsc","start":"node dist/index.js"}}));
        assert_eq!(recommended(&menu), Some("npm run build && npm run start"));
        let menu = choices(serde_json::json!({"scripts":{"dev":"next dev","start":"next start"}}));
        assert_eq!(recommended(&menu), Some("npm run dev"), "never invent a missing build");
    }

    #[test]
    fn lifecycle_builds_are_kept_and_windows_builds_are_not_recommended() {
        let menu = choices(serde_json::json!({"scripts":{
            "dev":"next dev","prestart":"next build","start":"next start","build":"next build"
        }}));
        assert_eq!(recommended(&menu), Some("npm run start"));
        let menu = choices(serde_json::json!({"scripts":{
            "dev":"next dev","start":"next start","build":"powershell -File build.ps1"
        }}));
        assert_eq!(recommended(&menu), Some("npm run dev"));
    }

    #[test]
    fn full_stack_runner_beats_individual_services_even_in_one_package() {
        for runner in ["concurrently \"npm:dev:web\" \"npm:dev:api\"", "run-p dev:web dev:api"] {
            let menu = choices(serde_json::json!({"scripts":{
                "dev":"npm run dev:web","dev:web":"vite","dev:api":"tsx watch src/index.ts","dev:full":runner
            }}));
            assert_eq!(recommended(&menu), Some("npm run dev:full"));
            assert!(!menu.iter().any(|c| c.command == "npm run dev:web"), "deduplicate aliases");
        }
    }

    #[test]
    fn incomplete_or_sequential_service_sets_have_no_recommendation() {
        for runner in ["npm run dev:web && npm run dev:api", "run-s dev:*", "echo choose a service"] {
            let menu = choices(serde_json::json!({"scripts":{
                "dev":runner,"dev:web":"vite","dev:api":"tsx watch src/index.ts"
            }}));
            assert_eq!(recommended(&menu), None, "{runner}");
        }
    }

    #[test]
    fn misleading_names_windows_commands_and_opaque_orchestrators_are_not_recommended() {
        for command in ["vite build", "echo hello", "node scripts/setup.js", "powershell -File run.ps1", "turbo run dev", "npm run missing", "npm run dev", "tsc --watch && node dist/index.js", "tsc --noEmit && node dist/index.js"] {
            let menu = choices(serde_json::json!({"scripts":{"dev":command}}));
            assert_eq!(recommended(&menu), None, "{command}");
        }
    }

    #[test]
    fn python_django_is_known_but_arbitrary_python_files_are_not_assumed_to_be_servers() {
        for (file, expected) in [("manage.py", true), ("main.py", false)] {
            let root = tempfile::tempdir().unwrap();
            fs::write(root.path().join(file), "").unwrap();
            let menu = run_choices(root.path(), &Project::load(root.path()).unwrap());
            assert_eq!(recommended(&menu).is_some(), expected);
        }
    }
}
