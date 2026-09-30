use super::*;
use std::collections::HashMap;

fn scalar(v: &Value) -> Result<String, String> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        _ => Err("Expected a scalar value".into()),
    }
}
fn supported(
    v: &serde_json::Map<String, Value>,
    keys: &[&str],
    context: &str,
) -> Result<(), String> {
    for key in v.keys() {
        if !keys.contains(&key.as_str()) && !key.starts_with("x-") {
            return Err(format!("{context}: '{key}' cannot be translated safely yet. Remove it or configure this feature in Yougori explicitly; nothing was imported."));
        }
    }
    Ok(())
}
fn env_file(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let text = read(path)?;
    let mut result = BTreeMap::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let (k, v) = line
            .split_once('=')
            .ok_or_else(|| format!("{}:{}: expected KEY=value", path.display(), i + 1))?;
        let v = v.trim();
        let value = if v.starts_with('\'') && v.ends_with('\'') {
            v[1..v.len() - 1].to_owned()
        } else if v.starts_with('"') && v.ends_with('"') {
            serde_json::from_str::<String>(v).map_err(|_| "Invalid quoted environment value")?
        } else {
            v.split(" #").next().unwrap_or(v).trim_end().to_owned()
        };
        result.insert(k.trim().into(), value);
    }
    Ok(result)
}
pub fn interpolate(text: &str, env: &HashMap<String, String>) -> Result<String, String> {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'$') {
            chars.next();
            out.push('$');
            continue;
        }
        let expr = if chars.peek() == Some(&'{') {
            chars.next();
            let mut e = String::new();
            loop {
                match chars.next() {
                    Some('}') => break,
                    Some(c) => e.push(c),
                    None => return Err("Unclosed Compose variable".into()),
                }
            }
            e
        } else {
            let mut e = String::new();
            while chars
                .peek()
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
            {
                e.push(chars.next().unwrap())
            }
            if e.is_empty() {
                out.push('$');
                continue;
            }
            e
        };
        let mut matched = false;
        for op in [":-", ":?", ":+", "-", "?", "+"] {
            if let Some((key, fallback)) = expr.split_once(op) {
                let value = env.get(key);
                let present = value.is_some_and(|v| !op.starts_with(':') || !v.is_empty());
                match op.trim_start_matches(':') {
                    "-" => out.push_str(if present { value.unwrap() } else { fallback }),
                    "+" => out.push_str(if present { fallback } else { "" }),
                    "?" => {
                        if present {
                            out.push_str(value.unwrap())
                        } else {
                            return Err(format!("Required Compose variable {key}: {fallback}"));
                        }
                    }
                    _ => unreachable!(),
                }
                matched = true;
                break;
            }
        }
        if !matched {
            out.push_str(env.get(&expr).ok_or_else(|| {
                format!("Set Compose variable {expr} in your shell or project .env file")
            })?)
        }
    }
    Ok(out)
}
fn expand(value: &mut Value, env: &HashMap<String, String>) -> Result<(), String> {
    match value {
        Value::String(s) => *s = interpolate(s, env)?,
        Value::Array(a) => {
            for v in a {
                expand(v, env)?
            }
        }
        Value::Object(o) => {
            for v in o.values_mut() {
                expand(v, env)?
            }
        }
        _ => {}
    }
    Ok(())
}
fn command(v: &Value) -> Result<Option<Command>, String> {
    if v.is_null() {
        return Ok(None);
    }
    if let Some(s) = v.as_str() {
        // Compose string commands are argv, not an implicit shell.
        return Ok(Some(Command::Args(
            shell_words::split(s).map_err(|e| e.to_string())?,
        )));
    }
    Ok(Some(Command::Args(
        v.as_array()
            .ok_or("Command must be a string or list")?
            .iter()
            .map(scalar)
            .collect::<Result<_, _>>()?,
    )))
}
pub fn convert(path: &Path) -> Result<Project, String> {
    let directory = path.parent().ok_or("Compose file needs a directory")?;
    let mut env: HashMap<String, String> = if directory.join(".env").is_file() {
        env_file(&directory.join(".env"))?.into_iter().collect()
    } else {
        HashMap::new()
    };
    env.extend(std::env::vars());
    convert_text(&read(path)?, directory, &env)
}
pub fn convert_text(
    text: &str,
    directory: &Path,
    env: &HashMap<String, String>,
) -> Result<Project, String> {
    let mut v: Value =
        serde_yaml_ng::from_str(text).map_err(|e| format!("Invalid Compose YAML: {e}"))?;
    expand(&mut v, env)?;
    let root = v.as_object().ok_or("Compose must be a mapping")?;
    supported(
        root,
        &["name", "version", "services", "volumes", "networks"],
        "Compose",
    )?;
    let mut declared_volumes = BTreeSet::new();
    if let Some(volumes) = v.get("volumes").and_then(Value::as_object) {
        for (name, config) in volumes {
            if !config.is_null() && config.as_object().is_none_or(|o| !o.is_empty()) {
                return Err(format!(
                    "Volume {name}: external/custom volume drivers cannot be imported"
                ));
            }
            declared_volumes.insert(name.clone());
        }
    }
    if let Some(networks) = v.get("networks").and_then(Value::as_object) {
        for (name, config) in networks {
            if !config.is_null()
                && config.as_object().is_none_or(|o| {
                    o.keys().any(|k| k != "driver")
                        || o.get("driver").is_some_and(|d| d != "bridge")
                })
            {
                return Err(format!(
                    "Network {name}: external/custom networks cannot be imported"
                ));
            }
        }
    }
    let project = v["name"].as_str().map(str::to_owned).unwrap_or_else(|| {
        directory
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect()
    });
    let services = v["services"]
        .as_object()
        .ok_or("Compose requires services")?;
    let mut p = Project {
        project,
        environments: BTreeMap::new(),
        connections: Vec::new(),
        publish: BTreeMap::new(),
    };
    let mut service_networks: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (key, s) in services {
        let o = s.as_object().ok_or("Service must be a mapping")?;
        supported(
            o,
            &[
                "image",
                "container_name",
                "command",
                "entrypoint",
                "environment",
                "env_file",
                "ports",
                "expose",
                "volumes",
                "working_dir",
                "user",
                "restart",
                "depends_on",
                "cpus",
                "mem_limit",
                "networks",
                "init",
                "deploy",
            ],
            key,
        )?;
        if s.get("init").is_some_and(|v| v != false) {
            return Err(format!(
                "{key}: init:true needs an init process in the image"
            ));
        }
        let mut e=Environment{image:s["image"].as_str().ok_or_else(||format!("{key}: an image is required; build-only projects need a built OCI image first"))?.into(),internet:true,..Default::default()};
        e.command = command(&s["command"])?;
        e.entrypoint = command(&s["entrypoint"])?;
        for (field, dest) in [("working_dir", &mut e.working_dir), ("user", &mut e.user)] {
            *dest = s.get(field).map(scalar).transpose()?
        }
        e.restart = s["restart"].as_str().unwrap_or("no").into();
        if let Some(n) = s.get("cpus") {
            e.cpu = scalar(n)?.parse().map_err(|_| "Invalid CPU limit")?
        }
        if let Some(m) = s.get("mem_limit") {
            e.memory = m.clone()
        }
        if let Some(deploy) = s.get("deploy").and_then(Value::as_object) {
            supported(deploy, &["resources"], key)?;
            let resources = deploy
                .get("resources")
                .and_then(Value::as_object)
                .ok_or("deploy.resources must be a mapping")?;
            supported(resources, &["limits", "reservations"], key)?;
            if let Some(limits) = resources.get("limits").and_then(Value::as_object) {
                supported(limits, &["cpus", "memory"], key)?;
                if let Some(cpu) = limits.get("cpus") {
                    e.cpu = scalar(cpu)?.parse().map_err(|_| "Invalid CPUs")?
                }
                if let Some(memory) = limits.get("memory") {
                    e.memory = memory.clone()
                }
            }
            if let Some(res) = resources.get("reservations").and_then(Value::as_object) {
                supported(res, &["devices"], key)?;
                if let Some(devices) = res.get("devices").and_then(Value::as_array) {
                    for d in devices {
                        let d = d.as_object().ok_or("Invalid GPU reservation")?;
                        supported(d, &["driver", "capabilities", "count"], key)?;
                        if d.get("driver").is_some_and(|v| v != "nvidia")
                            || !d
                                .get("capabilities")
                                .and_then(Value::as_array)
                                .is_some_and(|c| c == &vec![json!("gpu")])
                            || d.get("count").is_some_and(|v| v != "all" && v != 1)
                        {
                            return Err(
                                "Only NVIDIA GPU reservations (all or one device) are supported"
                                    .into(),
                            );
                        }
                        e.gpu = true
                    }
                }
            }
        }
        if let Some(files) = s.get("env_file") {
            let files = if let Some(a) = files.as_array() {
                a.clone()
            } else {
                vec![files.clone()]
            };
            for file in files {
                e.environment.extend(env_file(
                    &directory.join(file.as_str().ok_or("env_file must name a file")?),
                )?)
            }
        }
        if let Some(vars) = s.get("environment") {
            match vars {
                Value::Object(o) => {
                    for (k, v) in o {
                        e.environment.insert(
                            k.clone(),
                            if v.is_null() {
                                env.get(k)
                                    .ok_or_else(|| format!("Set environment variable {k}"))?
                                    .clone()
                            } else {
                                scalar(v)?
                            },
                        );
                    }
                }
                Value::Array(a) => {
                    for v in a {
                        let s = v
                            .as_str()
                            .ok_or("Environment list entries must be strings")?;
                        let (k, value) = match s.split_once('=') {
                            Some((k, v)) => (k, v.to_owned()),
                            None => (
                                s,
                                env.get(s)
                                    .ok_or_else(|| format!("Set environment variable {s}"))?
                                    .clone(),
                            ),
                        };
                        e.environment.insert(k.into(), value);
                    }
                }
                _ => return Err("Invalid environment mapping".into()),
            }
        }
        for field in ["ports", "expose"] {
            if let Some(ports) = s.get(field) {
                for port in ports.as_array().ok_or("ports/expose must be a list")? {
                    let p = if let Some(o) = port.as_object() {
                        supported(o, &["target", "published", "protocol", "host_ip"], key)?;
                        if o.get("protocol").is_some_and(|v| v != "tcp")
                            || o.get("host_ip").is_some_and(|v| v != "127.0.0.1")
                        {
                            return Err("Only loopback TCP published ports are supported".into());
                        }
                        Port::Detailed {
                            target: scalar(&port["target"])?
                                .parse()
                                .map_err(|_| "Invalid target port")?,
                            published: o
                                .get("published")
                                .map(|v| {
                                    scalar(v)?
                                        .parse::<u16>()
                                        .map_err(|_| "Invalid host port".to_string())
                                })
                                .transpose()?,
                        }
                    } else {
                        Port::Mapping(scalar(port)?)
                    };
                    p.values()?;
                    e.ports.push(p)
                }
            }
        }
        if let Some(volumes) = s.get("volumes") {
            for v in volumes.as_array().ok_or("volumes must be a list")? {
                let mount = if let Some(o) = v.as_object() {
                    supported(o, &["type", "source", "target", "read_only"], key)?;
                    let kind = v["type"].as_str().unwrap_or("volume");
                    if !["volume", "bind"].contains(&kind) {
                        return Err(
                            "Only named volumes and folder bind mounts can be imported".into()
                        );
                    }
                    Mount {
                        source: v["source"].as_str().ok_or("Volume source required")?.into(),
                        target: v["target"].as_str().ok_or("Volume target required")?.into(),
                        read_only: v["read_only"].as_bool().unwrap_or(false),
                        bind: kind == "bind",
                    }
                } else {
                    let s = v.as_str().ok_or("Volume must be a string or mapping")?;
                    let parts = s.rsplitn(3, ':').collect::<Vec<_>>();
                    let (source, target, ro) = match parts.as_slice() {
                        [target, source] => (*source, *target, false),
                        [mode, target, source] if *mode == "ro" || *mode == "rw" => {
                            (*source, *target, *mode == "ro")
                        }
                        _ => {
                            return Err(
                                "Use source:target[:ro] or long syntax for Windows paths".into()
                            )
                        }
                    };
                    Mount {
                        source: source.into(),
                        target: target.into(),
                        read_only: ro,
                        bind: source.starts_with(['.', '/', '~'])
                            || Path::new(source).is_absolute(),
                    }
                };
                if !mount.bind && !declared_volumes.contains(&mount.source) {
                    return Err(format!("Undeclared Compose volume {}", mount.source));
                }
                if mount.bind {
                    e.permissions.pc = true;
                    e.permissions.edit |= !mount.read_only
                }
                e.volumes.push(mount);
            }
        }
        if let Some(dep) = s.get("depends_on") {
            e.depends_on = if let Some(a) = dep.as_array() {
                a.iter().map(scalar).collect::<Result<_, _>>()?
            } else {
                let deps = dep.as_object().ok_or("Invalid depends_on")?;
                for (name, config) in deps {
                    if config["condition"] != "service_started"
                        || config
                            .as_object()
                            .is_none_or(|o| o.keys().any(|k| k != "condition"))
                    {
                        return Err(format!("{key} depends on {name}: only service_started is supported; health/completion gates must not be discarded"));
                    }
                }
                deps.keys().cloned().collect()
            };
        }
        let networks = match s.get("networks") {
            None => ["default".into()].into_iter().collect(),
            Some(Value::Array(a)) => a.iter().map(scalar).collect::<Result<_, _>>()?,
            Some(Value::Object(o)) => {
                if o.values()
                    .any(|v| !v.is_null() && v.as_object().is_none_or(|m| !m.is_empty()))
                {
                    return Err("Network aliases/static addresses cannot be imported".into());
                }
                o.keys().cloned().collect()
            }
            _ => return Err("Invalid service networks".into()),
        };
        service_networks.insert(key.clone(), networks);
        p.environments.insert(key.clone(), e);
    }
    // Compose network peers are mutually reachable, not just depends_on pairs.
    for (a, na) in &service_networks {
        for (b, nb) in &service_networks {
            if a != b && !na.is_disjoint(nb) {
                p.connections.push(Connection::Arrow(format!("{a} -> {b}")))
            }
        }
    }
    p.validate()?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn translates_images_dependencies_ports_environment_and_named_data() {
        let p=convert_text("name: test\nservices:\n  db:\n    image: postgres:17\n    environment: {POSTGRES_PASSWORD: '${PASSWORD:?required}'}\n    volumes: [data:/var/lib/postgresql/data]\n  web:\n    image: node:24\n    command: ['node', 'server.js']\n    ports: ['8080:80']\n    depends_on: [db]\nvolumes: {data: {}}",Path::new("."),&HashMap::from([("PASSWORD".into(),"do-not-log".into())])).unwrap();
        assert_eq!(p.order().unwrap(), vec!["db", "web"]);
        assert_eq!(p.connections.len(), 2);
        assert_eq!(
            p.environments["web"].ports[0].values().unwrap(),
            (80, Some(8080))
        );
        assert_eq!(
            p.environments["db"].environment["POSTGRES_PASSWORD"],
            "do-not-log"
        );
    }
    #[test]
    fn never_silently_drops_unsafe_or_unsupported_compose_behavior() {
        for extra in [
            "    privileged: true",
            "    build: .",
            "    network_mode: host",
            "    healthcheck: {test: [CMD, 'true']}",
        ] {
            assert!(convert_text(
                &format!("name: test\nservices:\n  app:\n    image: alpine\n{extra}"),
                Path::new("."),
                &HashMap::new()
            )
            .is_err())
        }
    }
    #[test]
    fn expansion_is_strict_and_supports_escapes() {
        let e = HashMap::from([("EMPTY".into(), "".into())]);
        assert_eq!(
            interpolate("$$HOME ${EMPTY:-fallback} ${MISSING-default}", &e).unwrap(),
            "$HOME fallback default"
        );
        assert!(interpolate("${MISSING}", &e).is_err())
    }
}
